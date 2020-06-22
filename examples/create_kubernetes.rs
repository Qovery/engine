use chrono::Utc;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::error::CloudProviderError;
use qovery_engine::cloud_provider::gcp::GCP;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::config::Config;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{
    Action, Application, CloudProvider as CP, Deployment, Environment, GitCredentials,
};
use qovery_engine::session::Session;
use qovery_engine::transaction::{ProgressInfo, ProgressListener};
use rusoto_core::Region;

fn main() {
    let container_registry = Box::new(DockerHub::new(
        "qoveryrd",
        "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24",
    ));

    let build_platform = Box::new(LocalDocker::new());

    let cloud_provider = Box::new(AWS::new(
        "AKIAZ4KMLSYJLRGNNFNI",
        "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
    ));

    let config = Config {
        build_platform,
        container_registry,
        cloud_provider,
    };

    let session = match config.session() {
        Ok(session) => session,
        Err(err) => match err {
            ConfigurationError::BuildPlatform(e) => panic!(e),
            ConfigurationError::ContainerRegistry(e) => panic!(e),
            ConfigurationError::CloudProvider(e) => match e {
                CloudProviderError::Credentials => panic!("bad cloud provider credentials"),
                CloudProviderError::Error(err) => panic!("qerror: err"),
                CloudProviderError::Unknown => panic!("cloud provider unknown error"),
            },
        },
    };

    let mut tx = session.transaction();

    let eks_eu_west_3 = EKS::new("my-eu-west-3-k8s", "1.16", "eu-west-3");
    tx.create_kubernetes(&eks_eu_west_3);

    let eks_us_east_2 = EKS::new("my-us-east-2-k8s", "1.16", "us-east-2");
    tx.create_kubernetes(&eks_us_east_2);

    tx.commit();
}
