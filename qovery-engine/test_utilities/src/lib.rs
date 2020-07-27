use chrono::Utc;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::models::{
    Action, Application, CustomDomain, Database, DatabaseKind, Environment, EnvironmentVariable,
    GitCredentials, Kind, Route, Router, Storage, StorageType,
};

pub const AWS_KEY_ID: &str = "AKIAZ4KMLSYJLRGNNFNI";
pub const AWS_ACCESS_KEY: &str = "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/";
pub const AWS_DEFAULT_REGION: &str = "us-east-2";
pub const ORGANIZATION_ID: &str = "adwopakdpo221";

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}

pub fn container_registry_ecr(execution_id: &str) -> ECR {
    ECR::new(
        execution_id,
        "my-ecr-id-123",
        "my-default-ecr",
        AWS_KEY_ID,
        AWS_ACCESS_KEY,
        AWS_DEFAULT_REGION,
    )
}

pub fn container_registry_docker_hub(execution_id: &str) -> DockerHub {
    DockerHub::new(
        execution_id,
        "my-docker-hub-id-123",
        "my-default-docker-hub",
        "qoveryrd",
        "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24",
    )
}

pub fn build_platform_local_docker(execution_id: &str) -> LocalDocker {
    LocalDocker::new(
        execution_id,
        "my-local-docker-id-123",
        "my-default-local-docker",
    )
}

pub fn aws_kubernetes_nodes() -> Vec<Node> {
    vec![Node::new(2, 16), Node::new(2, 16), Node::new(2, 16)]
}

pub fn cloud_provider_aws(execution_id: &str) -> AWS {
    AWS::new(
        execution_id,
        "my-aws-id-123",
        ORGANIZATION_ID,
        "my-default-aws",
        AWS_KEY_ID,
        AWS_ACCESS_KEY,
    )
}

pub fn aws_kubernetes_eks<'a>(
    execution_id: &str,
    cloud_provider: &'a AWS,
    nodes: Vec<Node>,
) -> EKS<'a> {
    EKS::<'a>::new(
        execution_id,
        "my-eks-id-123",
        "my-default-eks",
        "1.16",
        "us-east-2",
        cloud_provider,
        nodes,
    )
}

pub fn working_environment(execution_id: &str) -> Environment {
    Environment {
        execution_id: execution_id.to_string(),
        id: "odiajwio6468a468".to_string(),
        kind: Kind::Development,
        owner_id: "123456basuiug".to_string(),
        project_id: "adoiwajd45ad4w".to_string(),
        organization_id: ORGANIZATION_ID.to_string(),
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
                snapshot_retention_in_days: 0,
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
            private_port: Some(3000),
            total_cpus: 1,
            total_ram_in_mib: 256,
            total_instances: 2,
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
            total_cpus: 2,
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
        }],
        clone_from_environment_id: None,
    }
}
