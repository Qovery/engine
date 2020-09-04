use chrono::Utc;
use dirs::home_dir;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::build_platform::BuildPlatform;
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::container_registry::ContainerRegistry;
use qovery_engine::engine::Engine;
use qovery_engine::models::{
    Action, Application, Context, CustomDomain, Database, DatabaseKind, Environment,
    EnvironmentVariable, GitCredentials, Kind, Route, Router, Storage, StorageType,
};
use qovery_engine::session::Session;
use std::borrow::Borrow;

use crate::utilities::build_platform_local_docker;

pub const AWS_KEY_ID: &str = "AKIAZ4KMLSYJLRGNNFNI";
pub const AWS_ACCESS_KEY: &str = "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/";
pub const AWS_DEFAULT_REGION: &str = "us-east-2";
pub const ORGANIZATION_ID: &str = "adwopakdpo221";
pub const AWS_KUBERNETES_VERSION: &str = "1.16";

pub fn init() {
    println!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );

    env_logger::init();
}

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}

pub fn context() -> Context {
    let execution_id = execution_id();
    let home_dir = std::env::var("WORKSPACE_ROOT_DIR")
        .unwrap_or(home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = std::env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");

    Context::new(
        execution_id.as_str(),
        home_dir.as_str(),
        lib_root_dir.as_str(),
    )
}

pub fn container_registry_ecr(context: &Context) -> ECR {
    ECR::new(
        context.clone(),
        "my-ecr-id-123",
        "my-default-ecr",
        AWS_KEY_ID,
        AWS_ACCESS_KEY,
        AWS_DEFAULT_REGION,
    )
}

pub fn container_registry_docker_hub(context: &Context) -> DockerHub {
    DockerHub::new(
        context.clone(),
        "my-docker-hub-id-123",
        "my-default-docker-hub",
        "qoveryrd",
        "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24",
    )
}

pub fn aws_kubernetes_nodes() -> Vec<Node> {
    vec![
        Node::new(2, 16),
        Node::new(2, 16),
        Node::new(2, 16),
        Node::new(2, 16),
    ]
}

pub fn cloud_provider_aws(context: &Context) -> AWS {
    AWS::new(
        context.clone(),
        "my-aws-id-123",
        ORGANIZATION_ID,
        "my-default-aws",
        AWS_KEY_ID,
        AWS_ACCESS_KEY,
    )
}

pub fn aws_kubernetes_eks<'a>(
    context: &Context,
    cloud_provider: &'a AWS,
    nodes: Vec<Node>,
) -> EKS<'a> {
    EKS::<'a>::new(
        context.clone(),
        "my-eks-on-us-east-2",
        "my-default-eks",
        AWS_KUBERNETES_VERSION,
        "us-east-2",
        cloud_provider,
        nodes,
    )
}

pub fn docker_ecr_aws_engine(context: &Context) -> Engine {
    // use ECR
    let container_registry = Box::new(container_registry_ecr(context));

    // use LocalDocker
    let build_platform = Box::new(build_platform_local_docker(context));

    // use AWS
    let cloud_provider = Box::new(cloud_provider_aws(context));

    Engine::new(
        context.clone(),
        build_platform,
        container_registry,
        cloud_provider,
    )
}

pub fn working_environment(context: &Context) -> Environment {
    Environment {
        execution_id: context.execution_id().to_string(),
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
                storage_type: StorageType::Ssd,
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
                default_domain: "toto-default.oom.sh".to_string(),
                public_port: 443,
                custom_domains: vec![CustomDomain {
                    domain: "toto.custom.io".to_string(),
                    target_domain: "toto.oom.sh".to_string(),
                }],
                routes: vec![Route {
                    path: "/".to_string(),
                    application_name: "simple-example-node-with-postgresql".to_string(),
                }],
            },
            Router {
                id: "adawhdiua545545".to_string(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: "coco-default.oom.sh".to_string(),
                public_port: 443,
                custom_domains: vec![CustomDomain {
                    domain: "coco.custom.io".to_string(),
                    target_domain: "coco.oom.sh".to_string(),
                }],
                routes: vec![Route {
                    path: "/coco".to_string(),
                    application_name: "simple-example-node-with-postgresql".to_string(),
                }],
            },
        ],
        databases: vec![
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                id: "waoidja468787454".to_string(),
                name: "my-psql".to_string(),
                version: "11.8.0".to_string(),
                fqdn_id: "my-postgresql-test-123".to_string(),
                fqdn: "my-postgresql-test-123.oom.sh".to_string(),
                port: 5432,
                username: "superuser".to_string(),
                password: "BdcDconI2k8AVN6z".to_string(),
                total_cpus: 2,
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
            }, /*,
               Database {
                   kind: DatabaseKind::MySQL,
                   action: Action::Create,
                   id: "adoiaj22390soj".to_string(),
                   name: "my-mysql".to_string(),
                   version: "11.8.0".to_string(),
                   fqdn_id: "my-mysql-test-123".to_string(),
                   fqdn: "my-mysql-test-123.oom.sh".to_string(),
                   port: 3306,
                   username: "superuser".to_string(),
                   password: "BdcDconI2k8AVN6z".to_string(),
                   total_cpus: 2,
                   total_ram_in_mib: 512,
                   disk_size_in_gib: 10,
               },
               Database {
                   kind: DatabaseKind::MongoDB,
                   action: Action::Create,
                   id: "waoidja468787454".to_string(),
                   name: "my-psql".to_string(),
                   version: "11.8.0".to_string(),
                   fqdn_id: "my-mongodb-test-123".to_string(),
                   fqdn: "my-mongodb-test-123.oom.sh".to_string(),
                   port: 5432,
                   username: "superuser".to_string(),
                   password: "BdcDconI2k8AVN6z".to_string(),
                   total_cpus: 2,
                   total_ram_in_mib: 512,
                   disk_size_in_gib: 10,
               },*/
        ],
        clone_from_environment_id: None,
    }
}

pub fn non_working_environment(context: &Context) -> Environment {
    let mut environment = working_environment(context);

    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://notworking.com".to_string();
            app
        })
        .collect::<Vec<_>>();

    environment
}
