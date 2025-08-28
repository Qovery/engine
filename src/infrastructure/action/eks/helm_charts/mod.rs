use chrono::{DateTime, Utc};

use crate::helm::HelmChart;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::kubernetes::aws::Options;
use crate::infrastructure::models::kubernetes::karpenter::KarpenterParameters;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesVersion};
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::models::CpuArchitecture;

use crate::errors::EngineError;
use crate::infrastructure::models::dns_provider::DnsProviderConfiguration;

use crate::environment::models::domain::ToHelmString;
use crate::environment::models::third_parties::LetsEncryptConfig;
use crate::infrastructure::action::deploy_helms::{HelmInfraContext, HelmInfraResources};
use crate::infrastructure::action::eks::AwsEksQoveryTerraformOutput;
use crate::infrastructure::action::eks::helm_charts::gen_charts::eks_helm_charts;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::io_models::context::Features;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::metrics::MetricsParameters;
use crate::string::terraform_list_format;
use std::collections::HashMap;

pub mod aws_alb_controller_chart;
pub mod aws_iam_eks_user_mapper_chart;
pub mod aws_node_term_handler_chart;
pub mod cluster_autoscaler_chart;
mod gen_charts;
pub mod gen_karpenter_charts;
pub mod karpenter;
pub mod karpenter_configuration;
pub mod karpenter_crd;

#[derive(Clone)]
pub struct EksChartsConfigPrerequisites {
    pub organization_id: String,
    pub organization_long_id: uuid::Uuid,
    pub cluster_id: String,
    pub cluster_long_id: uuid::Uuid,
    pub cluster_creation_date: DateTime<Utc>,
    pub region: AwsRegion,
    pub cluster_name: String,
    pub cpu_architectures: Vec<CpuArchitecture>,
    pub cloud_provider: String,
    pub qovery_engine_location: EngineLocation,
    pub ff_log_history_enabled: bool,
    pub ff_grafana_enabled: bool,
    pub managed_dns_helm_format: String,
    pub managed_dns_resolvers_terraform_format: String,
    pub managed_dns_root_domain_helm_format: String,
    pub lets_encrypt_config: LetsEncryptConfig,
    pub dns_provider_config: DnsProviderConfiguration,
    pub alb_controller_already_deployed: bool,
    pub kubernetes_version_upgrade_requested: bool,
    pub kubernetes_version: KubernetesVersion,
    // qovery options form json input
    pub infra_options: Options,
    pub cluster_advanced_settings: ClusterAdvancedSettings,
    pub is_karpenter_enabled: bool,
    pub karpenter_parameters: Option<KarpenterParameters>,
    pub aws_iam_eks_user_mapper_role_arn: String,
    pub aws_iam_cluster_autoscaler_role_arn: String,
    pub aws_iam_cloudwatch_role_arn: String,
    pub aws_iam_loki_role_arn: String,
    pub aws_s3_loki_bucket_name: String,
    pub loki_storage_config_aws_s3: String,
    pub metrics_parameters: Option<MetricsParameters>,
    pub aws_iam_eks_prometheus_role_arn: String,
    pub aws_s3_prometheus_bucket_name: String,
    pub karpenter_controller_aws_role_arn: String,
    pub cluster_security_group_id: String,
    pub aws_iam_alb_controller_arn: String,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
}

pub struct EksHelmsDeployment<'a> {
    context: HelmInfraContext,
    terraform_output: AwsEksQoveryTerraformOutput,
    cluster: &'a EKS,
    alb_already_deployed: bool,
    kubernetes_version_upgrade_requested: bool,
}

impl<'a> EksHelmsDeployment<'a> {
    pub fn new(
        context: HelmInfraContext,
        terraform_output: AwsEksQoveryTerraformOutput,
        cluster: &'a EKS,
        alb_already_deployed: bool,
        kubernetes_version_upgrade_requested: bool,
    ) -> Self {
        Self {
            context,
            terraform_output,
            cluster,
            alb_already_deployed,
            kubernetes_version_upgrade_requested,
        }
    }
}

impl HelmInfraResources for EksHelmsDeployment<'_> {
    type ChartPrerequisite = EksChartsConfigPrerequisites;

    fn charts_context(&self) -> &HelmInfraContext {
        &self.context
    }

    fn new_chart_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite {
        let dns_provider = infra_ctx.dns_provider();
        let cluster = self.cluster;
        EksChartsConfigPrerequisites {
            organization_id: infra_ctx.context().organization_short_id().to_string(),
            organization_long_id: *infra_ctx.context().organization_long_id(),
            infra_options: cluster.options.clone(),
            cluster_id: cluster.short_id().to_string(),
            cluster_long_id: cluster.long_id,
            region: cluster.region.clone(),
            kubernetes_version: cluster.version.clone(),
            cluster_name: cluster.cluster_name(),
            cpu_architectures: cluster.cpu_architectures(),
            cloud_provider: "aws".to_string(),
            qovery_engine_location: cluster.options.qovery_engine_location.clone(),
            ff_log_history_enabled: cluster.context().is_feature_enabled(&Features::LogsHistory),
            ff_grafana_enabled: cluster.context().is_feature_enabled(&Features::Grafana),
            managed_dns_helm_format: dns_provider.domain().to_helm_format_string(),
            managed_dns_resolvers_terraform_format: terraform_list_format(
                dns_provider.resolvers().iter().map(|x| x.clone().to_string()).collect(),
            ),
            managed_dns_root_domain_helm_format: dns_provider.domain().root_domain().to_helm_format_string(),
            lets_encrypt_config: LetsEncryptConfig::new(
                cluster.options.tls_email_report.to_string(),
                cluster.context().is_test_cluster(),
            ),
            dns_provider_config: dns_provider.provider_configuration(),
            cluster_advanced_settings: cluster.advanced_settings().clone(),
            is_karpenter_enabled: cluster.is_karpenter_enabled(),
            karpenter_parameters: cluster.get_karpenter_parameters(),
            aws_iam_eks_user_mapper_role_arn: self.terraform_output.aws_iam_eks_user_mapper_role_arn.clone(),
            aws_iam_cluster_autoscaler_role_arn: self.terraform_output.aws_iam_cluster_autoscaler_role_arn.clone(),
            aws_iam_cloudwatch_role_arn: self.terraform_output.aws_iam_cloudwatch_role_arn.clone(),
            aws_iam_loki_role_arn: self.terraform_output.aws_iam_loki_role_arn.clone(),
            aws_s3_loki_bucket_name: self.terraform_output.aws_s3_loki_bucket_name.clone(),
            loki_storage_config_aws_s3: self.terraform_output.loki_storage_config_aws_s3.clone(),
            karpenter_controller_aws_role_arn: self.terraform_output.karpenter_controller_aws_role_arn.clone(),
            cluster_security_group_id: self.terraform_output.cluster_security_group_id.clone(),
            alb_controller_already_deployed: self.alb_already_deployed,
            kubernetes_version_upgrade_requested: self.kubernetes_version_upgrade_requested,
            aws_iam_alb_controller_arn: self.terraform_output.aws_iam_alb_controller_arn.clone(),
            customer_helm_charts_override: cluster.customer_helm_charts_override.clone(),
            metrics_parameters: cluster.options.metrics_parameters.clone(),
            aws_iam_eks_prometheus_role_arn: self.terraform_output.aws_iam_eks_prometheus_role_arn.clone(),
            aws_s3_prometheus_bucket_name: self.terraform_output.aws_s3_prometheus_bucket_name.clone(),
            cluster_creation_date: cluster.created_at,
        }
    }

    fn gen_charts_to_deploy(
        &self,
        infra_ctx: &InfrastructureContext,
        charts_prerequisites: Self::ChartPrerequisite,
    ) -> Result<Vec<Vec<Box<dyn HelmChart>>>, Box<EngineError>> {
        eks_helm_charts(
            &charts_prerequisites,
            Some(self.context.destination_folder.to_string_lossy().as_ref()),
            &*infra_ctx.context().qovery_api,
            infra_ctx.dns_provider().domain(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_charts_setup_error(self.context.event_details.clone(), e)))
    }
}
