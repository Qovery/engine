use crate::cloud_provider::helm::{
    get_chart_namespace, ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces,
};
use crate::cmd::kubectl::{kubectl_exec_get_daemonset, kubectl_exec_with_output};
use crate::error::{SimpleError, SimpleErrorKind};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsQoveryTerraformConfig {
    pub cloud_provider: String,
    pub region: String,
    pub cluster_name: String,
    pub cluster_id: String,
    pub organization_id: String,
    pub test_cluster: String,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    // feature flags
    pub feature_flag_metrics_history: String,
    pub feature_flag_log_history: String,
    // nats
    pub nats_host_url: String,
    pub nats_username: String,
    pub nats_password: String,
    pub aws_iam_eks_user_mapper_key: String,
    pub aws_iam_eks_user_mapper_secret: String,
    pub aws_iam_cluster_autoscaler_key: String,
    pub aws_iam_cluster_autoscaler_secret: String,
    // dns
    pub managed_dns_resolvers_terraform_format: String,
    pub external_dns_provider: String,
    pub dns_email_report: String,
    pub cloudflare_api_token: String,
    pub cloudflare_email: String,
    // tls
    pub acme_server_url: String,
    pub managed_dns_domains_terraform_format: String,
    // logs
    pub loki_storage_config_aws_s3: String,
    pub aws_iam_loki_storage_key: String,
    pub aws_iam_loki_storage_secret: String,
    // qovery
    pub qovery_agent_version: String,
    pub qovery_engine_version: String,
}

pub fn aws_helm_charts(qovery_terraform_config_file: &str) -> Result<Vec<Vec<Box<dyn HelmChart>>>, SimpleError> {
    let qovery_terraform_config = match serde_json::from_str::<AwsQoveryTerraformConfig>(qovery_terraform_config_file) {
        Ok(x) => x,
        Err(e) => {
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(format!("{:?}", e)),
            })
        }
    };
    let prometheus_namespace = HelmChartNamespaces::Prometheus;
    let loki_namespace = HelmChartNamespaces::Logging;
    let loki_service_name = "loki".to_string();

    // Qovery storage class
    let q_storage_class = CommonChart {
        chart_info: ChartInfo {
            name: "q-storageclass".to_string(),
            path: "charts/q-storageclass".to_string(),
            ..Default::default()
        },
    };

    let aws_vpc_cni_chart = AwsVpcCniChart {
        chart_info: ChartInfo {
            name: "aws-vpc-cni".to_string(),
            path: "charts/aws-vpc-cni".to_string(),
            values: vec![
                ChartSetValue {
                    key: "image.region".to_string(),
                    value: qovery_terraform_config.region.clone(),
                },
                ChartSetValue {
                    key: "image.pullPolicy".to_string(),
                    value: "IfNotPresent".to_string(),
                },
                ChartSetValue {
                    key: "crd.create".to_string(),
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "env.CLUSTER_NAME".to_string(),
                    value: qovery_terraform_config.cluster_name.clone(),
                },
                ChartSetValue {
                    key: "env.MINIMUM_IP_TARGET".to_string(),
                    value: "60".to_string(),
                },
                ChartSetValue {
                    key: "env.WARM_IP_TARGET".to_string(),
                    value: "10".to_string(),
                },
                ChartSetValue {
                    key: "env.MAX_ENI".to_string(),
                    value: "100".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "50".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let aws_iam_eks_user_mapper = CommonChart {
        chart_info: ChartInfo {
            name: "iam-eks-user-mapper".to_string(),
            path: "charts/iam-eks-user-mapper".to_string(),
            values: vec![
                ChartSetValue {
                    key: "aws.accessKey".to_string(),
                    value: qovery_terraform_config.aws_iam_eks_user_mapper_key,
                },
                ChartSetValue {
                    key: "aws.secretKey".to_string(),
                    value: qovery_terraform_config.aws_iam_eks_user_mapper_secret,
                },
                ChartSetValue {
                    key: "image.region".to_string(),
                    value: qovery_terraform_config.region.clone(),
                },
                ChartSetValue {
                    key: "syncIamGroup".to_string(),
                    value: "Admins".to_string(),
                },
                // resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "20m".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "10m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "32Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "32Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let aws_node_term_handler = CommonChart {
        chart_info: ChartInfo {
            name: "aws-node-term-handler".to_string(),
            path: "charts/aws-node-termination-handler".to_string(),
            values: vec![
                ChartSetValue {
                    key: "nameOverride".to_string(),
                    value: "aws-node-term-handler".to_string(),
                },
                ChartSetValue {
                    key: "fullnameOverride".to_string(),
                    value: "aws-node-term-handler".to_string(),
                },
                ChartSetValue {
                    key: "enableSpotInterruptionDraining".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "enableScheduledEventDraining".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "deleteLocalData".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "ignoreDaemonSets".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "podTerminationGracePeriod".to_string(),
                    value: "300".to_string(),
                },
                ChartSetValue {
                    key: "nodeTerminationGracePeriod".to_string(),
                    value: "120".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    // Calico for AWS
    let aws_calico = CommonChart {
        chart_info: ChartInfo {
            name: "calico".to_string(),
            path: "charts/aws-calico".to_string(),
            ..Default::default()
        },
    };

    let cluster_autoscaler = CommonChart {
        chart_info: ChartInfo {
            name: "cluster-autoscaler".to_string(),
            path: "common/charts/cluster-autoscaler".to_string(),
            values: vec![
                ChartSetValue {
                    key: "cloudProvider".to_string(),
                    value: "aws".to_string(),
                },
                ChartSetValue {
                    key: "awsRegion".to_string(),
                    value: qovery_terraform_config.region.clone(),
                },
                ChartSetValue {
                    key: "autoDiscovery.clusterName".to_string(),
                    value: qovery_terraform_config.cluster_name.clone(),
                },
                ChartSetValue {
                    key: "awsAccessKeyID".to_string(),
                    value: qovery_terraform_config.aws_iam_cluster_autoscaler_key,
                },
                ChartSetValue {
                    key: "awsSecretAccessKey".to_string(),
                    value: qovery_terraform_config.aws_iam_cluster_autoscaler_secret,
                },
                // It's mandatory to get this class to ensure paused infra will behave properly on restore
                ChartSetValue {
                    key: "priorityClassName".to_string(),
                    value: "system-cluster-critical".to_string(),
                },
                // cluster autoscaler options
                ChartSetValue {
                    key: "extraArgs.balance-similar-node-groups".to_string(),
                    value: "true".to_string(),
                },
                // observability
                ChartSetValue {
                    key: "serviceMonitor.enabled".to_string(),
                    value: qovery_terraform_config.feature_flag_metrics_history.clone(),
                },
                ChartSetValue {
                    key: "serviceMonitor.namespace".to_string(),
                    value: get_chart_namespace(prometheus_namespace),
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
                    value: "300Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "300Mi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let coredns_config = CommonChart {
        chart_info: ChartInfo {
            name: "coredns-config".to_string(),
            path: "charts/coredns-config".to_string(),
            values: vec![
                ChartSetValue {
                    key: "managed_dns".to_string(),
                    value: qovery_terraform_config.managed_dns_resolvers_terraform_format.clone(),
                },
                ChartSetValue {
                    key: "managed_dns_resolvers".to_string(),
                    value: qovery_terraform_config.managed_dns_resolvers_terraform_format,
                },
            ],
            ..Default::default()
        },
    };

    let external_dns = CommonChart {
        chart_info: ChartInfo {
            name: "externaldns".to_string(),
            path: "common/charts/external-dns".to_string(),
            values_files: vec!["chart_values/external-dns.yaml".to_string()],
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
            path: "common/charts/promtail".to_string(),
            namespace: loki_namespace,
            values: vec![
                ChartSetValue {
                    key: "loki.serviceName".to_string(),
                    value: loki_service_name.clone(),
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
            path: "common/charts/loki".to_string(),
            namespace: loki_namespace,
            values_files: vec!["chart_values/loki.yaml".to_string()],
            values: vec![
                ChartSetValue {
                    key: "config.storage_config.aws.s3".to_string(),
                    value: qovery_terraform_config.loki_storage_config_aws_s3,
                },
                ChartSetValue {
                    key: "config.storage_config.aws.region".to_string(),
                    value: qovery_terraform_config.region.clone(),
                },
                ChartSetValue {
                    key: "aws_iam_loki_storage_key".to_string(),
                    value: qovery_terraform_config.aws_iam_loki_storage_key,
                },
                ChartSetValue {
                    key: "aws_iam_loki_storage_secret".to_string(),
                    value: qovery_terraform_config.aws_iam_loki_storage_secret,
                },
                ChartSetValue {
                    key: "config.storage_config.aws.sse_encryption".to_string(),
                    value: "true".to_string(),
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
            path: "common/charts/prometheus-operator".to_string(),
            namespace: HelmChartNamespaces::Logging,
            // high timeout because on bootstrap, it's one of the biggest dependencies and on upgrade, it can takes time
            // to upgrade because of the CRD and the number of elements it has to deploy
            timeout: "480".to_string(),
            values_files: vec!["chart_values/prometheus_operator.yaml".to_string()],
            values: vec![
                ChartSetValue {
                    key: "nameOverride".to_string(),
                    value: "prometheus-operator".to_string(),
                },
                ChartSetValue {
                    key: "fullnameOverride".to_string(),
                    value: "prometheus-operator".to_string(),
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
    if &qovery_terraform_config.test_cluster == "true" {
        prometheus_operator.chart_info.values.push(ChartSetValue {
            key: "defaultRules.config".to_string(),
            value: "{}".to_string(),
        })
    }

    let prometheus_adapter = CommonChart {
        chart_info: ChartInfo {
            name: "prometheus-adapter".to_string(),
            path: "common/charts/prometheus-adapter".to_string(),
            namespace: HelmChartNamespaces::Logging,
            values: vec![
                ChartSetValue {
                    key: "metricsRelistInterval".to_string(),
                    value: "30s".to_string(),
                },
                ChartSetValue {
                    key: "prometheus.url".to_string(),
                    value: format!(
                        "http://prometheus-operated.{}.svc",
                        get_chart_namespace(prometheus_namespace)
                    ),
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

    let metric_server = CommonChart {
        chart_info: ChartInfo {
            name: "metrics-server".to_string(),
            path: "common/charts/metrics-server".to_string(),
            values: vec![
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

    // todo: add custom datasource to values_file
    let grafana = CommonChart {
        chart_info: ChartInfo {
            name: "grafana".to_string(),
            path: "common/charts/grafana".to_string(),
            namespace: prometheus_namespace,
            values_files: vec!["chart_values/grafana.yaml".to_string()],
            ..Default::default()
        },
    };

    let cert_manager = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager".to_string(),
            path: "common/charts/cert-manager".to_string(),
            namespace: HelmChartNamespaces::CertManager,
            values_files: vec!["chart_values/cert-manager.yaml".to_string()],
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
                    value: qovery_terraform_config.feature_flag_metrics_history.clone(),
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
            path: "common/charts/cert-manager-configs".to_string(),
            namespace: HelmChartNamespaces::CertManager,
            values: vec![
                ChartSetValue {
                    key: "externalDnsProvider".to_string(),
                    value: qovery_terraform_config.external_dns_provider.clone(),
                },
                ChartSetValue {
                    key: "acme.letsEncrypt.emailReport".to_string(),
                    value: qovery_terraform_config.dns_email_report,
                },
                ChartSetValue {
                    key: "acme.letsEncrypt.acmeUrl".to_string(),
                    value: qovery_terraform_config.acme_server_url,
                },
                ChartSetValue {
                    key: "managedDns".to_string(),
                    value: qovery_terraform_config.managed_dns_domains_terraform_format,
                },
            ],
            ..Default::default()
        },
    };
    if &qovery_terraform_config.external_dns_provider == "cloudflare" {
        cert_manager_config.chart_info.values.push(ChartSetValue {
            key: "cloudflare_api_token".to_string(),
            value: qovery_terraform_config.cloudflare_api_token,
        });
        cert_manager_config.chart_info.values.push(ChartSetValue {
            key: "cloudflare_email".to_string(),
            value: qovery_terraform_config.cloudflare_email,
        })
    }

    let nginx_ingress = CommonChart {
        chart_info: ChartInfo {
            name: "nginx-ingress".to_string(),
            path: "common/charts/nginx-ingress".to_string(),
            namespace: HelmChartNamespaces::NginxIngress,
            // Because of NLB, svc can take some time to start
            timeout: "300".to_string(),
            values_files: vec!["chart_values/nginx-ingress.yaml".to_string()],
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

    // todo: add missing parameters
    let pleco = CommonChart {
        chart_info: ChartInfo {
            name: "pleco".to_string(),
            path: "common/charts/pleco".to_string(),
            values: vec![
                ChartSetValue {
                    key: "environmentVariables.AWS_ACCESS_KEY_ID".to_string(),
                    value: qovery_terraform_config.aws_access_key_id,
                },
                ChartSetValue {
                    key: "environmentVariables.AWS_SECRET_ACCESS_KEY".to_string(),
                    value: qovery_terraform_config.aws_secret_access_key,
                },
                ChartSetValue {
                    key: "environmentVariables.LOG_LEVEL".to_string(),
                    value: "debug".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let qovery_agent = CommonChart {
        chart_info: ChartInfo {
            name: "qovery-agent".to_string(),
            path: "common/charts/qovery-agent".to_string(),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                // todo: directly get version from the engine, not from terraform helper
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: qovery_terraform_config.qovery_agent_version,
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_HOST_URL".to_string(),
                    value: qovery_terraform_config.nats_host_url.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_USERNAME".to_string(),
                    value: qovery_terraform_config.nats_username.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_PASSWORD".to_string(),
                    value: qovery_terraform_config.nats_password.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.LOKI_URL".to_string(),
                    value: format!(
                        "http://{}.{}.svc.cluster.local:3100",
                        loki_service_name,
                        get_chart_namespace(loki_namespace)
                    ),
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_REGION".to_string(),
                    value: qovery_terraform_config.region.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_PROVIDER".to_string(),
                    value: qovery_terraform_config.cloud_provider.clone(),
                },
                ChartSetValue {
                    key: "environmentVariables.KUBERNETES_ID".to_string(),
                    value: qovery_terraform_config.cluster_id,
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

    let qovery_engine = CommonChart {
        chart_info: ChartInfo {
            name: "qovery-engine".to_string(),
            path: "common/charts/qovery-engine".to_string(),
            namespace: HelmChartNamespaces::Qovery,
            values: vec![
                // todo: directly get version from the engine, not from terraform
                ChartSetValue {
                    key: "image.tag".to_string(),
                    value: qovery_terraform_config.qovery_engine_version,
                },
                // need kubernetes 1.18, should be well tested before activating it
                ChartSetValue {
                    key: "autoscaler.enabled".to_string(),
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "metrics.enabled".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "volumes.storageClassName".to_string(),
                    value: "aws-ebs-gp2-0".to_string(),
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_HOST_URL".to_string(),
                    value: qovery_terraform_config.nats_host_url,
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_USERNAME".to_string(),
                    value: qovery_terraform_config.nats_username,
                },
                ChartSetValue {
                    key: "environmentVariables.NATS_PASSWORD".to_string(),
                    value: qovery_terraform_config.nats_password,
                },
                ChartSetValue {
                    key: "environmentVariables.ORGANIZATION".to_string(),
                    value: qovery_terraform_config.organization_id,
                },
                ChartSetValue {
                    key: "environmentVariables.CLOUD_PROVIDER".to_string(),
                    value: qovery_terraform_config.cloud_provider,
                },
                ChartSetValue {
                    key: "environmentVariables.REGION".to_string(),
                    value: qovery_terraform_config.region,
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
                    key: "resources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "512Mi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "512Mi".to_string(),
                },
                // build resources limits
                ChartSetValue {
                    key: "resources.limits.cpu".to_string(),
                    value: "1".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.cpu".to_string(),
                    value: "500m".to_string(),
                },
                ChartSetValue {
                    key: "resources.limits.memory".to_string(),
                    value: "4Gi".to_string(),
                },
                ChartSetValue {
                    key: "resources.requests.memory".to_string(),
                    value: "4Gi".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    // chart deployment order matters!!!
    let level_1: Vec<Box<dyn HelmChart>> = vec![
        Box::new(q_storage_class),
        Box::new(coredns_config),
        Box::new(aws_vpc_cni_chart),
    ];

    let mut level_2: Vec<Box<dyn HelmChart>> = vec![];

    let mut level_3: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cluster_autoscaler),
        Box::new(aws_iam_eks_user_mapper),
        Box::new(aws_calico),
    ];

    let mut level_4: Vec<Box<dyn HelmChart>> = vec![
        Box::new(metric_server),
        Box::new(aws_node_term_handler),
        Box::new(external_dns),
    ];

    let level_5: Vec<Box<dyn HelmChart>> = vec![Box::new(nginx_ingress), Box::new(cert_manager), Box::new(pleco)];

    let mut level_6: Vec<Box<dyn HelmChart>> = vec![
        Box::new(cert_manager_config),
        Box::new(qovery_agent),
        Box::new(qovery_engine),
    ];

    if &qovery_terraform_config.feature_flag_metrics_history == "true" {
        level_2.push(Box::new(prometheus_operator));
        level_4.push(Box::new(prometheus_adapter));
    }
    if &qovery_terraform_config.feature_flag_log_history == "true" {
        level_3.push(Box::new(promtail));
        level_4.push(Box::new(loki));
    }

    if &qovery_terraform_config.feature_flag_metrics_history == "true"
        || &qovery_terraform_config.feature_flag_log_history == "true"
    {
        level_6.push(Box::new(grafana))
    };

    Ok(vec![level_1, level_2, level_3, level_4, level_5, level_6])
}

// AWS CNI

#[derive(Default)]
pub struct AwsVpcCniChart {
    pub chart_info: ChartInfo,
}

impl HelmChart for AwsVpcCniChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn pre_exec(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> Result<(), SimpleError> {
        let kinds = vec!["daemonSet", "clusterRole", "clusterRoleBinding", "serviceAccount"];
        let mut environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        environment_variables.push(("KUBECONFIG", kubernetes_config.to_str().unwrap()));

        match self.enable_cni_managed_by_helm(kubernetes_config, envs) {
            true => {
                info!("Enabling AWS CNI support by Helm");

                for kind in kinds {
                    info!("setting annotations and labels on {}/aws-node", &kind);
                    kubectl_exec_with_output(
                        vec![
                            "-n",
                            "kube-system",
                            "annotate",
                            "--overwrite",
                            kind,
                            "aws-node",
                            format!("meta.helm.sh/release-name={}", self.chart_info.name).as_str(),
                        ],
                        environment_variables.clone(),
                        |_| {},
                        |_| {},
                    )?;
                    kubectl_exec_with_output(
                        vec![
                            "-n",
                            "kube-system",
                            "annotate",
                            "--overwrite",
                            kind,
                            "aws-node",
                            "meta.helm.sh/release-namespace=kube-system",
                        ],
                        environment_variables.clone(),
                        |_| {},
                        |_| {},
                    )?;
                    kubectl_exec_with_output(
                        vec![
                            "-n",
                            "kube-system",
                            "label",
                            "--overwrite",
                            kind,
                            "aws-node",
                            "app.kubernetes.io/managed-by=Helm",
                        ],
                        environment_variables.clone(),
                        |_| {},
                        |_| {},
                    )?
                }

                info!("AWS CNI successfully deployed")
            }
            false => info!("AWS CNI is already supported by Helm, nothing to do"),
        };

        Ok(())
    }
}

impl AwsVpcCniChart {
    fn enable_cni_managed_by_helm(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> bool {
        let environment_variables = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();

        kubectl_exec_get_daemonset(
            kubernetes_config,
            &self.chart_info.name,
            self.namespace().as_str(),
            Some("k8s-app=aws-node,app.kubernetes.io/managed-by=Helm"),
            environment_variables,
        )
        .is_ok()
    }
}
