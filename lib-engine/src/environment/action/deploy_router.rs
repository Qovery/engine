use crate::environment::action::DeploymentAction;
use crate::environment::action::check_dns::CheckDnsForDomains;
use crate::environment::action::deploy_helm::{HelmChartValues, HelmDeployment};
use crate::environment::action::helm_release_inventory::installed_helm_releases;
use crate::environment::action::router_listener_set_ownership::{
    claim_hostname_release, delete_claim, installed_release, list_claims, list_legacy_release_owners,
};
use crate::environment::models::router::Router;
use crate::environment::models::router_listener_set::HostnameListenerSet;
use crate::environment::models::router_listener_set_ownership::{
    ClaimError, release_stale_claim, remove_if_owned, stale_claims, stale_legacy_releases, unclaimed_hostname_releases,
};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::router::reporter::RouterDeploymentReporter;
use crate::environment::report::{DeploymentTaskRef, execute_long_deployment};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::infrastructure::models::kubernetes::Kind;
use crate::io_models::models::CustomDomain;
use crate::runtime::block_on;
use crate::{
    cmd::kubectl::{
        gateway_fallback_certificate_ref_annotation_key, is_engine_gateway_fallback_reference_grant,
        kubectl_check_gateway_api_crds_available, kubectl_ensure_gateway_to_secret_reference_grant,
        kubectl_gateway_crd_supports_allowed_listeners, kubectl_get_gateway_api_served_version,
        kubectl_get_reference_grant_served_version, kubectl_reconcile_gateway_certrefs_for_router_tls_secrets,
    },
    errors::CommandError,
};

use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use json_patch::{AddOperation, PatchOperation, RemoveOperation, TestOperation};
use jsonptr::PointerBuf;
use kube::Api as TypedApi;
use kube::api::{Api, ApiResource, DeleteParams, Patch, PatchParams};
use kube::core::GroupVersionKind;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;
use tera::Context as TeraContext;

impl<T: CloudProvider> DeploymentAction for Router<T>
where
    Router<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let pre_run = |_: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };
        let run = |logger: &EnvProgressLogger, _: ()| -> Result<(), Box<EngineError>> {
            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir(target)),
                HelmChartValues::SerializedContext,
                chart,
            );

            // The routes below attach to these ListenerSets, so they are applied first.
            deploy_hostname_listener_sets(self, target, logger)?;

            helm.on_create(target)?;

            maybe_patch_gateway_cert_refs_for_custom_domains(self, target, logger)?;

            // check non custom domains
            let custom_domains_to_check = self
                .custom_domains
                .clone()
                .into_iter()
                .filter(|it| !it.use_cdn)
                .collect::<Vec<CustomDomain>>();

            let domain_checker = CheckDnsForDomains {
                resolve_to_ip: vec![self.default_domain.clone()],
                resolve_to_cname: custom_domains_to_check,
                log: Box::new(move |msg| logger.info(msg)),
            };
            let _ = domain_checker.on_create(target);

            Ok(())
        };

        let empty_post_run = |_logger: &EnvSuccessLogger, _: ()| {};

        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Create),
            DeploymentTaskRef {
                pre_run: &pre_run,
                run: &run,
                post_run_success: &empty_post_run,
            },
        )
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Pause),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) },
        )
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Delete),
            |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                let chart = ChartInfo {
                    name: self.helm_release_name(),
                    namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
                    action: HelmAction::Destroy,
                    ..Default::default()
                };
                let helm = HelmDeployment::new(
                    self.get_event_details(Stage::Environment(EnvironmentStep::Delete)),
                    self.to_tera_context(target)?,
                    PathBuf::from(self.helm_chart_dir(target).as_str()),
                    HelmChartValues::SerializedContext,
                    chart,
                );

                let result = helm.on_delete(target).and_then(|()| {
                    // These releases belong to this router alone, nothing else uses them.
                    uninstall_hostname_listener_sets(self, target, &BTreeSet::new(), EnvironmentStep::Delete, logger)
                });

                if result.is_ok()
                    && let Err(error) = maybe_remove_gateway_fallback_resources_for_custom_domains(self, target, logger)
                {
                    logger.warning(format!(
                        "Failed to remove GKE Gateway API fallback resources for custom domain TLS: {error}"
                    ));
                }

                result
                // FIXME: Delete also certificates
            },
        )
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Restart),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) },
        )
    }
}

/// Applies one Helm release per hostname of the router in `PER_HOSTNAME_LISTENER_SET` mode.
///
/// A release only depends on the hostname, so applying it again changes nothing.
/// A release already owned by another router is never taken over, the deployment fails instead.
/// Releases of hostnames the router no longer serves are removed. In the legacy mode nothing is applied and the
/// releases left by a previous `PER_HOSTNAME_LISTENER_SET` mode are removed before the router chart is applied.
fn deploy_hostname_listener_sets<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>>
where
    Router<T>: ToTeraContext,
{
    let event_details = router.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
    let listener_sets = router.hostname_listener_sets(target)?;

    if !listener_sets.is_empty() && !Router::<T>::listenerset_is_deployable(target) {
        return Err(Box::new(EngineError::new_invalid_engine_payload(
            event_details,
            "PER_HOSTNAME_LISTENER_SET needs the Gateway API and ListenerSet CRDs on the cluster, \
             there is no fallback to another listener",
            None,
        )));
    }

    // Every claim is taken before the first release is applied, so a hostname held by another router
    // fails the deployment before anything changed.
    let router_long_id = router.long_id.to_string();
    for listener_set in &listener_sets {
        claim_hostname_release(target, &router_long_id, &listener_set.name, &listener_set.hostname)
            .map_err(|error| claim_error(&event_details, listener_set, error))?;
    }
    for listener_set in &listener_sets {
        logger.info(format!(
            "Applying ListenerSet release `{}` for hostname `{}`",
            listener_set.name, listener_set.hostname
        ));
        let workspace_directory = router
            .workspace_directory
            .with_file_name(format!("{}-listenersets", router.long_id))
            .join(&listener_set.name);
        std::fs::create_dir_all(&workspace_directory).map_err(|error| {
            Box::new(EngineError::new_cannot_create_file(
                event_details.clone(),
                CommandError::new_from_safe_message(format!(
                    "cannot create `{}`: {error}",
                    workspace_directory.display()
                )),
            ))
        })?;

        let helm = HelmDeployment::new(
            event_details.clone(),
            hostname_listener_set_chart_values(router, target, listener_set),
            PathBuf::from(router.hostname_listener_set_chart_dir()),
            HelmChartValues::SerializedContext,
            ChartInfo {
                name: listener_set.name.clone(),
                path: workspace_directory.to_string_lossy().to_string(),
                namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
                ..Default::default()
            },
        );
        helm.on_create(target)?;
    }

    let wanted: BTreeSet<&str> = listener_sets
        .iter()
        .map(|listener_set| listener_set.name.as_str())
        .collect();
    uninstall_hostname_listener_sets(router, target, &wanted, EnvironmentStep::Deploy, logger)
}

fn release_inventory_error(event_details: EventDetails, error: CommandError) -> Box<EngineError> {
    Box::new(EngineError::new_unknown(
        event_details,
        "Cannot list the Helm releases of the environment".to_string(),
        Some(error),
        None,
        None,
    ))
}

fn ownership_store_error(event_details: EventDetails, error: CommandError) -> Box<EngineError> {
    Box::new(EngineError::new_unknown(
        event_details,
        "Cannot read or write the ownership of the ListenerSet releases".to_string(),
        Some(error),
        None,
        None,
    ))
}

fn claim_error(
    event_details: &EventDetails,
    listener_set: &HostnameListenerSet,
    error: ClaimError<CommandError>,
) -> Box<EngineError> {
    let message = match error {
        ClaimError::Foreign { owner } => format!(
            "ListenerSet release `{}` for hostname `{}` is owned by router `{owner}`. \
             A hostname must be served by a single router, the release is not taken over",
            listener_set.name, listener_set.hostname
        ),
        ClaimError::Unverifiable { reason } => format!(
            "ListenerSet release `{}` for hostname `{}` is not applied: {reason}",
            listener_set.name, listener_set.hostname
        ),
        ClaimError::Store(error) => return ownership_store_error(event_details.clone(), error),
    };
    Box::new(EngineError::new_invalid_engine_payload(event_details.clone(), &message, None))
}

fn hostname_listener_set_chart_values<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    listener_set: &HostnameListenerSet,
) -> TeraContext {
    let mut context = TeraContext::new();
    context.insert("name", &listener_set.name);
    context.insert("hostname", &listener_set.hostname);
    context.insert("tls_secret_name", &listener_set.tls_secret_name);
    context.insert("namespaces", &listener_set.namespaces);
    context.insert("router_long_id", &router.long_id);
    context.insert("router_name", &router.kube_name);
    context.insert("environment_long_id", &target.environment.long_id);
    context.insert("project_long_id", &target.environment.project_long_id);
    context
}

/// Uninstalls the hostname releases of the router that are not in `wanted`, then releases their claims.
///
/// In `PER_HOSTNAME_LISTENER_SET` mode a failure fails the deployment. In the legacy mode these releases only
/// exist after a switch back, so a failure is a warning and never blocks a legacy deployment.
/// Only the claims held by this router and its own releases of the previous naming scheme are removed.
/// `step` is the environment step of the caller, so a failure is reported as a deploy or a delete error.
fn uninstall_hostname_listener_sets<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    wanted: &BTreeSet<&str>,
    step: EnvironmentStep,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>>
where
    Router<T>: ToTeraContext,
{
    let strict = router.uses_hostname_listener_sets(target);
    if !strict
        && !target
            .kubernetes
            .advanced_settings()
            .k8s_deploy_api_gateway
            .unwrap_or(false)
    {
        // Without the Gateway API this mode never created anything.
        return Ok(());
    }

    let event_details = router.get_event_details(Stage::Environment(step));
    let namespace = target.environment.namespace();
    // Read from the Secret metadata: `helm list -a` loads every revision of every release, which can run out of memory.
    let releases =
        match installed_helm_releases(target.kube.client(), namespace, target.helm.storage_driver().as_deref()) {
            Ok(releases) => releases,
            Err(error) if strict => return Err(release_inventory_error(event_details, error)),
            Err(error) => {
                logger.warning(format!("Cannot list the ListenerSet releases of the router: {error}"));
                return Ok(());
            }
        };

    let router_long_id = router.long_id.to_string();
    let installed: BTreeSet<&str> = releases.iter().map(|release| release.name.as_str()).collect();
    let (claims, legacy_owners) = match (list_claims(target), list_legacy_release_owners(target)) {
        (Ok(claims), Ok(legacy_owners)) => (claims, legacy_owners),
        (Err(error), _) | (_, Err(error)) if strict => return Err(ownership_store_error(event_details, error)),
        (Err(error), _) | (_, Err(error)) => {
            logger.warning(format!("Cannot read the ownership of the ListenerSet releases: {error}"));
            return Ok(());
        }
    };

    let store_error = |error: CommandError| ownership_store_error(event_details.clone(), error);

    // The release goes first and its claim last: until the release is gone, no other router can claim it.
    // Whether the release is the router's is read from its Helm manifest right before it is uninstalled.
    // A failure keeps the claim, the next deployment removes what is left.
    for claim in stale_claims(&router_long_id, &claims, wanted) {
        let result = release_stale_claim(
            &router_long_id,
            claim,
            || installed_release(target, &claim.release).map_err(store_error),
            || uninstall_release(router, target, &event_details, &claim.release, logger),
            || delete_claim(target, claim).map_err(store_error),
        );
        match result {
            Ok(()) => {}
            Err(ClaimError::Store(error)) if strict => return Err(error),
            Err(error) => logger.warning(format!(
                "Cannot remove ListenerSet release `{}`: {}",
                claim.release,
                describe_cleanup_error(error)
            )),
        }
    }

    // Releases applied before claims existed have a hostname name and no claim, they are not left behind either.
    for release_name in unclaimed_hostname_releases(installed.iter().copied(), &claims, wanted) {
        let result = remove_if_owned(
            &router_long_id,
            || installed_release(target, release_name).map_err(store_error),
            || uninstall_release(router, target, &event_details, release_name, logger),
        );
        match result {
            Ok(_) => {}
            Err(error) if strict => return Err(error),
            Err(error) => logger.warning(format!("Cannot remove ListenerSet release `{release_name}`: {error}")),
        }
    }

    // Releases of the previous naming scheme, `listenerset-<router id>-<hash>`, are never wanted.
    for release_name in stale_legacy_releases(&router.id, &router_long_id, &legacy_owners, installed.iter().copied()) {
        match uninstall_release(router, target, &event_details, &release_name, logger) {
            Ok(()) => {}
            Err(error) if strict => return Err(error),
            Err(error) => logger.warning(format!("Cannot remove ListenerSet release `{release_name}`: {error}")),
        }
    }
    Ok(())
}

fn describe_cleanup_error(error: ClaimError<Box<EngineError>>) -> String {
    match error {
        ClaimError::Foreign { owner } => format!("the claim belongs to router `{owner}`"),
        ClaimError::Unverifiable { reason } => reason.to_string(),
        ClaimError::Store(error) => error.to_string(),
    }
}

/// Uninstalls a release of the environment namespace.
fn uninstall_release<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    event_details: &EventDetails,
    release_name: &str,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>>
where
    Router<T>: ToTeraContext,
{
    logger.info(format!("Removing ListenerSet release `{release_name}`"));
    let helm = HelmDeployment::new(
        event_details.clone(),
        TeraContext::new(),
        PathBuf::from(router.hostname_listener_set_chart_dir()),
        HelmChartValues::SerializedContext,
        ChartInfo {
            name: release_name.to_string(),
            namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
            action: HelmAction::Destroy,
            ..Default::default()
        },
    );
    helm.on_delete(target)
}

fn maybe_patch_gateway_cert_refs_for_custom_domains<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
    // This fallback references the legacy `router-tls-<id>` Secret, which this mode does not create.
    if router.uses_hostname_listener_sets(target) {
        return Ok(());
    }

    let gateway_api_enabled = target
        .kubernetes
        .advanced_settings()
        .k8s_deploy_api_gateway
        .unwrap_or(false)
        && target
            .kubernetes
            .advanced_settings()
            .k8s_use_api_gateway
            .unwrap_or(false);

    if !gateway_api_enabled {
        return Ok(());
    }

    // Managed GKE has historically lagged on ListenerSet attachment support even when the
    // broader Gateway API stack is enabled. Keep the provider-specific fallback centralized
    // here so AWS/Azure/Scaleway continue to use the standard ListenerSet path.
    if !matches!(target.kubernetes.kind(), Kind::Gke) {
        return Ok(());
    }

    let has_custom_certs = router.custom_domains.iter().any(|domain| domain.generate_certificate);
    if !has_custom_certs {
        return Ok(());
    }

    if !kubectl_check_gateway_api_crds_available(&target.kube.client()) {
        logger.warning("Gateway API CRDs not detected; skipping Gateway certificateRef fallback patch.".to_string());
        return Ok(());
    }

    if kubectl_gateway_crd_supports_allowed_listeners(&target.kube.client()) {
        logger
            .info("Gateway CRD exposes allowedListeners; skipping Gateway certificateRef fallback patch.".to_string());
        return Ok(());
    }

    let secret_name = format!("router-tls-{}", router.id);
    let secret_namespace = target.environment.namespace().to_string();

    logger.info(format!(
        "GKE Gateway API ListenerSet fallback: ensuring Gateway uses TLS secret {secret_namespace}/{secret_name}."
    ));

    if let Err(e) = kubectl_ensure_gateway_to_secret_reference_grant(
        &target.kube.client(),
        "qovery",
        &secret_namespace,
        &secret_name,
    ) {
        logger.warning(format!(
            "Failed to ensure ReferenceGrant for Gateway TLS secret {secret_namespace}/{secret_name}: {e}"
        ));
        return Err(Box::new(EngineError::new_router_failed_to_deploy(
            router.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
        )));
    }

    // Reconciliation removes stale fallback references before checking the listener limit. A
    // failed reconciliation must not prevent the direct update below from returning its more
    // actionable capacity error.
    match kubectl_reconcile_gateway_certrefs_for_router_tls_secrets(
        &target.kube.client(),
        "qovery",
        "qovery-cluster-public-gateway",
        "https",
    ) {
        Ok(true) => logger.info("Gateway certificateRefs reconciled before router deployment.".to_string()),
        Ok(false) => logger.info("Gateway certificateRefs were already reconciled before router deployment.".to_string()),
        Err(error) => logger.warning(format!(
            "Failed to reconcile Gateway certificateRefs before router deployment; continuing with the direct fallback update: {error}"
        )),
    }

    match ensure_gateway_certificate_ref(
        target.kube.client(),
        "qovery",
        "qovery-cluster-public-gateway",
        "https",
        &secret_namespace,
        &secret_name,
    ) {
        Ok(true) => logger.info("Gateway certificateRefs patched.".to_string()),
        Ok(false) => logger.info("Gateway certificateRefs already up to date.".to_string()),
        Err(e) => {
            logger.warning(format!("Failed to patch Gateway certificateRefs for custom domain TLS: {e}"));
            return Err(Box::new(EngineError::new_router_failed_to_deploy(
                router.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
            )));
        }
    }

    Ok(())
}

fn ensure_gateway_certificate_ref(
    kube_client: kube::Client,
    gateway_namespace: &str,
    gateway_name: &str,
    listener_name: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<bool, CommandError> {
    let api_version = kubectl_get_gateway_api_served_version(&kube_client).unwrap_or_else(|| "v1".to_string());
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &api_version, "Gateway");
    let api: TypedApi<kube::core::DynamicObject> =
        TypedApi::namespaced_with(kube_client, gateway_namespace, &ApiResource::from_gvk(&gvk));

    for _ in 0..MAX_GATEWAY_CERTIFICATE_REF_MUTATION_ATTEMPTS {
        let gateway = block_on(api.get(gateway_name)).map_err(|error| {
            CommandError::new_from_safe_message(format!(
                "Failed to fetch Gateway {gateway_namespace}/{gateway_name}: {error}"
            ))
        })?;
        let Some(patch) = gateway_certificate_ref_ensure_patch(
            &gateway,
            gateway_namespace,
            gateway_name,
            listener_name,
            secret_namespace,
            secret_name,
        )?
        else {
            return Ok(false);
        };

        let patch: Patch<kube::core::DynamicObject> = Patch::Json(patch);
        match block_on(api.patch(gateway_name, &PatchParams::default(), &patch)) {
            Ok(_) => return Ok(true),
            Err(error) if is_kubernetes_conflict(&error) => continue,
            Err(error) => {
                return Err(CommandError::new_from_safe_message(format!(
                    "Failed to patch Gateway {gateway_namespace}/{gateway_name} certificateRefs: {error}"
                )));
            }
        }
    }

    Err(CommandError::new_from_safe_message(format!(
        "Failed to add Gateway {gateway_namespace}/{gateway_name} certificateRef for \
{secret_namespace}/{secret_name}: the Gateway changed concurrently"
    )))
}

const MAX_GATEWAY_CERTIFICATE_REFS: usize = 64;
const MAX_GATEWAY_CERTIFICATE_REF_MUTATION_ATTEMPTS: usize = 3;

fn gateway_certificate_ref_ensure_patch(
    gateway: &kube::core::DynamicObject,
    gateway_namespace: &str,
    gateway_name: &str,
    listener_name: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<Option<json_patch::Patch>, CommandError> {
    let resource_version = gateway.metadata.resource_version.as_ref().ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Gateway {} has no resourceVersion",
            gateway.metadata.name.as_deref().unwrap_or("unknown")
        ))
    })?;
    let listeners = gateway
        .data
        .get("spec")
        .and_then(|spec| spec.get("listeners"))
        .and_then(Value::as_array)
        .ok_or_else(|| CommandError::new_from_safe_message("Gateway has no spec.listeners".to_string()))?;
    let listener_index = listeners
        .iter()
        .position(|listener| listener.get("name").and_then(Value::as_str) == Some(listener_name))
        .ok_or_else(|| CommandError::new_from_safe_message(format!("Gateway has no '{listener_name}' listener")))?;
    let certificate_refs = listeners[listener_index]
        .get("tls")
        .and_then(|tls| tls.get("certificateRefs"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} listener '{listener_name}' has no tls.certificateRefs"
            ))
        })?;
    let certificate_ref_exists = certificate_refs.iter().any(|reference| {
        reference.get("name").and_then(Value::as_str) == Some(secret_name)
            && reference.get("namespace").and_then(Value::as_str) == Some(secret_namespace)
    });
    let annotation_key = gateway_fallback_certificate_ref_annotation_key(secret_name);
    let annotation_exists = gateway
        .metadata
        .annotations
        .as_ref()
        .is_some_and(|annotations| annotations.get(&annotation_key) == Some(&secret_namespace.to_string()));
    if certificate_ref_exists && annotation_exists {
        return Ok(None);
    }
    if !certificate_ref_exists {
        ensure_gateway_certificate_ref_capacity(
            certificate_refs.len(),
            gateway_namespace,
            gateway_name,
            listener_name,
            secret_namespace,
            secret_name,
        )?;
    }

    let mut patch_operations = vec![PatchOperation::Test(TestOperation {
        path: gateway_json_pointer("/metadata/resourceVersion")?,
        value: Value::String(resource_version.to_string()),
    })];
    if !certificate_ref_exists {
        patch_operations.push(PatchOperation::Add(AddOperation {
            path: gateway_json_pointer(format!("/spec/listeners/{listener_index}/tls/certificateRefs/-"))?,
            value: json!({
                "kind": "Secret",
                "name": secret_name,
                "namespace": secret_namespace,
            }),
        }));
    }
    match gateway.metadata.annotations.as_ref() {
        Some(_) => patch_operations.push(PatchOperation::Add(AddOperation {
            path: gateway_json_pointer(format!("/metadata/annotations/{}", json_pointer_segment(&annotation_key)))?,
            value: Value::String(secret_namespace.to_string()),
        })),
        None => patch_operations.push(PatchOperation::Add(AddOperation {
            path: gateway_json_pointer("/metadata/annotations")?,
            value: json!({ annotation_key: secret_namespace }),
        })),
    }

    Ok(Some(json_patch::Patch(patch_operations)))
}

fn gateway_json_pointer(path: impl AsRef<str>) -> Result<PointerBuf, CommandError> {
    PointerBuf::parse(path.as_ref())
        .map_err(|error| CommandError::new_from_safe_message(format!("Invalid Gateway patch path: {error}")))
}

fn json_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn ensure_gateway_certificate_ref_capacity(
    certificate_ref_count: usize,
    gateway_namespace: &str,
    gateway_name: &str,
    listener_name: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<(), CommandError> {
    if certificate_ref_count < MAX_GATEWAY_CERTIFICATE_REFS {
        return Ok(());
    }

    Err(CommandError::new_from_safe_message(format!(
        "Cannot attach custom-domain TLS Secret {secret_namespace}/{secret_name} to Gateway \
{gateway_namespace}/{gateway_name} listener '{listener_name}': it already has \
{certificate_ref_count} certificateRefs, but Gateway API allows at most \
{MAX_GATEWAY_CERTIFICATE_REFS}. On GKE this is a temporary fallback while ListenerSet attachment \
is unavailable. Stale Qovery router references are reconciled before this check; remove unused \
active routers or enable ListenerSet support before retrying."
    )))
}

/// Removes the GKE fallback Gateway resources after its router has been deleted.
///
/// The certificate reference and ReferenceGrant are created outside the router Helm release, so
/// Helm cannot remove them during uninstall. Leaving either resource behind lets cluster
/// reconciliation restore the stale reference and eventually exhaust the shared listener limit.
fn maybe_remove_gateway_fallback_resources_for_custom_domains<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) -> Result<(), CommandError> {
    let gateway_api_enabled = target
        .kubernetes
        .advanced_settings()
        .k8s_deploy_api_gateway
        .unwrap_or(false)
        && target
            .kubernetes
            .advanced_settings()
            .k8s_use_api_gateway
            .unwrap_or(false);

    if !gateway_api_enabled || !matches!(target.kubernetes.kind(), Kind::Gke) {
        return Ok(());
    }

    if !router.custom_domains.iter().any(|domain| domain.generate_certificate) {
        return Ok(());
    }

    let secret_namespace = target.environment.namespace().to_string();
    let secret_name = format!("router-tls-{}", router.id);

    logger.info(format!(
        "Removing GKE Gateway API fallback resources for {secret_namespace}/{secret_name}."
    ));

    let reference_grant_cleanup =
        remove_gateway_to_secret_reference_grant(target.kube.client(), "qovery", &secret_namespace, &secret_name);
    let certificate_ref_cleanup = remove_gateway_certificate_ref(
        target.kube.client(),
        "qovery",
        "qovery-cluster-public-gateway",
        "https",
        &secret_namespace,
        &secret_name,
    );

    match &reference_grant_cleanup {
        Ok(true) => logger.info("Gateway ReferenceGrant cleanup completed.".to_string()),
        Ok(false) => logger.info("Gateway ReferenceGrant cleanup was not needed.".to_string()),
        Err(_) => {}
    }

    match &certificate_ref_cleanup {
        Ok(true) => logger.info("Gateway certificateRef cleanup completed.".to_string()),
        Ok(false) => logger.info("Gateway certificateRef cleanup was not needed.".to_string()),
        Err(_) => {}
    }

    combine_gateway_fallback_cleanup_results(
        reference_grant_cleanup,
        certificate_ref_cleanup,
        &secret_namespace,
        &secret_name,
    )
}

fn combine_gateway_fallback_cleanup_results(
    reference_grant_cleanup: Result<bool, CommandError>,
    certificate_ref_cleanup: Result<bool, CommandError>,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<(), CommandError> {
    let mut cleanup_errors = Vec::new();

    if let Err(error) = reference_grant_cleanup {
        cleanup_errors.push(format!("ReferenceGrant: {error}"));
    }
    if let Err(error) = certificate_ref_cleanup {
        cleanup_errors.push(format!("certificateRef: {error}"));
    }

    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        Err(CommandError::new_from_safe_message(format!(
            "Failed to fully clean GKE Gateway API fallback resources for {secret_namespace}/{secret_name}: {}",
            cleanup_errors.join("; ")
        )))
    }
}

fn remove_gateway_to_secret_reference_grant(
    kube_client: kube::Client,
    gateway_namespace: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<bool, CommandError> {
    let api_version = kubectl_get_reference_grant_served_version(&kube_client).unwrap_or_else(|| "v1beta1".to_string());
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &api_version, "ReferenceGrant");
    let api: Api<kube::core::DynamicObject> =
        Api::namespaced_with(kube_client, secret_namespace, &ApiResource::from_gvk(&gvk));
    let grant_name = format!("allow-gateway-to-{secret_name}");

    let Some(reference_grant) = block_on(api.get_opt(&grant_name)).map_err(|error| {
        CommandError::new_from_safe_message(format!(
            "Failed to fetch ReferenceGrant {secret_namespace}/{grant_name}: {error}"
        ))
    })?
    else {
        return Ok(false);
    };

    if !reference_grant_allows_gateway_to_secret(&reference_grant, gateway_namespace, secret_name)
        || !is_engine_gateway_fallback_reference_grant(&reference_grant, secret_name)
    {
        return Ok(false);
    }

    block_on(api.delete(&grant_name, &DeleteParams::background())).map_err(|error| {
        CommandError::new_from_safe_message(format!(
            "Failed to delete ReferenceGrant {secret_namespace}/{grant_name}: {error}"
        ))
    })?;

    Ok(true)
}

fn reference_grant_allows_gateway_to_secret(
    reference_grant: &kube::core::DynamicObject,
    gateway_namespace: &str,
    secret_name: &str,
) -> bool {
    let Some(spec) = reference_grant.data.get("spec") else {
        return false;
    };
    let allows_gateway = spec.get("from").and_then(Value::as_array).is_some_and(|sources| {
        sources.iter().any(|source| {
            source.get("group").and_then(Value::as_str) == Some("gateway.networking.k8s.io")
                && source.get("kind").and_then(Value::as_str) == Some("Gateway")
                && source.get("namespace").and_then(Value::as_str) == Some(gateway_namespace)
        })
    });
    let allows_secret = spec.get("to").and_then(Value::as_array).is_some_and(|targets| {
        targets.iter().any(|target| {
            target.get("group").and_then(Value::as_str) == Some("")
                && target.get("kind").and_then(Value::as_str) == Some("Secret")
                && target.get("name").and_then(Value::as_str) == Some(secret_name)
        })
    });

    allows_gateway && allows_secret
}

fn remove_gateway_certificate_ref(
    kube_client: kube::Client,
    gateway_namespace: &str,
    gateway_name: &str,
    listener_name: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<bool, CommandError> {
    let api_version = kubectl_get_gateway_api_served_version(&kube_client).unwrap_or_else(|| "v1".to_string());
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &api_version, "Gateway");
    let api: TypedApi<kube::core::DynamicObject> =
        TypedApi::namespaced_with(kube_client, gateway_namespace, &ApiResource::from_gvk(&gvk));

    for _ in 0..MAX_GATEWAY_CERTIFICATE_REF_REMOVAL_ATTEMPTS {
        let gateway = block_on(api.get(gateway_name)).map_err(|error| {
            CommandError::new_from_safe_message(format!(
                "Failed to fetch Gateway {gateway_namespace}/{gateway_name}: {error}"
            ))
        })?;
        let Some(patch) = gateway_certificate_ref_remove_patch(&gateway, listener_name, secret_namespace, secret_name)?
        else {
            return Ok(false);
        };

        let patch: Patch<kube::core::DynamicObject> = Patch::Json(patch);
        match block_on(api.patch(gateway_name, &PatchParams::default(), &patch)) {
            Ok(_) => return Ok(true),
            Err(error) if is_kubernetes_conflict(&error) => continue,
            Err(error) => {
                return Err(CommandError::new_from_safe_message(format!(
                    "Failed to patch Gateway {gateway_namespace}/{gateway_name} certificateRefs: {error}"
                )));
            }
        }
    }

    Err(CommandError::new_from_safe_message(format!(
        "Failed to remove Gateway {gateway_namespace}/{gateway_name} certificateRef for \
{secret_namespace}/{secret_name}: the Gateway changed concurrently during cleanup"
    )))
}

const MAX_GATEWAY_CERTIFICATE_REF_REMOVAL_ATTEMPTS: usize = 3;

fn gateway_certificate_ref_remove_patch(
    gateway: &kube::core::DynamicObject,
    listener_name: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<Option<json_patch::Patch>, CommandError> {
    let resource_version = gateway.metadata.resource_version.as_ref().ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Gateway {} has no resourceVersion",
            gateway.metadata.name.as_deref().unwrap_or("unknown")
        ))
    })?;
    let listeners = gateway
        .data
        .get("spec")
        .and_then(|spec| spec.get("listeners"))
        .and_then(Value::as_array)
        .ok_or_else(|| CommandError::new_from_safe_message("Gateway has no spec.listeners".to_string()))?;
    let listener_index = listeners
        .iter()
        .position(|listener| listener.get("name").and_then(Value::as_str) == Some(listener_name))
        .ok_or_else(|| CommandError::new_from_safe_message(format!("Gateway has no '{listener_name}' listener")))?;
    let Some(certificate_refs) = listeners[listener_index]
        .get("tls")
        .and_then(|tls| tls.get("certificateRefs"))
        .and_then(Value::as_array)
    else {
        return gateway_fallback_ownership_remove_patch(gateway, secret_name, resource_version, None);
    };
    let certificate_ref_index = certificate_refs.iter().position(|reference| {
        reference.get("name").and_then(Value::as_str) == Some(secret_name)
            && reference.get("namespace").and_then(Value::as_str) == Some(secret_namespace)
    });

    gateway_fallback_ownership_remove_patch(
        gateway,
        secret_name,
        resource_version,
        certificate_ref_index.map(|index| format!("/spec/listeners/{listener_index}/tls/certificateRefs/{index}")),
    )
}

fn gateway_fallback_ownership_remove_patch(
    gateway: &kube::core::DynamicObject,
    secret_name: &str,
    resource_version: &str,
    certificate_ref_path: Option<String>,
) -> Result<Option<json_patch::Patch>, CommandError> {
    let annotation_key = gateway_fallback_certificate_ref_annotation_key(secret_name);
    let annotation_exists = gateway
        .metadata
        .annotations
        .as_ref()
        .is_some_and(|annotations| annotations.contains_key(&annotation_key));
    if certificate_ref_path.is_none() && !annotation_exists {
        return Ok(None);
    }

    let mut patch_operations = vec![PatchOperation::Test(TestOperation {
        path: gateway_json_pointer("/metadata/resourceVersion")?,
        value: Value::String(resource_version.to_string()),
    })];
    if let Some(path) = certificate_ref_path {
        patch_operations.push(PatchOperation::Remove(RemoveOperation {
            path: gateway_json_pointer(path)?,
        }));
    }
    if annotation_exists {
        patch_operations.push(PatchOperation::Remove(RemoveOperation {
            path: gateway_json_pointer(format!("/metadata/annotations/{}", json_pointer_segment(&annotation_key)))?,
        }));
    }

    Ok(Some(json_patch::Patch(patch_operations)))
}

fn is_kubernetes_conflict(error: &kube::Error) -> bool {
    matches!(error, kube::Error::Api(status) if status.is_conflict())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_GATEWAY_CERTIFICATE_REFS, combine_gateway_fallback_cleanup_results,
        ensure_gateway_certificate_ref_capacity, gateway_certificate_ref_ensure_patch,
        gateway_certificate_ref_remove_patch, is_engine_gateway_fallback_reference_grant, is_kubernetes_conflict,
        reference_grant_allows_gateway_to_secret,
    };
    use crate::cmd::kubectl::{GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL, GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE};
    use crate::errors::CommandError;
    use kube::api::ApiResource;
    use kube::core::{DynamicObject, GroupVersionKind, Status};
    use serde_json::json;

    #[test]
    fn certificate_ref_capacity_error_explains_the_gke_fallback_limit() {
        let error = ensure_gateway_certificate_ref_capacity(
            MAX_GATEWAY_CERTIFICATE_REFS,
            "qovery",
            "qovery-cluster-public-gateway",
            "https",
            "environment",
            "router-tls-z1234567",
        )
        .expect_err("the Gateway API limit should reject one more certificate reference");

        let message = error.message_safe();
        assert!(message.contains("environment/router-tls-z1234567"));
        assert!(message.contains("at most 64"));
        assert!(message.contains("temporary fallback while ListenerSet attachment is unavailable"));
    }

    #[test]
    fn certificate_ref_capacity_allows_the_last_available_reference() {
        assert!(
            ensure_gateway_certificate_ref_capacity(
                MAX_GATEWAY_CERTIFICATE_REFS - 1,
                "qovery",
                "qovery-cluster-public-gateway",
                "https",
                "environment",
                "router-tls-z1234567",
            )
            .is_ok()
        );
    }

    #[test]
    fn identifies_an_engine_owned_router_fallback_reference_grant_for_cleanup() {
        let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1beta1", "ReferenceGrant");
        let api_resource = ApiResource::from_gvk(&gvk);
        let mut reference_grant = DynamicObject::new("allow-gateway-to-router-tls-z1234567", &api_resource);
        reference_grant.data = json!({
            "spec": {
                "from": [{
                    "group": "gateway.networking.k8s.io",
                    "kind": "Gateway",
                    "namespace": "qovery"
                }],
                "to": [{
                    "group": "",
                    "kind": "Secret",
                    "name": "router-tls-z1234567"
                }]
            }
        });
        reference_grant.metadata.labels = Some(std::collections::BTreeMap::from([(
            GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL.to_string(),
            GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE.to_string(),
        )]));

        assert!(reference_grant_allows_gateway_to_secret(
            &reference_grant,
            "qovery",
            "router-tls-z1234567",
        ));
        assert!(!reference_grant_allows_gateway_to_secret(
            &reference_grant,
            "other-gateway",
            "router-tls-z1234567",
        ));
        assert!(is_engine_gateway_fallback_reference_grant(
            &reference_grant,
            "router-tls-z1234567",
        ));

        reference_grant.metadata.labels = None;
        assert!(!is_engine_gateway_fallback_reference_grant(
            &reference_grant,
            "router-tls-z1234567",
        ));
    }

    #[test]
    fn retries_only_kubernetes_conflicts_during_gateway_certificate_ref_mutation() {
        let conflict = kube::Error::Api(Status::failure("Gateway changed", "Conflict").with_code(409).boxed());
        let validation_error = kube::Error::Api(
            Status::failure("certificateRefs exceeds the maximum", "Invalid")
                .with_code(422)
                .boxed(),
        );

        assert!(is_kubernetes_conflict(&conflict));
        assert!(!is_kubernetes_conflict(&validation_error));
    }

    #[test]
    fn cleanup_reports_both_failures_after_attempting_every_fallback_resource() {
        let error = combine_gateway_fallback_cleanup_results(
            Err(CommandError::new_from_safe_message(
                "ReferenceGrant API unavailable".to_string(),
            )),
            Err(CommandError::new_from_safe_message("Gateway patch forbidden".to_string())),
            "environment",
            "router-tls-z1234567",
        )
        .expect_err("both cleanup errors should be reported after both operations are attempted");

        let message = error.message_safe();
        assert!(message.contains("ReferenceGrant API unavailable"));
        assert!(message.contains("Gateway patch forbidden"));
    }

    #[test]
    fn certificate_ref_cleanup_targets_one_reference_with_a_resource_version_test() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "tls": {
                        "certificateRefs": [
                            { "name": "letsencrypt-acme-qovery-cert", "namespace": "qovery" },
                            { "name": "router-tls-z1234567", "namespace": "environment" }
                        ]
                    }
                }]
            }
        });

        let patch = gateway_certificate_ref_remove_patch(&gateway, "https", "environment", "router-tls-z1234567")
            .expect("patch creation should succeed")
            .expect("the matching certificateRef should produce a patch");

        assert_eq!(
            serde_json::to_value(&patch).expect("JSON Patch should serialize"),
            json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": "42" },
                { "op": "remove", "path": "/spec/listeners/0/tls/certificateRefs/1" }
            ])
        );
    }

    #[test]
    fn certificate_ref_creation_records_gateway_fallback_ownership_in_the_same_patch() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "tls": { "certificateRefs": [] }
                }]
            }
        });

        let patch = gateway_certificate_ref_ensure_patch(
            &gateway,
            "qovery",
            "qovery-cluster-public-gateway",
            "https",
            "environment",
            "router-tls-z1234567",
        )
        .expect("patch creation should succeed")
        .expect("a missing certificateRef should produce a patch");

        assert_eq!(
            serde_json::to_value(&patch).expect("JSON Patch should serialize"),
            json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": "42" },
                {
                    "op": "add",
                    "path": "/spec/listeners/0/tls/certificateRefs/-",
                    "value": {
                        "kind": "Secret",
                        "name": "router-tls-z1234567",
                        "namespace": "environment"
                    }
                },
                {
                    "op": "add",
                    "path": "/metadata/annotations",
                    "value": {
                        "qovery.com/gateway-fallback-router-tls-z1234567": "environment"
                    }
                }
            ])
        );
    }

    #[test]
    fn certificate_ref_cleanup_removes_ownership_when_the_reference_is_already_absent() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.metadata.annotations = Some(std::collections::BTreeMap::from([(
            "qovery.com/gateway-fallback-router-tls-z1234567".to_string(),
            "environment".to_string(),
        )]));
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "tls": { "certificateRefs": [] }
                }]
            }
        });

        let patch = gateway_certificate_ref_remove_patch(&gateway, "https", "environment", "router-tls-z1234567")
            .expect("patch creation should succeed")
            .expect("the ownership marker should produce a patch");

        assert_eq!(
            serde_json::to_value(&patch).expect("JSON Patch should serialize"),
            json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": "42" },
                {
                    "op": "remove",
                    "path": "/metadata/annotations/qovery.com~1gateway-fallback-router-tls-z1234567"
                }
            ])
        );
    }
}
