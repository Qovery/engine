use std::env;

use chrono::Utc;
use rusoto_core::Region;

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::gcp::GCP;
use qovery_engine::cloud_provider::{CloudProvider, CloudProviderError};
use qovery_engine::config::Config;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{Action, Application, Environment, GitCredentials};
use qovery_engine::session::Session;
use qovery_engine::transaction::TransactionResult;

fn main() {
    env_logger::init();

    let execution_id = Utc::now().to_rfc3339();

    // use DockerHub
    //let container_registry = DockerHub::new("qoveryrd", "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24");

    // use ECR
    let container_registry = ECR::new(
        execution_id.as_str(),
        "my-ecr-id-123",
        "my-default-ecr",
        "AKIAZ4KMLSYJLRGNNFNI",
        "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
        "us-east-2",
    );

    let build_platform = LocalDocker::new(execution_id.as_str(), "123456", "my-local-docker");

    let cloud_provider = AWS::new(
        execution_id.as_str(),
        "my-aws-id-123",
        "adwopakdpo221",
        "my-default-aws",
        "AKIAZ4KMLSYJLRGNNFNI",
        "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
    );

    let config = Config::new(&build_platform, &container_registry, &cloud_provider);

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

    let nodes = vec![Node::new(4, 32), Node::new(4, 32), Node::new(4, 32)];

    let eks_eu_west_3 = EKS::new(
        execution_id.as_str(),
        "my-eks-id-123",
        "my-default-eks",
        "1.16",
        "eu-west-3",
        &cloud_provider,
        nodes,
    );
    tx.create_kubernetes(&eks_eu_west_3);

    // let nodes2 = vec![Node::new(4, 32), Node::new(4, 32), Node::new(4, 32)];
    //
    // let eks_eu_west_31 = EKS::new(
    //     "def456",
    //     "def456",
    //     "def456",
    //     "1.16",
    //     "eu-west-3",
    //     &cloud_provider,
    //     nodes2,
    // );
    // tx.create_kubernetes(&eks_eu_west_31);

    // let eks_us_east_2 = EKS::new(
    //     "456def",
    //     "my-us-east-2-k8s",
    //     "1.16",
    //     "us-east-2",
    //     &cloud_provider,
    //     nodes,
    // );
    // tx.create_kubernetes(&eks_us_east_2);

    match tx.commit() {
        TransactionResult::Ok => println!("execution: ok"),
        TransactionResult::Rollback(c) => println!("execution: rollback"),
        TransactionResult::UnrecoverableError(c, r) => println!("execution: unrecoverable error"),
    };
}
