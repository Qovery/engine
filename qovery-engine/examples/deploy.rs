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
use qovery_engine::models::{
    Action, Application, Environment, EnvironmentVariable, GitCredentials,
};
use qovery_engine::session::Session;
use qovery_engine::transaction::TransactionResult;

fn main() {
    env_logger::init();

    let environment = Environment {
        owner_id: "".to_string(),
        project_id: "".to_string(),
        environment_id: "".to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: "".to_string(),
            name: "simple-example-node-with-postgresql".to_string(),
            git_url: "https://github.com/Qovery/simple-example-node-with-postgresql.git"
                .to_string(),
            commit_id: "f400e2f199e6a7eb446690b6f2df1017dbbae518".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "v1.d6b3b7db582eab1b85df90df5f558ac5830624f9".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![],
            environment_variables: vec![EnvironmentVariable {
                key: "KEY_TEST_1".to_string(),
                value: "VAL_TEST_1".to_string(),
            }],
        }],
        routers: vec![],
        databases: vec![],
    };

    // use DockerHub
    //let container_registry = DockerHub::new("qoveryrd", "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24");

    // use ECR
    let container_registry = ECR::new(
        "123-abc",
        "my-default-ecr",
        "AKIAZ4KMLSYJLRGNNFNI",
        "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
        "us-east-2",
    );

    let build_platform = LocalDocker::new("123456", "my-local-docker");

    let cloud_provider = AWS::new(
        "123-abc",
        "my-default-aws",
        "AKIAZ4KMLSYJLRGNNFNI",
        "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
    );

    let nodes: Vec<Node> = vec![];

    let eks = EKS::new(
        "123abc",
        "my-k8s-cluster",
        "1.14",
        "us-east-2",
        &cloud_provider,
        nodes,
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

    match tx.build_environment(&environment) {
        Ok(_) => {}
        Err(err) => panic!("environment error"),
    }

    tx.deploy_environment(&eks, &environment);

    match tx.commit() {
        TransactionResult::Ok => println!("execution: ok"),
        TransactionResult::Rollback(c) => println!("execution: rollback"),
        TransactionResult::UnrecoverableError(c, r) => println!("execution: unrecoverable error"),
    };
}
