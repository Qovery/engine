extern crate serde;
extern crate serde_derive;

use std::fs::File;

use chrono::Utc;
use dirs::home_dir;

use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::Engine;
use qovery_engine::models::{
    Action, Application, Context, Database, DatabaseKind, Environment, EnvironmentVariable,
    GitCredentials, Kind, Metadata, Route, Router, Storage, StorageType,
};

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, generate_id};

pub const ORGANIZATION_ID: &str = "u8nb94c7fwxzr2jt";
pub const AWS_REGION_FOR_S3: &str = "us-east-1";
pub const AWS_KUBERNETES_VERSION: &str = "1.16";
pub const KUBE_CLUSTER_ID: &str = "dmubm9agk7sr8a8r";

pub fn aws_access_key_id() -> String {
    std::env::var("AWS_ACCESS_KEY_ID").expect("env var AWS_ACCESS_KEY_ID is mandatory")
}

pub fn aws_secret_access_key() -> String {
    std::env::var("AWS_SECRET_ACCESS_KEY").expect("env var AWS_SECRET_ACCESS_KEY is mandatory")
}

pub fn aws_default_region() -> String {
    std::env::var("AWS_DEFAULT_REGION").expect("env var AWS_DEFAULT_REGION is mandatory")
}

pub fn terraform_aws_access_key_id() -> String {
    std::env::var("TERRAFORM_AWS_ACCESS_KEY_ID")
        .expect("env var TERRAFORM_AWS_ACCESS_KEY_ID is mandatory")
}

pub fn terraform_aws_secret_access_key() -> String {
    std::env::var("TERRAFORM_AWS_SECRET_ACCESS_KEY")
        .expect("env var TERRAFORM_AWS_SECRET_ACCESS_KEY is mandatory")
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
    let metadata = Metadata {
        test: Option::from(true),
        dry_run_deploy: Option::from(false),
        resource_expiration_in_seconds: Some(2700),
    };

    Context::new(
        execution_id.as_str(),
        home_dir.as_str(),
        lib_root_dir.as_str(),
        None,
        Option::from(metadata),
    )
}

pub fn container_registry_ecr(context: &Context) -> ECR {
    ECR::new(
        context.clone(),
        "default-ecr-registry-Qovery Test",
        "ea59qe62xaw3wjai",
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        aws_default_region().as_str(),
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
        Node::new_with_cpu_and_mem(2, 16),
        Node::new_with_cpu_and_mem(2, 16),
        Node::new_with_cpu_and_mem(2, 16),
        Node::new_with_cpu_and_mem(2, 16),
    ]
}

pub fn cloud_provider_aws(context: &Context) -> AWS {
    AWS::new(
        context.clone(),
        "u8nb94c7fwxzr2jt",
        ORGANIZATION_ID,
        "QoveryTest",
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        TerraformStateCredentials {
            access_key_id: terraform_aws_access_key_id().to_string(),
            secret_access_key: terraform_aws_secret_access_key().to_string(),
            region: "eu-west-3".to_string(),
        },
    )
}

pub fn aws_kubernetes_eks<'a>(
    context: &Context,
    cloud_provider: &'a AWS,
    dns_provider: &'a dyn DnsProvider,
    nodes: Vec<Node>,
) -> EKS<'a> {
    let file = File::open("tests/assets/eks-options.json").expect("file not found");
    let options_values = serde_json::from_reader(file).expect("JSON was not well-formatted");
    EKS::<'a>::new(
        context.clone(),
        KUBE_CLUSTER_ID,
        KUBE_CLUSTER_ID,
        AWS_KUBERNETES_VERSION,
        aws_default_region().as_str(),
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

pub fn environment_3_apps_3_routers_3_databases(context: &Context) -> Environment {
    let app_name_1 = format!("{}-{}", "simple-app-1".to_string(), generate_id());
    let app_name_2 = format!("{}-{}", "simple-app-2".to_string(), generate_id());
    let app_name_3 = format!("{}-{}", "simple-app-3".to_string(), generate_id());

    // mongoDB management part
    let database_host_mongo =
        "mongodb-".to_string() + generate_id().as_str() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN"; // External access check
    let database_port_mongo = 27017;
    let database_db_name_mongo = "my-mongodb".to_string();
    let database_username_mongo = "superuser".to_string();
    let database_password_mongo = generate_id();
    let database_uri_mongo = format!(
        "mongodb://{}:{}@{}:{}/{}",
        database_username_mongo,
        database_password_mongo,
        database_host_mongo,
        database_port_mongo,
        database_db_name_mongo
    );
    let version_mongo = "4.4";

    // pSQL 1 management part
    let fqdn_id = "my-postgresql-".to_string() + generate_id().as_str();
    let fqdn = fqdn_id.clone() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN";
    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let database_name = "my-psql".to_string();

    // pSQL 2 management part
    let fqdn_id_2 = "my-postgresql-2".to_string() + generate_id().as_str();
    let fqdn_2 = fqdn_id_2.clone() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN";
    let database_username_2 = "superuser2".to_string();
    let database_name_2 = "my-psql-2".to_string();

    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        kind: Kind::Development,
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: ORGANIZATION_ID.to_string(),
        action: Action::Create,
        applications: vec![
            Application {
                id: generate_id(),
                name: app_name_1.clone(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "5990752647af11ef21c3d46a51abbde3da1ab351".to_string(),
                dockerfile_path: "Dockerfile".to_string(),
                action: Action::Create,
                git_credentials: GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
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
                        key: "PG_DBNAME".to_string(),
                        value: database_name.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_HOST".to_string(),
                        value: fqdn.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "PG_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ],
                branch: "master".to_string(),
                private_port: Some(1234),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                total_instances: 2,
                cpu_burst: "100m".to_string(),
                start_timeout_in_seconds: 60,
            },
            Application {
                id: generate_id(),
                name: app_name_2.clone(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "5990752647af11ef21c3d46a51abbde3da1ab351".to_string(),
                dockerfile_path: "Dockerfile".to_string(),
                action: Action::Create,
                git_credentials: GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
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
                        key: "PG_DBNAME".to_string(),
                        value: database_name_2.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_HOST".to_string(),
                        value: fqdn_2.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "PG_USERNAME".to_string(),
                        value: database_username_2.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ],
                branch: "master".to_string(),
                private_port: Some(1234),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                total_instances: 2,
                cpu_burst: "100m".to_string(),
                start_timeout_in_seconds: 60,
            },
            Application {
                id: generate_id(),
                name: app_name_3.clone(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "158ea8ebc9897c50a7c56b910db33ce837ac1e61".to_string(),
                dockerfile_path: format!("Dockerfile-{}", version_mongo),
                action: Action::Create,
                git_credentials: GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
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
                        key: "IS_DOCUMENTDB".to_string(),
                        value: "false".to_string(),
                    },
                    EnvironmentVariable {
                        key: "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string(),
                        value: database_host_mongo.clone(),
                    },
                    EnvironmentVariable {
                        key: "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string(),
                        value: database_uri_mongo.clone(),
                    },
                    EnvironmentVariable {
                        key: "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string(),
                        value: database_port_mongo.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "MONGODB_DBNAME".to_string(),
                        value: database_db_name_mongo.clone(),
                    },
                    EnvironmentVariable {
                        key: "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string(),
                        value: database_username_mongo.clone(),
                    },
                    EnvironmentVariable {
                        key: "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string(),
                        value: database_password_mongo.clone(),
                    },
                ],
                branch: "master".to_string(),
                private_port: Some(1234),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                total_instances: 2,
                cpu_burst: "100m".to_string(),
                start_timeout_in_seconds: 60,
            },
        ],
        routers: vec![
            Router {
                id: generate_id(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app1".to_string(),
                    application_name: app_name_1.clone(),
                }],
            },
            Router {
                id: generate_id(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app2".to_string(),
                    application_name: app_name_2.clone(),
                }],
            },
            Router {
                id: generate_id(),
                name: "third-router".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/app3".to_string(),
                    application_name: app_name_3.clone(),
                }],
            },
        ],
        databases: vec![
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                id: generate_id(),
                name: database_name.clone(),
                version: "11.8.0".to_string(),
                fqdn_id: fqdn_id.clone(),
                fqdn: fqdn.clone(),
                port: database_port.clone(),
                username: database_username.clone(),
                password: database_password.clone(),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: "db.t2.micro".to_string(),
                database_disk_type: "gp2".to_string(),
            },
            Database {
                kind: DatabaseKind::Postgresql,
                action: Action::Create,
                id: generate_id(),
                name: database_name_2.clone(),
                version: "11.8.0".to_string(),
                fqdn_id: fqdn_id_2.clone(),
                fqdn: fqdn_2.clone(),
                port: database_port.clone(),
                username: database_username_2.clone(),
                password: database_password.clone(),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: "db.t2.micro".to_string(),
                database_disk_type: "gp2".to_string(),
            },
            Database {
                kind: DatabaseKind::Mongodb,
                action: Action::Create,
                id: generate_id(),
                name: database_db_name_mongo.clone(),
                version: version_mongo.to_string(),
                fqdn_id: "mongodb-".to_string() + generate_id().as_str(),
                fqdn: database_host_mongo.clone(),
                port: database_port_mongo.clone(),
                username: database_username_mongo.clone(),
                password: database_password_mongo.clone(),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 512,
                disk_size_in_gib: 10,
                database_instance_type: "db.t3.medium".to_string(),
                database_disk_type: "gp2".to_string(),
            },
        ],
        external_services: vec![],
        clone_from_environment_id: None,
    }
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
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: "Dockerfile".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![],
            environment_variables: vec![],
            branch: "basic-app-deploy".to_string(),
            private_port: Some(80),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            total_instances: 2,
            cpu_burst: "100m".to_string(),
            start_timeout_in_seconds: 60,
        }],
        routers: vec![Router {
            id: generate_id(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
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

pub fn environnement_2_app_2_routers_1_psql(context: &Context) -> Environment {
    let fqdn_id = "my-postgresql-".to_string() + generate_id().as_str();
    let fqdn = fqdn_id.clone() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN";

    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let database_name = "my-psql".to_string();

    let suffix = generate_id();

    Environment {
        execution_id: context.execution_id().to_string(),
        id: generate_id(),
        kind: Kind::Development,
        owner_id: generate_id(),
        project_id: generate_id(),
        organization_id: ORGANIZATION_ID.to_string(),
        action: Action::Create,
        databases: vec![Database {
            kind: DatabaseKind::Postgresql,

            action: Action::Create,
            id: generate_id(),
            name: database_name.clone(),
            version: "11.8.0".to_string(),
            fqdn_id: fqdn_id.clone(),
            fqdn: fqdn.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: "db.t2.micro".to_string(),
            database_disk_type: "gp2".to_string(),
        }],
        applications: vec![
            Application {
                id: generate_id(),
                name: format!("{}-{}", "simple-app".to_string(), &suffix),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: "Dockerfile".to_string(),
                action: Action::Create,
                git_credentials: GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
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
                        key: "PG_DBNAME".to_string(),
                        value: database_name.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_HOST".to_string(),
                        value: fqdn.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "PG_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ],
                branch: "master".to_string(),
                private_port: Some(1234),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                total_instances: 2,
                cpu_burst: "100m".to_string(),
                start_timeout_in_seconds: 60,
            },
            Application {
                id: generate_id(),
                name: format!("{}-{}", "simple-app-2".to_string(), &suffix),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: "Dockerfile".to_string(),
                action: Action::Create,
                git_credentials: GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
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
                        key: "PG_DBNAME".to_string(),
                        value: database_name.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_HOST".to_string(),
                        value: fqdn.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "PG_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ],
                branch: "master".to_string(),
                private_port: Some(1234),
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                total_instances: 2,
                cpu_burst: "100m".to_string(),
                start_timeout_in_seconds: 60,
            },
        ],
        routers: vec![
            Router {
                id: generate_id(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/".to_string(),
                    application_name: format!("{}-{}", "simple-app".to_string(), &suffix),
                }],
            },
            Router {
                id: generate_id(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/coco".to_string(),
                    application_name: format!("{}-{}", "simple-app-2".to_string(), &suffix),
                }],
            },
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

// echo app environment is an environment that contains http-echo container (forked from hashicorp)
// ECHO_TEXT var will be the content of the application root path
pub fn echo_app_environment(context: &Context) -> Environment {
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
            name: format!("{}-{}", "echo-app".to_string(), &suffix),
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "2205adea1db295547b99f7b17229afd7e879b6ff".to_string(),
            dockerfile_path: "Dockerfile".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "CHANGE-ME/GITHUB_ACCESS_TOKEN".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![],
            environment_variables: vec![EnvironmentVariable {
                key: "ECHO_TEXT".to_string(),
                value: "42".to_string(),
            }],
            branch: "echo-app".to_string(),
            private_port: Some(5678),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            total_instances: 2,
            cpu_burst: "100m".to_string(),
            start_timeout_in_seconds: 60,
        }],
        routers: vec![Router {
            id: generate_id(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: generate_id() + ".CHANGE-ME/DEFAULT_TEST_DOMAIN",
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "echo-app".to_string(), &suffix),
            }],
        }],
        databases: vec![],
        external_services: vec![],
        clone_from_environment_id: None,
    }
}

pub fn environment_only_http_server(context: &Context) -> Environment {
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
            name: format!("{}-{}", "mini-http".to_string(), &suffix),
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "a873edd459c97beb51453db056c40bca85f36ef9".to_string(),
            dockerfile_path: "Dockerfile".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![],
            environment_variables: vec![],
            branch: "mini-http".to_string(),
            private_port: Some(3000),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            total_instances: 2,
            cpu_burst: "100m".to_string(),
            start_timeout_in_seconds: 60,
        }],
        routers: vec![],
        databases: vec![],
        external_services: vec![],
        clone_from_environment_id: None,
    }
}

