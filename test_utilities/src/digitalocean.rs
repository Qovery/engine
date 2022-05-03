use const_format::formatcp;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DoksOptions;
use qovery_engine::cloud_provider::digitalocean::network::vpc::VpcInitKind;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::{CloudProvider, TerraformStateCredentials};
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::engine::EngineConfig;
use qovery_engine::io_models::{Context, EnvironmentRequest, NoOpProgressListener};
use std::sync::Arc;

use crate::cloudflare::dns_provider_cloudflare;
use crate::common::{get_environment_test_kubernetes, Cluster, ClusterDomain};
use crate::utilities::{build_platform_local_docker, FuncTestsSecrets};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::errors::EngineError;
use qovery_engine::logger::Logger;
use qovery_engine::models::digital_ocean::DoRegion;

pub const DO_KUBERNETES_MAJOR_VERSION: u8 = 1;
pub const DO_KUBERNETES_MINOR_VERSION: u8 = 20;
pub const DO_KUBERNETES_VERSION: &'static str =
    formatcp!("{}.{}", DO_KUBERNETES_MAJOR_VERSION, DO_KUBERNETES_MINOR_VERSION);
pub const DOCR_ID: &str = "registry-the-one-and-unique";
pub const DO_TEST_REGION: DoRegion = DoRegion::Amsterdam3;
pub const DO_MANAGED_DATABASE_INSTANCE_TYPE: &str = "";
pub const DO_MANAGED_DATABASE_DISK_TYPE: &str = "";
pub const DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE: &str = "";
pub const DO_SELF_HOSTED_DATABASE_DISK_TYPE: &str = "do-block-storage";

pub fn container_registry_digital_ocean(context: &Context) -> DOCR {
    let secrets = FuncTestsSecrets::new();
    DOCR::new(
        context.clone(),
        DOCR_ID,
        DOCR_ID,
        secrets.DIGITAL_OCEAN_TOKEN.unwrap().as_str(),
        Arc::new(Box::new(NoOpProgressListener {})),
    )
    .unwrap()
}

pub fn do_default_engine_config(context: &Context, logger: Box<dyn Logger>) -> EngineConfig {
    DO::docker_cr_engine(
        &context,
        logger,
        DO_TEST_REGION.to_string().as_str(),
        KubernetesKind::Doks,
        DO_KUBERNETES_VERSION.to_string(),
        &ClusterDomain::Default {
            cluster_id: context.cluster_id().to_string(),
        },
        None,
    )
}

impl Cluster<DO, DoksOptions> for DO {
    fn docker_cr_engine(
        context: &Context,
        logger: Box<dyn Logger>,
        localisation: &str,
        kubernetes_kind: KubernetesKind,
        kubernetes_version: String,
        cluster_domain: &ClusterDomain,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> EngineConfig {
        // use DigitalOcean Container Registry
        let container_registry = Box::new(container_registry_digital_ocean(context));
        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context, logger.clone()));

        // use Digital Ocean
        let cloud_provider: Arc<Box<dyn CloudProvider>> = Arc::new(Self::cloud_provider(context));
        let dns_provider: Arc<Box<dyn DnsProvider>> = Arc::new(dns_provider_cloudflare(context, cluster_domain));

        let k = get_environment_test_kubernetes(
            context,
            cloud_provider.clone(),
            kubernetes_kind,
            kubernetes_version.as_str(),
            dns_provider.clone(),
            logger.clone(),
            localisation,
            vpc_network_mode,
        );

        EngineConfig::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            k,
        )
    }

    fn cloud_provider(context: &Context) -> Box<DO> {
        let secrets = FuncTestsSecrets::new();
        let cluster_id = secrets
            .DIGITAL_OCEAN_TEST_CLUSTER_ID
            .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set");
        Box::new(DO::new(
            context.clone(),
            cluster_id.clone().as_str(),
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .expect("DIGITAL_OCEAN_KUBE_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            uuid::Uuid::new_v4(),
            secrets
                .DIGITAL_OCEAN_TOKEN
                .expect("DIGITAL_OCEAN_TOKEN is not set")
                .as_str(),
            secrets
                .DIGITAL_OCEAN_SPACES_ACCESS_ID
                .expect("DIGITAL_OCEAN_SPACES_ACCESS_ID is not set")
                .as_str(),
            secrets
                .DIGITAL_OCEAN_SPACES_SECRET_ID
                .expect("DIGITAL_OCEAN_SPACES_SECRET_ID is not set")
                .as_str(),
            format!("qovery-{}", cluster_id).as_str(),
            TerraformStateCredentials {
                access_key_id: secrets
                    .TERRAFORM_AWS_ACCESS_KEY_ID
                    .expect("TERRAFORM_AWS_ACCESS_KEY_ID is not set"),
                secret_access_key: secrets
                    .TERRAFORM_AWS_SECRET_ACCESS_KEY
                    .expect("TERRAFORM_AWS_SECRET_ACCESS_KEY is not set"),
                region: secrets.TERRAFORM_AWS_REGION.expect("TERRAFORM_AWS_REGION is not set"),
            },
        ))
    }

    fn kubernetes_nodes() -> Vec<NodeGroups> {
        vec![
            NodeGroups::new("groupdoks0".to_string(), 5, 10, "s-4vcpu-8gb".to_string(), 0)
                .expect("Problem while setup DOKS nodes"),
        ]
    }

    fn kubernetes_cluster_options(secrets: FuncTestsSecrets, cluster_name: Option<String>) -> DoksOptions {
        DoksOptions {
            vpc_cidr_block: "should-not-bet-set".to_string(), // vpc_cidr_set to autodetect will fil this empty string
            vpc_cidr_set: VpcInitKind::Autodetect,
            vpc_name: cluster_name.unwrap(),
            qovery_api_url: secrets.QOVERY_API_URL.unwrap(),
            qovery_grpc_url: secrets.QOVERY_GRPC_URL.unwrap(),
            qovery_cluster_secret_token: secrets.QOVERY_CLUSTER_SECRET_TOKEN.unwrap(),
            qovery_engine_location: EngineLocation::ClientSide,
            engine_version_controller_token: secrets.QOVERY_ENGINE_CONTROLLER_TOKEN.unwrap(),
            agent_version_controller_token: secrets.QOVERY_AGENT_CONTROLLER_TOKEN.unwrap(),
            grafana_admin_user: "admin".to_string(),
            grafana_admin_password: "qovery".to_string(),
            discord_api_key: secrets.DISCORD_API_URL.unwrap(),
            qovery_nats_url: secrets.QOVERY_NATS_URL.unwrap(),
            qovery_nats_user: secrets.QOVERY_NATS_USERNAME.unwrap(),
            qovery_nats_password: secrets.QOVERY_NATS_PASSWORD.unwrap(),
            qovery_ssh_key: secrets.QOVERY_SSH_USER.unwrap(),
            tls_email_report: secrets.LETS_ENCRYPT_EMAIL_REPORT.unwrap(),
        }
    }
}

pub fn clean_environments(
    context: &Context,
    _environments: Vec<EnvironmentRequest>,
    secrets: FuncTestsSecrets,
    _region: DoRegion,
) -> Result<(), EngineError> {
    let _do_cr = DOCR::new(
        context.clone(),
        "test",
        "test",
        secrets
            .DIGITAL_OCEAN_TOKEN
            .as_ref()
            .expect("DIGITAL_OCEAN_TOKEN is not set in secrets"),
        Arc::new(Box::new(NoOpProgressListener {})),
    );

    // FIXME: re-enable it, or let pleco do its job ?
    /*
    // delete images created in registry
    let registry_url = do_cr.login()?;
    for env in environments.iter() {
        for image in env
            .applications
            .iter()
            .map(|a| a.to_image(&registry_url))
            .collect::<Vec<Image>>()
        {
            //if let Err(e) = do_cr.delete_registry(&image.name) {
            //    return Err(e);
            //}
        }
    }
     */

    Ok(())
}
