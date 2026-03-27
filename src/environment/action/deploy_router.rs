use crate::environment::action::DeploymentAction;
use crate::environment::action::check_dns::CheckDnsForDomains;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::models::router::Router;
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::router::reporter::RouterDeploymentReporter;
use crate::environment::report::{DeploymentTaskRef, execute_long_deployment};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::infrastructure::models::kubernetes::Kind;
use crate::io_models::models::CustomDomain;
use crate::runtime::block_on;
use crate::{
    cmd::kubectl::{
        kubectl_check_gateway_api_crds_available, kubectl_gateway_crd_supports_allowed_listeners,
        kubectl_get_gateway_api_served_version, kubectl_get_reference_grant_served_version,
    },
    errors::CommandError,
};

use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use kube::api::{Api, ApiResource, DynamicObject, Patch, PatchParams};
use kube::core::GroupVersionKind;
use serde_json::json;
use std::path::PathBuf;

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
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

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
                    PathBuf::from(self.helm_chart_dir().as_str()),
                    None,
                    chart,
                );

                let result = helm.on_delete(target);

                if result.is_ok()
                    && let Err(e) = maybe_remove_gateway_cert_refs_for_custom_domains(self, target, logger)
                {
                    logger.warning(format!("Failed to remove Gateway certificateRefs for custom domain TLS: {e}"));
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

fn maybe_patch_gateway_cert_refs_for_custom_domains<T: CloudProvider>(
    router: &Router<T>,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
) -> Result<(), Box<EngineError>> {
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

    // ListenerSets are deployed for backward compatibility, but on GKE they can be ineffective.
    // Apply Gateway certRef fallback for custom domain TLS.
    if !matches!(target.kubernetes.kind(), Kind::Gke) {
        return Ok(());
    }

    let has_custom_certs = router.custom_domains.iter().any(|d| d.generate_certificate);
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

    // ReferenceGrant required so the Gateway (qovery ns) can read the TLS secret cross-namespace.
    if let Err(e) =
        ensure_gateway_to_secret_reference_grant(target.kube.client(), "qovery", &secret_namespace, &secret_name)
    {
        logger.warning(format!(
            "Failed to ensure ReferenceGrant for Gateway TLS secret {secret_namespace}/{secret_name}: {e}"
        ));
        return Err(Box::new(EngineError::new_router_failed_to_deploy(
            router.get_event_details(Stage::Environment(EnvironmentStep::Deploy)),
        )));
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

/// Ensures a ReferenceGrant in `secret_namespace` allowing the Gateway in `gateway_namespace`
/// to reference the TLS Secret. Idempotent (server-side apply).
fn ensure_gateway_to_secret_reference_grant(
    kube_client: kube::Client,
    gateway_namespace: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<(), CommandError> {
    let api_version = kubectl_get_reference_grant_served_version(&kube_client).unwrap_or_else(|| "v1beta1".to_string());
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &api_version, "ReferenceGrant");
    let api: Api<DynamicObject> = Api::namespaced_with(kube_client, secret_namespace, &ApiResource::from_gvk(&gvk));

    let grant_name = format!("allow-gateway-to-{secret_name}");
    let grant = json!({
        "apiVersion": format!("gateway.networking.k8s.io/{api_version}"),
        "kind": "ReferenceGrant",
        "metadata": { "name": grant_name, "namespace": secret_namespace },
        "spec": {
            "from": [{
                "group": "gateway.networking.k8s.io",
                "kind": "Gateway",
                "namespace": gateway_namespace
            }],
            "to": [{
                "group": "",
                "kind": "Secret",
                "name": secret_name
            }]
        }
    });

    block_on(api.patch(&grant_name, &PatchParams::apply("qovery-engine"), &Patch::Apply(&grant))).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Failed to apply ReferenceGrant {secret_namespace}/{grant_name}: {e}"
        ))
    })?;

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
    let api: Api<DynamicObject> = Api::namespaced_with(kube_client, gateway_namespace, &ApiResource::from_gvk(&gvk));

    let mut gateway = block_on(api.get(gateway_name)).map_err(|e| {
        CommandError::new_from_safe_message(format!("Failed to fetch Gateway {gateway_namespace}/{gateway_name}: {e}"))
    })?;

    let listeners = gateway
        .data
        .get_mut("spec")
        .and_then(|spec| spec.get_mut("listeners"))
        .and_then(|listeners| listeners.as_array_mut())
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} has no spec.listeners"
            ))
        })?;

    let listener = listeners
        .iter_mut()
        .find(|l| l.get("name").and_then(|v| v.as_str()) == Some(listener_name))
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} has no '{listener_name}' listener"
            ))
        })?;

    let tls = listener
        .as_object_mut()
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} listener '{listener_name}' is not an object"
            ))
        })?
        .entry("tls")
        .or_insert_with(|| json!({}));

    let cert_refs = tls
        .as_object_mut()
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} listener '{listener_name}' tls is not an object"
            ))
        })?
        .entry("certificateRefs")
        .or_insert_with(|| json!([]));

    let cert_refs_array = cert_refs.as_array_mut().ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Gateway {gateway_namespace}/{gateway_name} listener '{listener_name}' tls.certificateRefs is not an array"
        ))
    })?;

    let already_present = cert_refs_array.iter().any(|r| {
        r.get("name").and_then(|v| v.as_str()) == Some(secret_name)
            && r.get("namespace").and_then(|v| v.as_str()) == Some(secret_namespace)
    });

    if already_present {
        return Ok(false);
    }

    cert_refs_array.push(json!({
        "group": "",
        "kind": "Secret",
        "name": secret_name,
        "namespace": secret_namespace
    }));

    let listeners_patch = gateway
        .data
        .get("spec")
        .and_then(|spec| spec.get("listeners"))
        .cloned()
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} has no spec.listeners after mutation"
            ))
        })?;

    let patch = json!({
        "spec": {
            "listeners": listeners_patch
        }
    });

    block_on(api.patch(gateway_name, &PatchParams::default(), &Patch::Merge(&patch))).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Failed to patch Gateway {gateway_namespace}/{gateway_name} certificateRefs: {e}"
        ))
    })?;

    Ok(true)
}

fn maybe_remove_gateway_cert_refs_for_custom_domains<T: CloudProvider>(
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

    let has_custom_certs = router.custom_domains.iter().any(|d| d.generate_certificate);
    if !has_custom_certs {
        return Ok(());
    }

    let secret_name = format!("router-tls-{}", router.id);
    let secret_namespace = target.environment.namespace().to_string();

    logger.info(format!(
        "Cleaning up Gateway TLS secret reference for {secret_namespace}/{secret_name}."
    ));

    let removed = remove_gateway_certificate_ref(
        target.kube.client(),
        "qovery",
        "qovery-cluster-public-gateway",
        "https",
        &secret_namespace,
        &secret_name,
    )?;

    if removed {
        logger.info("Gateway certificateRefs cleanup completed.".to_string());
    } else {
        logger.info("Gateway certificateRefs cleanup not needed.".to_string());
    }

    Ok(())
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
    let api: Api<DynamicObject> = Api::namespaced_with(kube_client, gateway_namespace, &ApiResource::from_gvk(&gvk));

    let mut gateway = block_on(api.get(gateway_name)).map_err(|e| {
        CommandError::new_from_safe_message(format!("Failed to fetch Gateway {gateway_namespace}/{gateway_name}: {e}"))
    })?;

    let listeners = gateway
        .data
        .get_mut("spec")
        .and_then(|spec| spec.get_mut("listeners"))
        .and_then(|listeners| listeners.as_array_mut())
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} has no spec.listeners"
            ))
        })?;

    let listener = listeners
        .iter_mut()
        .find(|l| l.get("name").and_then(|v| v.as_str()) == Some(listener_name))
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} has no '{listener_name}' listener"
            ))
        })?;

    let tls = match listener.get_mut("tls") {
        Some(tls) => tls,
        None => return Ok(false),
    };

    let cert_refs = match tls.get_mut("certificateRefs") {
        Some(cert_refs) => cert_refs,
        None => return Ok(false),
    };

    let cert_refs_array = cert_refs.as_array_mut().ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Gateway {gateway_namespace}/{gateway_name} listener '{listener_name}' tls.certificateRefs is not an array"
        ))
    })?;

    let original_len = cert_refs_array.len();
    cert_refs_array.retain(|r| {
        !(r.get("name").and_then(|v| v.as_str()) == Some(secret_name)
            && r.get("namespace").and_then(|v| v.as_str()) == Some(secret_namespace))
    });

    if cert_refs_array.len() == original_len {
        return Ok(false);
    }

    let listeners_patch = gateway
        .data
        .get("spec")
        .and_then(|spec| spec.get("listeners"))
        .cloned()
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!(
                "Gateway {gateway_namespace}/{gateway_name} has no spec.listeners after mutation"
            ))
        })?;

    let patch = json!({
        "spec": {
            "listeners": listeners_patch
        }
    });

    block_on(api.patch(gateway_name, &PatchParams::default(), &Patch::Merge(&patch))).map_err(|e| {
        CommandError::new_from_safe_message(format!(
            "Failed to patch Gateway {gateway_namespace}/{gateway_name} certificateRefs: {e}"
        ))
    })?;

    Ok(true)
}
