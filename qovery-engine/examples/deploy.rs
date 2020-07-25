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
    Action, Application, CustomDomain, Database, DatabaseKind, Environment, EnvironmentAction,
    EnvironmentVariable, GitCredentials, Kind, Route, Router, Storage, StorageType,
};
use qovery_engine::session::Session;
use qovery_engine::transaction::TransactionResult;

fn main() {
    env_logger::init();

    let execution_id = Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-");

    let environment = Environment {
        execution_id: execution_id.clone(),
        id: "odiajwio6468a468".to_string(),
        kind: Kind::Development,
        owner_id: "123456basuiug".to_string(),
        project_id: "adoiwajd45ad4w".to_string(),
        organization_id: "adwopakdpo221".to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: "owiahdiu877".to_string(),
            name: "simple-example-node-with-postgresql".to_string(),
            git_url: "https://github.com/Qovery/simple-example-node-with-postgresql.git"
                .to_string(),
            commit_id: "f400e2f199e6a7eb446690b6f2df1017dbbae518".to_string(),
            dockerfile_path: "Dockerfile".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "v1.d6b3b7db582eab1b85df90df5f558ac5830624f9".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![Storage {
                id: "adawd5wa4d65aw4".to_string(),
                name: "photos".to_string(),
                storage_type: StorageType::SSD,
                size_in_gib: 10,
                mount_point: "/mnt/photos".to_string(),
                snapshot_retention_in_days: 30,
            }],
            environment_variables: vec![
                EnvironmentVariable {
                    key: "KEY_TEST_1".to_string(),
                    value: "VAL_TEST_1".to_string(),
                },
                EnvironmentVariable {
                    key: "KEY_TEST_2".to_string(),
                    value: "VAL_TEST_2".to_string(),
                },
            ],
            branch: "master".to_string(),
            private_port: Some(8080),
        }],
        routers: vec![
            Router {
                id: "ofejoiafj5464".to_string(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: "toto-default.qovery.io".to_string(),
                public_port: 443,
                custom_domains: vec![CustomDomain {
                    domain: "toto.custom.io".to_string(),
                    target_domain: "toto.qovery.io".to_string(),
                }],
                routes: vec![Route {
                    path: "/*".to_string(),
                    application_name: "simple-example-node-with-postgresql".to_string(),
                }],
            },
            Router {
                id: "adawhdiua545545".to_string(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: "coco-default.qovery.io".to_string(),
                public_port: 443,
                custom_domains: vec![CustomDomain {
                    domain: "coco.custom.io".to_string(),
                    target_domain: "coco.qovery.io".to_string(),
                }],
                routes: vec![Route {
                    path: "/coco/*".to_string(),
                    application_name: "simple-example-node-with-postgresql".to_string(),
                }],
            },
        ],
        databases: vec![Database {
            kind: DatabaseKind::PostgreSQL,
            action: Action::Create,
            id: "waoidja468787454".to_string(),
            name: "my-psql".to_string(),
            version: "11.8.0".to_string(),
            fqdn_id: "no-fqdn-test".to_string(),
            fqdn: "no-fqdn-test.qovery.io".to_string(),
            port: 5432,
            username: "superuser".to_string(),
            password: "BdcDconI2k8AVN6z".to_string(),
            disk_size_in_gib: 10,
        }],
        clone_from_environment_id: None,
    };

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

    let nodes: Vec<Node> = vec![];

    let eks = EKS::new(
        execution_id.as_str(),
        "my-eks-id-123",
        "my-default-eks",
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

    let environment_action = EnvironmentAction::Environment(environment);
    match tx.build_environment(&environment_action) {
        Ok(_) => {}
        Err(err) => panic!("environment error"),
    }

    tx.deploy_environment(&eks, &environment_action);

    match tx.commit() {
        TransactionResult::Ok => println!("execution: ok"),
        TransactionResult::Rollback(c) => println!("execution: rollback"),
        TransactionResult::UnrecoverableError(c, r) => println!("execution: unrecoverable error"),
    };
}
