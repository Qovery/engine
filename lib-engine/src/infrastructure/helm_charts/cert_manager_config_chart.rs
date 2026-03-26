use crate::cmd::kubectl::kubectl_check_gateway_api_crds_available;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInfoUpgradeRetry, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError,
    HelmChartNamespaces,
};
use crate::infrastructure::action::gateway_api::GatewayApiRolloutStatus;
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
use crate::runtime::block_on;
use crate::services::kube_client::{CertManagerCertificate, CertManagerListenerSet};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;
use kube::api::{ListParams, Patch, PatchParams};
use kube::{Api, Client, ResourceExt};
use tracing::{info, warn};

/// Label key set by Qovery on every router-related Kubernetes resource.
const QOVERY_SERVICE_TYPE_LABEL: &str = "qovery.com/service-type";
const QOVERY_ROUTER_LABEL_VALUE: &str = "router";
/// IngressClass used exclusively by the Qovery-managed nginx controller.
const NGINX_QOVERY_INGRESS_CLASS: &str = "nginx-qovery";

pub struct CertManagerConfigsChart<'a> {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    lets_encrypt_config: &'a LetsEncryptConfig,
    dns_provider_configuration: &'a DnsProviderConfiguration,
    managed_dns: Vec<String>,
    namespace: HelmChartNamespaces,
    k8s_deploy_api_gateway: bool,
    k8s_use_api_gateway: bool,
    k8s_remove_nginx: bool,
}

impl<'a> CertManagerConfigsChart<'a> {
    pub fn new(
        chart_prefix_path: Option<&str>,
        lets_encrypt_config: &'a LetsEncryptConfig,
        dns_provider_configuration: &'a DnsProviderConfiguration,
        managed_dns_helm_format: Vec<String>,
        namespace: HelmChartNamespaces,
        k8s_deploy_api_gateway: bool,
        k8s_use_api_gateway: bool,
        k8s_remove_nginx: bool,
    ) -> Self {
        CertManagerConfigsChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                CertManagerConfigsChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                CertManagerConfigsChart::chart_name(),
            ),
            lets_encrypt_config,
            dns_provider_configuration,
            managed_dns: managed_dns_helm_format,
            namespace,
            k8s_deploy_api_gateway,
            k8s_use_api_gateway,
            k8s_remove_nginx,
        }
    }

    pub fn chart_name() -> String {
        "cert-manager-configs".to_string()
    }
}

impl ToCommonHelmChart for CertManagerConfigsChart<'_> {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let values = vec![
            ChartSetValue {
                key: "namespace".to_string(),
                value: self.namespace.to_string(),
            },
            ChartSetValue {
                key: "externalDnsProvider".to_string(),
                value: self.dns_provider_configuration.get_cert_manager_config_name(),
            },
            ChartSetValue {
                key: "acme.letsEncrypt.emailReport".to_string(),
                value: self.lets_encrypt_config.email_report().to_string(),
            },
            ChartSetValue {
                key: "acme.letsEncrypt.acmeUrl".to_string(),
                value: self.lets_encrypt_config.acme_url().to_string(),
            },
            ChartSetValue {
                key: "managedDns".to_string(),
                value: format!("{{{}}}", self.managed_dns.join(",")),
            },
            // Providers
            // Cloudflare
            ChartSetValue {
                key: "provider.cloudflare.apiToken".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::Cloudflare(cloudflare_config) => {
                        cloudflare_config.cloudflare_api_token.to_string()
                    }
                    DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                    DnsProviderConfiguration::Route53(_) => "not-set".to_string(),
                },
            },
            ChartSetValue {
                key: "provider.cloudflare.email".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::Cloudflare(cloudflare_config) => {
                        cloudflare_config.cloudflare_email.to_string()
                    }
                    DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                    DnsProviderConfiguration::Route53(_) => "not-set".to_string(),
                },
            },
            // Qovery DNS
            ChartSetValue {
                key: "provider.pdns.apiPort".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::QoveryDns(qovery_dns_config) => {
                        // TODO(benjaminch): Hack to be fixed: I don't want to use `values_string` field from `ChartInfo`
                        // as it's also kind of a hack.
                        // Good solution will be to merge `values` and `values_string` fields into one and having `ChartSetValue`
                        // to carry type as variant making a cleaner API to be used, way less confusing and ... testable \o/ !
                        //
                        // Ticket: ENG-1404
                        //
                        // pub enum ChartSetValue {
                        //     String(String),
                        //     Integer(i64),
                        //     Boolean(bool),
                        //     Array(Vec<ChartSetValue>),
                        // }
                        //
                        // #[derive(Clone)]
                        // pub struct ChartSetValue {
                        //     pub key: String,
                        //     pub value: ChartSetValue,
                        // }
                        format!("\"{}\"", qovery_dns_config.api_url_port)
                    }
                    DnsProviderConfiguration::Cloudflare(_) => "no-set".to_string(),
                    DnsProviderConfiguration::Route53(_) => "no-set".to_string(),
                },
            },
            ChartSetValue {
                key: "provider.pdns.apiUrl".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::QoveryDns(qovery_dns_config) => {
                        qovery_dns_config.api_url_scheme_and_domain.to_string()
                    }
                    DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                    DnsProviderConfiguration::Route53(_) => "not-set".to_string(),
                },
            },
            ChartSetValue {
                key: "provider.pdns.apiKey".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::QoveryDns(qovery_dns_config) => qovery_dns_config.api_key.to_string(),
                    DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                    DnsProviderConfiguration::Route53(_) => "not-set".to_string(),
                },
            },
            // Route 53
            ChartSetValue {
                key: "provider.route53.accessKeyId".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::Route53(route53_config) => route53_config.aws_access_key_id.to_string(),
                    DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                    DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                },
            },
            ChartSetValue {
                key: "provider.route53.secretAccessKey".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::Route53(route53_config) => {
                        route53_config.aws_secret_access_key.to_string()
                    }
                    DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                    DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                },
            },
            ChartSetValue {
                key: "provider.route53.region".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::Route53(route53_config) => route53_config.aws_region.to_string(),
                    DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                    DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                },
            },
            ChartSetValue {
                key: "provider.route53.hostedZoneId".to_string(),
                value: match &self.dns_provider_configuration {
                    DnsProviderConfiguration::Route53(route53_config) => {
                        route53_config.hosted_zone_id.clone().unwrap_or_else(|| "".to_string())
                    }
                    DnsProviderConfiguration::Cloudflare(_) => "not-set".to_string(),
                    DnsProviderConfiguration::QoveryDns(_) => "not-set".to_string(),
                },
            },
            ChartSetValue {
                key: "k8sDeployApiGateway".to_string(),
                value: self.k8s_deploy_api_gateway.to_string(),
            },
            ChartSetValue {
                key: "k8sUseApiGateway".to_string(),
                value: self.k8s_use_api_gateway.to_string(),
            },
            ChartSetValue {
                key: "k8sRemoveNginx".to_string(),
                value: self.k8s_remove_nginx.to_string(),
            },
        ];

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: CertManagerConfigsChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.namespace.clone(),
                // TODO: fix backup apply, it makes the chart deployment failed randomly
                // backup_resources: Some(vec!["cert".to_string(), "issuer".to_string(), "clusterissuer".to_string()]),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                upgrade_retry: Some(ChartInfoUpgradeRetry {
                    nb_retry: 10,
                    delay_in_milli_sec: 30_000,
                }),
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(CertManagerConfigsChartChecker::new(
                GatewayApiRolloutStatus::new(self.k8s_deploy_api_gateway, self.k8s_use_api_gateway),
            ))),
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

/// The desired owner kind for cert-manager `Certificate` resources.
/// Drives which direction the ownership migration runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateOwnerTarget {
    /// Gateway API is active: certificates should be owned by `ListenerSet`.
    ListenerSet,
    /// Gateway API is disabled: certificates should be owned by `Ingress`.
    Ingress,
}

#[derive(Clone)]
pub struct CertManagerConfigsChartChecker {
    gateway_api_rollout_status: GatewayApiRolloutStatus,
}

impl CertManagerConfigsChartChecker {
    pub fn new(gateway_api_rollout_status: GatewayApiRolloutStatus) -> CertManagerConfigsChartChecker {
        CertManagerConfigsChartChecker {
            gateway_api_rollout_status,
        }
    }
}

impl Default for CertManagerConfigsChartChecker {
    fn default() -> Self {
        CertManagerConfigsChartChecker::new(GatewayApiRolloutStatus::NotDeployed)
    }
}

impl ChartInstallationChecker for CertManagerConfigsChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let target = match self.gateway_api_rollout_status {
            GatewayApiRolloutStatus::Default => CertificateOwnerTarget::ListenerSet,
            GatewayApiRolloutStatus::DualStack => CertificateOwnerTarget::Ingress,
            GatewayApiRolloutStatus::NotDeployed => CertificateOwnerTarget::Ingress,
        };

        if target == CertificateOwnerTarget::ListenerSet && !kubectl_check_gateway_api_crds_available(kube_client) {
            warn!(
                "Skipping cert-manager certificate owner migration to ListenerSet: Gateway API CRDs are not available"
            );
            return Ok(());
        }

        migrate_certificate_owners(kube_client, target)
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

/// Returns `true` when `ingress` is managed by the Qovery nginx controller:
/// `spec.ingressClassName == "nginx-qovery"` and label `qovery.com/service-type: router`.
pub fn is_qovery_router_ingress(ingress: &k8s_openapi::api::networking::v1::Ingress) -> bool {
    let has_nginx_qovery_class =
        ingress.spec.as_ref().and_then(|s| s.ingress_class_name.as_deref()) == Some(NGINX_QOVERY_INGRESS_CLASS);

    let has_router_label = ingress
        .metadata
        .labels
        .as_ref()
        .and_then(|l| l.get(QOVERY_SERVICE_TYPE_LABEL))
        .map(|v| v.as_str())
        == Some(QOVERY_ROUTER_LABEL_VALUE);

    has_nginx_qovery_class && has_router_label
}

/// Returns `true` when `ls` is a Qovery router `ListenerSet` (label `qovery.com/service-type: router`).
pub fn is_qovery_router_listenerset(ls: &CertManagerListenerSet) -> bool {
    ls.metadata
        .labels
        .as_ref()
        .and_then(|l| l.get(QOVERY_SERVICE_TYPE_LABEL))
        .map(|v| v.as_str())
        == Some(QOVERY_ROUTER_LABEL_VALUE)
}

/// Migrates `ownerReferences` on cert-manager `Certificate` resources so the correct
/// controller owns each cert after an nginx ↔ Gateway API switch. Idempotent.
///
/// TODO: remove once Gateway API is fully rolled out and nginx ingress support is dropped.
pub fn migrate_certificate_owners(kube_client: &Client, target: CertificateOwnerTarget) -> Result<(), CommandError> {
    block_on(migrate_certificate_owners_async(kube_client, target))
}

async fn migrate_certificate_owners_async(
    kube_client: &Client,
    target: CertificateOwnerTarget,
) -> Result<(), CommandError> {
    let (current_owner_kind, new_owner_kind) = match target {
        CertificateOwnerTarget::ListenerSet => ("Ingress", "ListenerSet"),
        CertificateOwnerTarget::Ingress => ("ListenerSet", "Ingress"),
    };

    info!(
        "cert-manager certificate owner migration: {} -> {}",
        current_owner_kind, new_owner_kind
    );

    // List all Certificates cluster-wide
    let certs_api: Api<CertManagerCertificate> = Api::all(kube_client.clone());
    let all_certs = certs_api
        .list(&ListParams::default())
        .await
        .map_err(|e| CommandError::new_from_safe_message(format!("Failed to list cert-manager Certificates: {e}")))?;

    for cert in &all_certs.items {
        let cert_name = cert.name_any();
        let cert_namespace = match cert.namespace() {
            Some(ns) => ns.to_string(),
            None => {
                warn!("Certificate {cert_name} has no namespace, skipping");
                continue;
            }
        };

        let owner_refs = cert.metadata.owner_references.as_deref().unwrap_or(&[]);

        // Validate the candidate target-kind owner against the live object before treating migration as complete.
        // A stale ownerReference (name/uid from a deleted-and-recreated resource) must not short-circuit the repair; we must re-migrate in that case.
        let mut needs_target_owner_repair = false;
        if let Some(existing_target_ref) = owner_refs.iter().find(|o| o.kind == new_owner_kind) {
            let live_uid_matches = match target {
                CertificateOwnerTarget::ListenerSet => {
                    let ls_api: Api<CertManagerListenerSet> = Api::namespaced(kube_client.clone(), &cert_namespace);
                    match ls_api.get_opt(&existing_target_ref.name).await.map_err(|e| {
                        CommandError::new_from_safe_message(format!(
                            "Failed to fetch ListenerSet {cert_namespace}/{}: {e}",
                            existing_target_ref.name
                        ))
                    })? {
                        Some(ls) => ls.metadata.uid.as_deref() == Some(existing_target_ref.uid.as_str()),
                        None => false,
                    }
                }
                CertificateOwnerTarget::Ingress => {
                    use k8s_openapi::api::networking::v1::Ingress;
                    let ingress_api: Api<Ingress> = Api::namespaced(kube_client.clone(), &cert_namespace);
                    match ingress_api.get_opt(&existing_target_ref.name).await.map_err(|e| {
                        CommandError::new_from_safe_message(format!(
                            "Failed to fetch Ingress {cert_namespace}/{}: {e}",
                            existing_target_ref.name
                        ))
                    })? {
                        Some(ingress) => ingress.metadata.uid.as_deref() == Some(existing_target_ref.uid.as_str()),
                        None => false,
                    }
                }
            };

            if live_uid_matches {
                info!(
                    "Certificate {cert_namespace}/{cert_name} already owned by live {new_owner_kind}/{}, skipping",
                    existing_target_ref.name
                );
                continue;
            }

            // UID mismatch: the referenced object was recreated. Fall through to re-migrate.
            warn!(
                "Certificate {cert_namespace}/{cert_name}: ownerReference {new_owner_kind}/{} has stale UID ({}), will re-migrate",
                existing_target_ref.name, existing_target_ref.uid
            );
            needs_target_owner_repair = true;
        }

        let current_owner = owner_refs.iter().find(|o| o.kind == current_owner_kind);

        if let Some(current_owner) = current_owner {
            // Only migrate certs whose current owner is a Qovery-managed resource.
            let owned_by_qovery = match target {
                CertificateOwnerTarget::ListenerSet => {
                    use k8s_openapi::api::networking::v1::Ingress;
                    let ingress_api: Api<Ingress> = Api::namespaced(kube_client.clone(), &cert_namespace);
                    match ingress_api.get_opt(&current_owner.name).await.map_err(|e| {
                        CommandError::new_from_safe_message(format!(
                            "Failed to fetch Ingress {cert_namespace}/{}: {e}",
                            current_owner.name
                        ))
                    })? {
                        Some(ingress) => is_qovery_router_ingress(&ingress),
                        None => false,
                    }
                }
                CertificateOwnerTarget::Ingress => {
                    let ls_api: Api<CertManagerListenerSet> = Api::namespaced(kube_client.clone(), &cert_namespace);
                    match ls_api.get_opt(&current_owner.name).await.map_err(|e| {
                        CommandError::new_from_safe_message(format!(
                            "Failed to fetch ListenerSet {cert_namespace}/{}: {e}",
                            current_owner.name
                        ))
                    })? {
                        Some(ls) => is_qovery_router_listenerset(&ls),
                        None => false,
                    }
                }
            };

            if !owned_by_qovery {
                info!(
                    "Certificate {cert_namespace}/{cert_name}: current owner {}/{} is not a Qovery-managed resource, skipping",
                    current_owner_kind, current_owner.name
                );
                continue;
            }

            info!(
                "Certificate {cert_namespace}/{cert_name}: migrating owner from {}/{} to {new_owner_kind}",
                current_owner_kind, current_owner.name
            );
        } else if needs_target_owner_repair {
            info!(
                "Certificate {cert_namespace}/{cert_name}: stale {new_owner_kind} owner detected with no {current_owner_kind} owner, forcing owner repair"
            );
        } else {
            continue;
        }

        let new_owner_ref = match target {
            CertificateOwnerTarget::ListenerSet => {
                find_listenerset_owning_secret(kube_client, &cert_namespace, &cert.spec.secret_name).await?
            }
            CertificateOwnerTarget::Ingress => {
                find_ingress_owning_secret(kube_client, &cert_namespace, &cert.spec.secret_name).await?
            }
        };

        let new_owner_ref = match new_owner_ref {
            Some(r) => r,
            None => {
                warn!(
                    "Certificate {cert_namespace}/{cert_name}: no matching {new_owner_kind} found for secret '{}', skipping",
                    cert.spec.secret_name
                );
                continue;
            }
        };

        // Drop old and any stale new-kind refs, then add the fresh one.
        let mut new_refs: Vec<OwnerReference> = owner_refs
            .iter()
            .filter(|o| o.kind != current_owner_kind && o.kind != new_owner_kind)
            .cloned()
            .collect();
        new_refs.push(new_owner_ref);

        let patch = serde_json::json!({
            "metadata": {
                "ownerReferences": new_refs
            }
        });

        let certs_ns_api: Api<CertManagerCertificate> = Api::namespaced(kube_client.clone(), &cert_namespace);
        certs_ns_api
            .patch(&cert_name, &PatchParams::default(), &Patch::Merge(&patch))
            .await
            .map_err(|e| {
                CommandError::new_from_safe_message(format!(
                    "Failed to patch ownerReferences on Certificate {cert_namespace}/{cert_name}: {e}"
                ))
            })?;

        info!(
            "Certificate {cert_namespace}/{cert_name}: owner successfully migrated to {new_owner_kind}/{}",
            new_refs.last().map(|r| r.name.as_str()).unwrap_or("?")
        );
    }

    Ok(())
}

/// Returns an `OwnerReference` for the Qovery router `ListenerSet` in `namespace`
/// whose `spec.listeners[].tls.certificateRefs[].name` matches `secret_name`.
async fn find_listenerset_owning_secret(
    kube_client: &Client,
    namespace: &str,
    secret_name: &str,
) -> Result<Option<OwnerReference>, CommandError> {
    let api: Api<CertManagerListenerSet> = Api::namespaced(kube_client.clone(), namespace);
    let list = api.list(&ListParams::default()).await.map_err(|e| {
        CommandError::new_from_safe_message(format!("Failed to list ListenerSets in namespace {namespace}: {e}"))
    })?;

    for ls in &list.items {
        if !is_qovery_router_listenerset(ls) {
            continue;
        }

        let references_secret = ls.spec.listeners.iter().any(|l| {
            l.tls
                .as_ref()
                .map(|tls| tls.certificate_refs.iter().any(|r| r.name == secret_name))
                .unwrap_or(false)
        });

        if references_secret && let (Some(name), Some(uid)) = (ls.metadata.name.as_deref(), ls.metadata.uid.as_deref())
        {
            return Ok(Some(OwnerReference {
                api_version: "gateway.networking.k8s.io/v1".to_string(),
                kind: "ListenerSet".to_string(),
                name: name.to_string(),
                uid: uid.to_string(),
                block_owner_deletion: Some(true),
                controller: Some(true),
            }));
        }
    }

    Ok(None)
}

/// Returns an `OwnerReference` for the Qovery nginx `Ingress` in `namespace`
/// whose `spec.tls[].secretName` matches `secret_name`.
async fn find_ingress_owning_secret(
    kube_client: &Client,
    namespace: &str,
    secret_name: &str,
) -> Result<Option<OwnerReference>, CommandError> {
    use k8s_openapi::api::networking::v1::Ingress;

    let api: Api<Ingress> = Api::namespaced(kube_client.clone(), namespace);
    let list = api.list(&ListParams::default()).await.map_err(|e| {
        CommandError::new_from_safe_message(format!("Failed to list Ingresses in namespace {namespace}: {e}"))
    })?;

    for ingress in &list.items {
        if !is_qovery_router_ingress(ingress) {
            continue;
        }

        let references_secret = ingress
            .spec
            .as_ref()
            .and_then(|s| s.tls.as_deref())
            .map(|tls_entries| {
                tls_entries
                    .iter()
                    .any(|t| t.secret_name.as_deref() == Some(secret_name))
            })
            .unwrap_or(false);

        if references_secret
            && let (Some(name), Some(uid)) = (ingress.metadata.name.as_deref(), ingress.metadata.uid.as_deref())
        {
            return Ok(Some(OwnerReference {
                api_version: "networking.k8s.io/v1".to_string(),
                kind: "Ingress".to_string(),
                name: name.to_string(),
                uid: uid.to_string(),
                block_owner_deletion: Some(true),
                controller: Some(true),
            }));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::action::gateway_api::GatewayApiRolloutStatus;
    use crate::infrastructure::helm_charts::cert_manager_config_chart::{
        CertManagerConfigsChart, CertManagerConfigsChartChecker, CertificateOwnerTarget, LetsEncryptConfig,
    };
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;
    use crate::infrastructure::models::dns_provider::qoverydns::QoveryDnsConfig;
    use crate::services::kube_client::{CertManagerListenerSet, ListenerSetSpec};
    use k8s_openapi::api::networking::v1::{Ingress, IngressSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
    use std::env;
    use url::Url;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn cert_manager_configs_chart_directory_exists_test() {
        let lets_encrypt_config = LetsEncryptConfig::new("whatever".to_string(), true);
        let dns_provider_config = DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_key: "whatever".to_string(),
            api_url: Url::parse("https://whatever.com").expect("Unable to parse URL"),
            api_url_port: "whatever".to_string(),
            api_url_scheme_and_domain: "whatever".to_string(),
        });
        let chart = CertManagerConfigsChart::new(
            None,
            &lets_encrypt_config,
            &dns_provider_config,
            vec!["whatever".to_string()],
            HelmChartNamespaces::CertManager,
            false,
            false,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            CertManagerConfigsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn cert_manager_configs_chart_values_file_exists_test() {
        // setup:
        let lets_encrypt_config = LetsEncryptConfig::new("whatever".to_string(), true);
        let dns_provider_config = DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_key: "whatever".to_string(),
            api_url: Url::parse("https://whatever.com").expect("Unable to parse URL"),
            api_url_port: "whatever".to_string(),
            api_url_scheme_and_domain: "whatever".to_string(),
        });
        let chart = CertManagerConfigsChart::new(
            None,
            &lets_encrypt_config,
            &dns_provider_config,
            vec!["whatever".to_string()],
            HelmChartNamespaces::CertManager,
            false,
            false,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            CertManagerConfigsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn cert_manager_configs_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let lets_encrypt_config = LetsEncryptConfig::new("whatever".to_string(), true);
        let dns_provider_config = DnsProviderConfiguration::QoveryDns(QoveryDnsConfig {
            api_key: "whatever".to_string(),
            api_url: Url::parse("https://whatever.com").expect("Unable to parse URL"),
            api_url_port: "whatever".to_string(),
            api_url_scheme_and_domain: "whatever".to_string(),
        });
        let chart = CertManagerConfigsChart::new(
            None,
            &lets_encrypt_config,
            &dns_provider_config,
            vec!["whatever".to_string()],
            HelmChartNamespaces::CertManager,
            false,
            false,
            false,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                CertManagerConfigsChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }

    #[test]
    fn checker_target_is_listenerset_when_gateway_api_is_default() {
        let checker = CertManagerConfigsChartChecker::new(GatewayApiRolloutStatus::Default);
        assert_eq!(checker.gateway_api_rollout_status, GatewayApiRolloutStatus::Default);
    }

    #[test]
    fn checker_target_is_listenerset_when_gateway_api_is_dual_stack() {
        let checker = CertManagerConfigsChartChecker::new(GatewayApiRolloutStatus::DualStack);
        assert_eq!(checker.gateway_api_rollout_status, GatewayApiRolloutStatus::DualStack);
    }

    #[test]
    fn checker_target_is_ingress_when_gateway_api_is_not_deployed() {
        let checker = CertManagerConfigsChartChecker::new(GatewayApiRolloutStatus::NotDeployed);
        assert_eq!(checker.gateway_api_rollout_status, GatewayApiRolloutStatus::NotDeployed);
    }

    #[test]
    fn checker_default_targets_ingress() {
        let checker = CertManagerConfigsChartChecker::default();
        assert_eq!(checker.gateway_api_rollout_status, GatewayApiRolloutStatus::NotDeployed);
    }

    #[test]
    fn certificate_owner_target_variants_are_distinct() {
        assert_ne!(CertificateOwnerTarget::ListenerSet, CertificateOwnerTarget::Ingress);
    }

    fn compute_new_owner_refs(
        existing: &[OwnerReference],
        current_owner_kind: &str,
        new_owner_ref: OwnerReference,
    ) -> Vec<OwnerReference> {
        let new_owner_kind = &new_owner_ref.kind;
        let mut new_refs: Vec<OwnerReference> = existing
            .iter()
            .filter(|o| o.kind != current_owner_kind && o.kind.as_str() != new_owner_kind.as_str())
            .cloned()
            .collect();
        new_refs.push(new_owner_ref);
        new_refs
    }

    #[test]
    fn replaces_ingress_owner_with_listenerset() {
        let existing = vec![OwnerReference {
            api_version: "networking.k8s.io/v1".to_string(),
            kind: "Ingress".to_string(),
            name: "my-ingress".to_string(),
            uid: "ingress-uid".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }];
        let new_ref = OwnerReference {
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            kind: "ListenerSet".to_string(),
            name: "my-listenerset".to_string(),
            uid: "ls-uid".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        };
        let result = compute_new_owner_refs(&existing, "Ingress", new_ref);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, "ListenerSet");
        assert_eq!(result[0].name, "my-listenerset");
        assert_eq!(result[0].uid, "ls-uid");
    }

    #[test]
    fn replaces_listenerset_owner_with_ingress() {
        let existing = vec![OwnerReference {
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            kind: "ListenerSet".to_string(),
            name: "my-listenerset".to_string(),
            uid: "ls-uid".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        }];
        let new_ref = OwnerReference {
            api_version: "networking.k8s.io/v1".to_string(),
            kind: "Ingress".to_string(),
            name: "my-ingress".to_string(),
            uid: "ingress-uid".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        };
        let result = compute_new_owner_refs(&existing, "ListenerSet", new_ref);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, "Ingress");
        assert_eq!(result[0].name, "my-ingress");
    }

    #[test]
    fn preserves_unrelated_owner_refs_during_migration() {
        let existing = vec![
            OwnerReference {
                api_version: "networking.k8s.io/v1".to_string(),
                kind: "Ingress".to_string(),
                name: "my-ingress".to_string(),
                uid: "ingress-uid".to_string(),
                controller: Some(true),
                block_owner_deletion: Some(true),
            },
            OwnerReference {
                api_version: "v1".to_string(),
                kind: "SomeOtherKind".to_string(),
                name: "other".to_string(),
                uid: "other-uid".to_string(),
                controller: Some(false),
                block_owner_deletion: Some(false),
            },
        ];
        let new_ref = OwnerReference {
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            kind: "ListenerSet".to_string(),
            name: "my-listenerset".to_string(),
            uid: "ls-uid".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        };
        let result = compute_new_owner_refs(&existing, "Ingress", new_ref);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|r| r.kind == "SomeOtherKind"));
        assert!(result.iter().any(|r| r.kind == "ListenerSet"));
    }

    #[test]
    fn does_not_duplicate_new_owner_kind_if_already_present() {
        // If both old and new kind exist (shouldn't happen normally), only new kind survives.
        let existing = vec![
            OwnerReference {
                api_version: "networking.k8s.io/v1".to_string(),
                kind: "Ingress".to_string(),
                name: "old-ingress".to_string(),
                uid: "old-uid".to_string(),
                controller: Some(true),
                block_owner_deletion: Some(true),
            },
            OwnerReference {
                api_version: "gateway.networking.k8s.io/v1".to_string(),
                kind: "ListenerSet".to_string(),
                name: "stale-ls".to_string(),
                uid: "stale-uid".to_string(),
                controller: Some(true),
                block_owner_deletion: Some(true),
            },
        ];
        let new_ref = OwnerReference {
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            kind: "ListenerSet".to_string(),
            name: "new-ls".to_string(),
            uid: "new-ls-uid".to_string(),
            controller: Some(true),
            block_owner_deletion: Some(true),
        };
        let result = compute_new_owner_refs(&existing, "Ingress", new_ref);
        // Ingress removed, stale ListenerSet removed, new ListenerSet added → 1
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "new-ls");
    }

    fn make_ingress(ingress_class: Option<&str>, service_type_label: Option<&str>) -> Ingress {
        use std::collections::BTreeMap;
        let mut labels = BTreeMap::new();
        if let Some(v) = service_type_label {
            labels.insert("qovery.com/service-type".to_string(), v.to_string());
        }
        Ingress {
            metadata: ObjectMeta {
                name: Some("my-ingress".to_string()),
                uid: Some("ingress-uid".to_string()),
                labels: if labels.is_empty() { None } else { Some(labels) },
                ..Default::default()
            },
            spec: Some(IngressSpec {
                ingress_class_name: ingress_class.map(|s| s.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn qovery_router_ingress_passes_when_both_conditions_met() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_ingress;
        let ingress = make_ingress(Some("nginx-qovery"), Some("router"));
        assert!(is_qovery_router_ingress(&ingress));
    }

    #[test]
    fn qovery_router_ingress_fails_without_label() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_ingress;
        let ingress = make_ingress(Some("nginx-qovery"), None);
        assert!(!is_qovery_router_ingress(&ingress));
    }

    #[test]
    fn qovery_router_ingress_fails_with_wrong_label_value() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_ingress;
        let ingress = make_ingress(Some("nginx-qovery"), Some("application"));
        assert!(!is_qovery_router_ingress(&ingress));
    }

    #[test]
    fn qovery_router_ingress_fails_without_ingress_class() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_ingress;
        let ingress = make_ingress(None, Some("router"));
        assert!(!is_qovery_router_ingress(&ingress));
    }

    #[test]
    fn qovery_router_ingress_fails_with_wrong_ingress_class() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_ingress;
        let ingress = make_ingress(Some("nginx"), Some("router"));
        assert!(!is_qovery_router_ingress(&ingress));
    }

    fn make_listenerset(service_type_label: Option<&str>) -> CertManagerListenerSet {
        use std::collections::BTreeMap;
        let mut labels = BTreeMap::new();
        if let Some(v) = service_type_label {
            labels.insert("qovery.com/service-type".to_string(), v.to_string());
        }
        CertManagerListenerSet {
            metadata: ObjectMeta {
                name: Some("my-listenerset".to_string()),
                uid: Some("ls-uid".to_string()),
                labels: if labels.is_empty() { None } else { Some(labels) },
                ..Default::default()
            },
            spec: ListenerSetSpec { listeners: vec![] },
        }
    }

    #[test]
    fn qovery_router_listenerset_passes_with_router_label() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_listenerset;
        let ls = make_listenerset(Some("router"));
        assert!(is_qovery_router_listenerset(&ls));
    }

    #[test]
    fn qovery_router_listenerset_fails_without_label() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_listenerset;
        let ls = make_listenerset(None);
        assert!(!is_qovery_router_listenerset(&ls));
    }

    #[test]
    fn qovery_router_listenerset_fails_with_wrong_label_value() {
        use crate::infrastructure::helm_charts::cert_manager_config_chart::is_qovery_router_listenerset;
        let ls = make_listenerset(Some("application"));
        assert!(!is_qovery_router_listenerset(&ls));
    }
}
