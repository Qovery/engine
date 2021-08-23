use crate::cloud_provider::digitalocean::kubernetes::DoksOptions;
use crate::cloud_provider::helm::{
    get_chart_namespace, ChartInfo, ChartSetValue, ChartValuesGenerated, CommonChart, CoreDNSConfigChart, HelmChart,
    HelmChartNamespaces,
};
use crate::cloud_provider::qovery::{get_qovery_app_version, QoveryAgent, QoveryAppName, QoveryEngine};
use crate::error::{SimpleError, SimpleErrorKind};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigitalOceanQoveryTerraformConfig {
    pub loki_storage_config_do_space: String,
}

pub struct ChartsConfigPrerequisites {
    pub organization_id: String,
    pub cluster_id: String,
    pub do_cluster_id: String,
    pub region: String,
    pub cluster_name: String,
    pub cloud_provider: String,
    pub test_cluster: bool,
    pub do_token: String,
    pub do_space_access_id: String,
    pub do_space_secret_key: String,
    pub do_space_bucket_kubeconfig: String,
    pub do_space_kubeconfig_filename: String,
    pub ff_log_history_enabled: bool,
    pub ff_metrics_history_enabled: bool,
    pub managed_dns_name: String,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub external_dns_provider: String,
    pub dns_email_report: String,
    pub acme_url: String,
    pub cloudflare_email: String,
    pub cloudflare_api_token: String,
    pub disable_pleco: bool,
    // qovery options form json input
    pub infra_options: DoksOptions,
}

impl ChartsConfigPrerequisites {
    pub fn new(
        organization_id: String,
        cluster_id: String,
        do_cluster_id: String,
        region: String,
        cluster_name: String,
        cloud_provider: String,
        test_cluster: bool,
        do_token: String,
        do_space_access_id: String,
        do_space_secret_key: String,
        do_space_bucket_kubeconfig: String,
        do_space_kubeconfig_filename: String,
        ff_log_history_enabled: bool,
        ff_metrics_history_enabled: bool,
        managed_dns_name: String,
        managed_dns_helm_format: String,
        managed_dns_resolvers_terraform_format: String,
        external_dns_provider: String,
        dns_email_report: String,
        acme_url: String,
        cloudflare_email: String,
        cloudflare_api_token: String,
        disable_pleco: bool,
        infra_options: DoksOptions,
    ) -> Self {
        ChartsConfigPrerequisites {
            organization_id,
            cluster_id,
            do_cluster_id,
            region,
            cluster_name,
            cloud_provider,
            test_cluster,
            do_token,
            do_space_access_id,
            do_space_secret_key,
            do_space_bucket_kubeconfig,
            do_space_kubeconfig_filename,
            ff_log_history_enabled,
            ff_metrics_history_enabled,
            managed_dns_name,
            managed_dns_helm_format,
            managed_dns_resolvers_terraform_format,
            external_dns_provider,
            dns_email_report,
            acme_url,
            cloudflare_email,
            cloudflare_api_token,
            disable_pleco,
            infra_options,
        }
    }
}

pub fn do_helm_charts(
    qovery_terraform_config_file: &str,
    chart_config_prerequisites: &ChartsConfigPrerequisites,
    chart_prefix_path: Option<&str>,
    _kubernetes_config: &Path,
    _envs: &[(String, String)],
) -> Result<Vec<Vec<Box<dyn HelmChart>>>, SimpleError> {
    info!("preparing chart configuration to be deployed");

    let content_file = match File::open(&qovery_terraform_config_file) {
        Ok(x) => x,
        Err(e) => return Err(SimpleError{ kind: SimpleErrorKind::Other, message: Some(
            format!("Can't deploy helm chart as Qovery terraform config file has not been rendered by Terraform. Are you running it in dry run mode?. {:?}", e)
        )}),
    };
    let chart_prefix = chart_prefix_path.unwrap_or("./");
    let chart_path = |x: &str| -> String { format!("{}/{}", &chart_prefix, x) };
    let reader = BufReader::new(content_file);
    let qovery_terraform_config: DigitalOceanQoveryTerraformConfig = match serde_json::from_reader(reader) {
        Ok(config) => config,
        Err(e) => {
            error!(
                "error while parsing terraform config file {}: {:?}",
                &qovery_terraform_config_file, &e
            );
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(format!("{:?}", e)),
            });
        }
    };

    let prometheus_namespace = HelmChartNamespaces::Prometheus;
    let prometheus_internal_url = format!(
        "http://prometheus-operated.{}.svc",
        get_chart_namespace(prometheus_namespace)
    );
    let loki_namespace = HelmChartNamespaces::Logging;
    let loki_kube_dns_prefix = format!("loki.{}.svc", get_chart_namespace(loki_namespace));

    // Qovery storage class
    let q_storage_class = CommonChart {
        chart_info: ChartInfo {
            name: "q-storageclass".to_string(),
            path: chart_path("/charts/q-storageclass"),
            ..Default::default()
        },
    };

    let coredns_config = CoreDNSConfigChart {
        chart_info: ChartInfo {
            name: "coredns".to_string(),
            path: chart_path("/charts/coredns-config"),
            values: vec![
                ChartSetValue {
                    key: "managed_dns".to_string(),
                    value: chart_config_prerequisites.managed_dns_helm_format.clone(),
                },
                ChartSetValue {
                    key: "managed_dns_resolvers".to_string(),
                    value: chart_config_prerequisites
                        .managed_dns_resolvers_terraform_format
                        .clone(),
                },
            ],
            ..Default::default()
        },
    };

    let external_dns = CommonChart {
        chart_info: ChartInfo {
            name: "externaldns".to_string(),
            path: chart_path("common/charts/external-dns"),
            values_files: vec![chart_path("chart_values/external-dns.yaml")],
            values: vec![
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "50m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "50m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "50Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "50Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let promtail = CommonChart {
        chart_info: ChartInfo {
            name: "promtail".to_string(),
            path: chart_path("common/charts/promtail"),
            // because of priorityClassName, we need to add it to kube-system
            namespace: HelmChartNamespaces::KubeSystem,
            values: vec![
                ChartSetValue {
                    key: "loki.serviceName".to_string(),
                    value: loki_kube_dns_prefix.clone(),
                },
                // it's mandatory to get this class to ensure paused infra will behave properly on restore
                ChartSetValue {
                    key: "priorityClassName".to_string(),
                    value: "system-node-critical".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let loki = CommonChart {
        chart_info: ChartInfo {
            name: "loki".to_string(),
            path: chart_path("common/charts/loki"),
            namespace: loki_namespace,
            values_files: vec![chart_path("chart_values/loki.yaml")],
            values: vec![
                ChartSetValue {
                    key: "config.storage_config.aws.s3".to_string(),
                    value: qovery_terraform_config.loki_storage_config_do_space,
                },
                ChartSetValue {
                    key: "config.storage_config.aws.endpoint".to_string(),
                    value: format!("{}.digitaloceanspaces.com", chart_config_prerequisites.region.clone()),
                },
                ChartSetValue {
                    key: "config.storage_config.aws.region".to_string(),
                    value: chart_config_prerequisites.region.clone(),
                },
                ChartSetValue {
                    key: "aws_iam_loki_storage_key".to_string(),
                    value: chart_config_prerequisites.do_space_access_id.clone(),
                },
                ChartSetValue {
                    key: "aws_iam_loki_storage_secret".to_string(),
                    value: chart_config_prerequisites.do_space_secret_key.clone(),
                },
                // DigitalOcean do not support encryption yet
                // https://docs.digitalocean.com/reference/api/spaces-api/
                ChartSetValue {
                    key: "config.storage_config.aws.sse_encryption".to_string(),
                    value: "false".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "2Gi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let mut prometheus_operator = CommonChart {
        chart_info: ChartInfo {
            name: "prometheus-operator".to_string(),
            path: chart_path("/common/charts/prometheus-operator"),
            namespace: prometheus_namespace,
            // high timeout because on bootstrap, it's one of the biggest dependencies and on upgrade, it can takes time
            // to upgrade because of the CRD and the number of elements it has to deploy
            timeout: "480".to_string(),
            values_files: vec![chart_path("chart_values/prometheus_operator.yaml")],
            values: vec![
                ChartSetValue {
                    key: "nameOverride".to_string(),
                    value: "prometheus-operator".to_string(),
                },
                ChartSetValue {
                    key: "fullnameOverride".to_string(),
                    value: "prometheus-operator".to_string(),
                },
                ChartSetValue {
                    key: "prometheus.prometheusSpec.externalUrl".to_string(),
                    value: prometheus_internal_url.clone(),
                },
                // Limits kube-state-metrics
                ChartSetValue {
                    key: "kube-state-metrics.resources.limits.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "kube-state-metrics.resources.requests.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "kube-state-metrics.resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "kube-state-metrics.resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                // Limits prometheus-node-exporter
                ChartSetValue {
                    key: "prometheus-node-exporter.resources.limits.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "prometheus-node-exporter.resources.requests.cpu".to_string(),
                    value: "10m".to_string(),
                },
                ChartSetValue {
                    key: "prometheus-node-exporter.resources.limits.memory".to_string(),
                    value: "32Mi".to_string(),
                },
                ChartSetValue {
                    key: "prometheus-node-exporter.resources.requests.memory".to_string(),
                    value: "32Mi".to_string(),
                },
                // Limits kube-state-metrics
                ChartSetValue {
                    key: "kube-state-metrics.resources.limits.cpu".to_string(),
                    value: "30m".to_string(),
                },
                ChartSetValue {
                    key: "kube-state-metrics.resources.requests.cpu".to_string(),
                    value: "10m".to_string(),
                },
                ChartSetValue {
                    key: "kube-state-metrics.resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "kube-state-metrics.resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "prometheusOperator.resources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "prometheusOperator.resources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "prometheusOperator.resources.limits.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                ChartSetValue {
                    key: "prometheusOperator.resources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
            ],
            ..Default::default()
        },
    };
    if chart_config_prerequisites.test_cluster {
        prometheus_operator.chart_info.values.push(ChartSetValue {
            key: "defaultRules.config".to_string(),
            value: "{}".to_string(),
        })
    }

    let prometheus_adapter = CommonChart {
        chart_info: ChartInfo {
            name: "prometheus-adapter".to_string(),
            path: chart_path("common/charts/prometheus-adapter"),
            namespace: prometheus_namespace,
            values: vec![
                ChartSetValue {
                    key: "metricsRelistInterval".to_string(),
                    value: "30s".to_string(),
                },
                ChartSetValue {
                    key: "prometheus.url".to_string(),
                    value: prometheus_internal_url.clone(),
                },
                ChartSetValue {
                    key: "podDisruptionBudget.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "podDisruptionBudget.maxUnavailable".to_string(),
                    value: "1".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let metrics_server = CommonChart {
        chart_info: ChartInfo {
            name: "metrics-server".to_string(),
            path: chart_path("common/charts/metrics-server"),
            values: vec![
                ChartSetValue {
                    key: "extraArgs.kubelet-preferred-address-types".to_string(),
                    value: "InternalIP".to_string(),
                },
                ChartSetValue {
                    key: "apiService.create".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "250m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "250m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "256Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "256Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let kube_state_metrics = CommonChart {
        chart_info: ChartInfo {
            name: "kube-state-metrics".to_string(),
            namespace: HelmChartNamespaces::Prometheus,
            path: chart_path("common/charts/kube-state-metrics"),
            values: vec![
                ChartSetValue {
                    key: "prometheus.monitor.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "75m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "75m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let grafana_datasources = format!(
        "
datasources:
  datasources.yaml:
    apiVersion: 1
    datasources:
      - name: Prometheus
        type: prometheus
        url: \"{}:9090\"
        access: proxy
        isDefault: true
      - name: PromLoki
        type: prometheus
        url: \"http://{}.{}.svc:3100/loki\"
        access: proxy
        isDefault: false
      - name: Loki
        type: loki
        url: \"http://{}.{}.svc:3100\"
      ",
        prometheus_internal_url.clone(),
        &loki.chart_info.name,
        get_chart_namespace(loki_namespace),
        &loki.chart_info.name,
        get_chart_namespace(loki_namespace),
    );

    let grafana = CommonChart {
        chart_info: ChartInfo {
            name: "grafana".to_string(),
            path: chart_path("common/charts/grafana"),
            namespace: prometheus_namespace,
            values_files: vec![chart_path("chart_values/grafana.yaml")],
            yaml_files_content: vec![ChartValuesGenerated {
                filename: "grafana_generated.yaml".to_string(),
                yaml_content: grafana_datasources,
            }],
            ..Default::default()
        },
    };

    let cert_manager = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager".to_string(),
            path: chart_path("common/charts/cert-manager"),
            namespace: HelmChartNamespaces::CertManager,
            values: vec![
                ChartSetValue {
                    key: "installCRDs".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "2".to_string(),
                },
                // https://cert-manager.io/docs/configuration/acme/dns01/#setting-nameservers-for-dns01-self-check
                ChartSetValue {
                    key: "extraArgs".to_string(),
                    value: "{--dns01-recursive-nameservers-only,--dns01-recursive-nameservers=1.1.1.1:53\\,8.8.8.8:53}"
                        .to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.prometheusInstance".to_string(),
                    value: "qovery".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                // Webhooks resources limits
                ChartSetValue {
                    key: "webhook.resources.limits.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.requests.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.limits.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                ChartSetValue {
                    key: "webhook.resources.requests.memory".to_string(),
                    value: "128Mi".to_string(),
                },
                // Cainjector resources limits
                ChartSetValue {
                    key: "cainjector.resources.limits.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.limits.memory".to_string(),
                    value: "1Gi".to_string(),
                },
                ChartSetValue {
                    key: "cainjector.resources.requests.memory".to_string(),
                    value: "1Gi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let mut cert_manager_config = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager-configs".to_string(),
            path: chart_path("common/charts/cert-manager-configs"),
            namespace: HelmChartNamespaces::CertManager,
            values: vec![
                ChartSetValue {
                    key: "externalDnsProvider".to_string(),
                    value: chart_config_prerequisites.external_dns_provider.clone(),
                },
                ChartSetValue {
                    key: "acme.letsEncrypt.emailReport".to_string(),
                    value: chart_config_prerequisites.dns_email_report.clone(),
                },
                ChartSetValue {
                    key: "acme.letsEncrypt.acmeUrl".to_string(),
                    value: chart_config_prerequisites.acme_url.clone(),
                },
                ChartSetValue {
                    key: "managedDns".to_string(),
                    value: chart_config_prerequisites.managed_dns_helm_format.clone(),
                },
            ],
            ..Default::default()
        },
    };
    if chart_config_prerequisites.external_dns_provider == "cloudflare" {
        cert_manager_config.chart_info.values.push(ChartSetValue {
            key: "provider.cloudflare.apiToken".to_string(),
            value: chart_config_prerequisites.cloudflare_api_token.clone(),
        });
        cert_manager_config.chart_info.values.push(ChartSetValue {
            key: "provider.cloudflare.email".to_string(),
            value: chart_config_prerequisites.cloudflare_email.clone(),
        })
    }

    let nginx_ingress = CommonChart {
        chart_info: ChartInfo {
            name: "nginx-ingress".to_string(),
            path: chart_path("common/charts/nginx-ingress"),
            namespace: HelmChartNamespaces::NginxIngress,
            // Because of NLB, svc can take some time to start
            timeout: "300".to_string(),
            values_files: vec![chart_path("chart_values/nginx-ingress.yaml")],
            values: vec![
                // Controller resources limits
                ChartSetValue {
                    key: "controller.resources.limits.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.requests.cpu".to_string(),
                    value: "100m".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.limits.memory".to_string(),
                    value: "768Mi".to_string(),
                },
                ChartSetValue {
                    key: "controller.resources.requests.memory".to_string(),
                    value: "768Mi".to_string(),
                },
                // Default backend resources limits
                ChartSetValue {
                    key: "defaultBackend.resources.limits.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "defaultBackend.resources.requests.cpu".to_string(),
                    value: "10m".to_string(),
                },
                ChartSetValue {
                    key: "defaultBackend.resources.limits.memory".to_string(),
                    value: "32Mi".to_string(),
                },
                ChartSetValue {
                    key: "defaultBackend.resources.requests.memory".to_string(),
                    value: "32Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let digital_mobius = CommonChart {
        chart_info: ChartInfo {
            name: "digital-mobius".to_string(),
            path: "charts/digital-mobius".to_string(),
            values: vec![
                ChartSetValue {
                    key: "environmentVariables.LOG_LEVEL".to_string(),
                    value: "debug".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.DELAY_NODE_CREATION".to_string(),
                    value: "5m".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.DIGITAL_OCEAN_TOKEN".to_string(),
                    value: chart_config_prerequisites.do_token.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.DIGITAL_OCEAN_CLUSTER_ID".to_string(),
                    // todo: fill this
                    value: "".to_string(),
                },
                ChartSetValue {
                    key: "enabledFeatures.disableDryRun".to_string(),
                    value: "true".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let pleco = CommonChart {
        chart_info: ChartInfo {
            name: "pleco".to_string(),
            path: chart_path("common/charts/pleco"),
            values_files: vec![chart_path("chart_values/pleco.yaml")],
            values: vec![
                ChartSetValue {
                    key: "environmentVariables.PLECO_IDENTIFIER".to_string(),
                    value: chart_config_prerequisites.cluster_id.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.LOG_LEVEL".to_string(),
                    value: "debug".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let k8s_token_rotate = CommonChart {
        chart_info: ChartInfo {
            name: "k8s-token-rotate".to_string(),
            path: "charts/do-k8s-token-rotate".to_string(),
            values: vec![
                ChartSetValue {
                    key: "environmentVariables.DO_API_TOKEN".to_string(),
                    value: chart_config_prerequisites.do_token.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.SPACES_KEY_ACCESS".to_string(),
                    value: chart_config_prerequisites.do_space_access_id.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.SPACES_SECRET_KEY".to_string(),
                    value: chart_config_prerequisites.do_space_secret_key.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.SPACES_BUCKET".to_string(),
                    value: chart_config_prerequisites.do_space_bucket_kubeconfig.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.SPACES_REGION".to_string(),
                    value: chart_config_prerequisites.region.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.SPACES_FILENAME".to_string(),
                    value: chart_config_prerequisites.do_space_kubeconfig_filename.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.K8S_CLUSTER_ID".to_string(),
                    value: chart_config_prerequisites.cluster_id.clone(),
                },
            ],
            ..Default::default()
        },
    };

    let qovery_agent_version: QoveryAgent = match get_qovery_app_version(
        QoveryAppName::Agent,
        &chart_config_prerequisites.infra_options.agent_version_controller_token,
        &chart_config_prerequisites.infra_options.qovery_api_url,
        &chart_config_prerequisites.cluster_id,
    ) {
        Ok(x) => x,
        Err(e) => {
            let msg = format!("Qovery agent version couldn't be retrieved. {}", e);
            error!("{}", &msg);
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(msg),
            });
        }
    };
    let qovery_agent = CommonChart {
        chart_info: ChartInfo {
            name: "qovery-agent".to_string(),
            path: chart_path("common/charts/qovery-agent"),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: qovery_agent_version.version,
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_HOST_URL".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_nats_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_USERNAME".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_nats_user.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_PASSWORD".to_string(),
                    value: chart_config_prerequisites
                        .infra_options
                        .qovery_nats_password
                        .to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.LOKI_URL".to_string(),
                    value: format!("http://{}.cluster.local:3100", loki_kube_dns_prefix),
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_REGION".to_string(),
                    value: chart_config_prerequisites.region.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_PROVIDER".to_string(),
                    value: chart_config_prerequisites.cloud_provider.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.KUBERNETES_ID".to_string(),
                    value: chart_config_prerequisites.cluster_id.clone(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "200m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "500Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "500Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let qovery_engine_version: QoveryEngine = match get_qovery_app_version(
        QoveryAppName::Engine,
        &chart_config_prerequisites.infra_options.engine_version_controller_token,
        &chart_config_prerequisites.infra_options.qovery_api_url,
        &chart_config_prerequisites.cluster_id,
    ) {
        Ok(x) => x,
        Err(e) => {
            let msg = format!("Qovery engine version couldn't be retrieved. {}", e);
            error!("{}", &msg);
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(msg),
            });
        }
    };
    let qovery_engine = CommonChart {
        chart_info: ChartInfo {
            name: "qovery-engine".to_string(),
            path: chart_path("common/charts/qovery-engine"),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: qovery_engine_version.version,
                },
                // need kubernetes 1.18, should be well tested before activating it
                ChartSetValue {
                    key: "autoscaler.enabled".to_string(),
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "metrics.enabled".to_string(),
                    value: chart_config_prerequisites.ff_metrics_history_enabled.to_string(),
                },
                ChartSetValue {
                    key: "volumes.storageClassName".to_string(),
                    value: "do-volume-standard-0".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.QOVERY_NATS_URL".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_nats_url.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.QOVERY_NATS_USER".to_string(),
                    value: chart_config_prerequisites.infra_options.qovery_nats_user.to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.QOVERY_NATS_PASSWORD".to_string(),
                    value: chart_config_prerequisites
                        .infra_options
                        .qovery_nats_password
                        .to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION".to_string(),
                    value: chart_config_prerequisites.organization_id.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_PROVIDER".to_string(),
                    value: chart_config_prerequisites.cloud_provider.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.REGION".to_string(),
                    value: chart_config_prerequisites.region.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.LIB_ROOT_DIR".to_string(),
                    value: "/home/qovery/lib".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.DOCKER_HOST".to_string(),
                    value: "tcp://0.0.0.0:2375".to_string(),
                },
                // engine resources limits
                ChartSetValue {
                    key: "engineResources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.limits.memory".to_string(),
                    value: "512Mi".to_string(),
                },
                ChartSetValue {
                    key: "engineResources.requests.memory".to_string(),
                    value: "512Mi".to_string(),
                },
                // build resources limits
                ChartSetValue {
                    key: "buildResources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "buildResources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "buildResources.limits.memory".to_string(),
                    value: "4Gi".to_string(),
                },
                ChartSetValue {
                    key: "buildResources.requests.memory".to_string(),
                    value: "4Gi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    // chart deployment order matters!!!
    let level_1: Vec<Box<dyn HelmChart>> = vec![Box::new(q_storage_class), Box::new(coredns_config)];

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![];

    let mut level_3: Vec<Box<dyn HelmChart>> = vec![];

    let mut level_4: Vec<Box<dyn HelmChart>> = vec![Box::new(metrics_server), Box::new(external_dns)];

    let mut level_5: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress), Box::new(cert_manager)];

    let mut level_6: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(qovery_agent),
        Box::new(qovery_engine),
        Box::new(digital_mobius),
        Box::new(k8s_token_rotate),
    ];

    // observability
    if chart_config_prerequisites.ff_metrics_history_enabled {
        level_2.push(Box::new(prometheus_operator));
        level_4.push(Box::new(prometheus_adapter));
        level_4.push(Box::new(kube_state_metrics));
    }
    if chart_config_prerequisites.ff_log_history_enabled {
        level_3.push(Box::new(promtail));
        level_4.push(Box::new(loki));
    }

    if chart_config_prerequisites.ff_metrics_history_enabled || chart_config_prerequisites.ff_log_history_enabled {
        level_6.push(Box::new(grafana))
    };

    // pleco
    if !chart_config_prerequisites.disable_pleco {
        level_5.push(Box::new(pleco));
    }

    info!("charts configuration preparation finished");
    Ok(vec![level_1, level_2, level_3, level_4, level_5, level_6])
}
