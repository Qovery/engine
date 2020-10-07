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
use serde_json::value::Value;
use std::borrow::Borrow;

use crate::utilities::init;
use crate::utilities::{build_platform_local_docker, generate_id};
use serde_json::map::Values;
extern crate serde;
extern crate serde_derive;
use crate::cloudflare::dns_provider_cloudflare;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::dns_provider::DnsProvider;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;

pub const AWS_KEY_ID: &str = "AKIA4IVG73IUU5NNVN5Q"; // AWS username: infra-test-deploy
pub const AWS_ACCESS_KEY: &str = "E9Ugsvv7MI3vCaHtn1qoxXU8KwNJeTWn3GfVLNYN";
pub const AWS_DEFAULT_REGION: &str = "us-east-2";
pub const ORGANIZATION_ID: &str = "azerl1aowkdoiqjdoiwjqdioqj";
pub const AWS_KUBERNETES_VERSION: &str = "1.16";

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
        None,
    )
}

pub fn container_registry_ecr(context: &Context) -> ECR {
    ECR::new(
        context.clone(),
        "ecr-test-id",
        "ecr-test-name",
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
        "aws-provider-1",
        ORGANIZATION_ID,
        "aws-provider-name",
        AWS_KEY_ID,
        AWS_ACCESS_KEY,
    )
}

pub fn aws_kubernetes_eks<'a>(
    context: &Context,
    cloud_provider: &'a AWS,
    dns_provider: &'a dyn DnsProvider,
    nodes: Vec<Node>,
) -> EKS<'a> {
    let mut file = File::open("tests/assets/eks-options.json").expect("file not found");
    let options_values = serde_json::from_reader(file).expect("JSON was not well-formatted");
    EKS::<'a>::new(
        context.clone(),
        "main-eks-cluster-test",
        "main-eks-cluster-test",
        AWS_KUBERNETES_VERSION,
        AWS_DEFAULT_REGION,
        cloud_provider,
        dns_provider,
        options_values,
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

    let dns_provider = Box::new(dns_provider_cloudflare(context));

    Engine::new(
        context.clone(),
        build_platform,
        container_registry,
        cloud_provider,
        dns_provider,
    )
}

pub fn working_minimal_environment(context: &Context) -> Environment {
    let suffix = generate_id();
    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        kind: Kind::Development,
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: ORGANIZATION_ID.to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: generate_id(),
            name: format!("{}-{}", "simple-app".to_string(), &suffix),
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: "Dockerfile".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "v1.d6b3b7db582eab1b85df90df5f558ac5830624f9".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![],
            environment_variables: vec![],
            branch: "basic-app-deploy".to_string(),
            private_port: Some(80),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            total_instances: 2,
        }],
        routers: vec![Router {
            id: generate_id(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: generate_id() + ".oom.sh",
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "simple-app".to_string(), &suffix),
            }],
        }],
        databases: vec![],
        external_services: vec![],
        clone_from_environment_id: None,
    }
}

pub fn working_environment(context: &Context) -> Environment {
    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        kind: Kind::Development,
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: ORGANIZATION_ID.to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: generate_id(),
            name: format!("{}-{}", "simple-app".to_string(), generate_id()),
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
                id: generate_id(),
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
            total_cpus: "1".to_string(),
            total_ram_in_mib: 256,
            total_instances: 2,
        }],
        routers: vec![
            Router {
                id: generate_id(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".oom.sh",
                public_port: 443,
                custom_domains: vec![CustomDomain {
                    domain: generate_id() + "custom.io",
                    target_domain: generate_id() + "toto.oom.sh",
                }],
                routes: vec![Route {
                    path: "/".to_string(),
                    application_name: "simple-example-node-with-postgresql".to_string(),
                }],
            },
            Router {
                id: generate_id(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".oom.sh",
                public_port: 443,
                custom_domains: vec![CustomDomain {
                    domain: generate_id() + "custom.io",
                    target_domain: generate_id() + ".oom.sh",
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
                id: generate_id(),
                name: "my-psql".to_string(),
                version: "11.8.0".to_string(),
                fqdn_id: "my-postgresql-".to_string() + generate_id().as_str(),
                fqdn: "my-postgresql-".to_string() + generate_id().as_str() + ".oom.sh",
                port: 5432,
                username: "superuser".to_string(),
                password: generate_id(),
                total_cpus: "256m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: "db.t2.micro".to_string(),
                database_disk_type: "gp2".to_string(),
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
        external_services: vec![],
        clone_from_environment_id: None,
    }
}

pub fn non_working_environment(context: &Context) -> Environment {
    let mut environment = working_minimal_environment(context);

    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
            app.branch = "bugged-image".to_string();
            app.commit_id = "c2b2d7b5d96832732df25fe992721f53842b5eac".to_string();
            app
        })
        .collect::<Vec<_>>();

    environment
}
