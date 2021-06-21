extern crate serde;
extern crate serde_derive;
use tracing::error;

use chrono::Utc;

use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::kubernetes::{Options, EKS};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::utilities::sanitize_name;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::Engine;
use qovery_engine::models::{
    Action, Application, Context, Database, DatabaseKind, Environment, EnvironmentVariable, GitCredentials, Kind,
    Route, Router, Storage, StorageType,
};

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, generate_id, FuncTestsSecrets};

pub const ORGANIZATION_ID: &str = "u8nb94c7fwxzr2jt";
pub const AWS_REGION_FOR_S3: &str = "us-east-2";
pub const AWS_KUBERNETES_VERSION: &str = "1.17";
pub const KUBE_CLUSTER_ID: &str = "dmubm9agk7sr8a8r";

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}

pub fn container_registry_ecr(context: &Context) -> ECR {
    let secrets = FuncTestsSecrets::new();
    if secrets.AWS_ACCESS_KEY_ID.is_none()
        || secrets.AWS_SECRET_ACCESS_KEY.is_none()
        || secrets.AWS_DEFAULT_REGION.is_none()
    {
        error!("Please check your Vault connectivity (token/address) or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_DEFAULT_REGION envrionment variables are set");
        std::process::exit(1)
    }

    ECR::new(
        context.clone(),
        "default-ecr-registry-Qovery Test",
        "ea59qe62xaw3wjai",
        secrets.AWS_ACCESS_KEY_ID.unwrap().as_str(),
        secrets.AWS_SECRET_ACCESS_KEY.unwrap().as_str(),
        secrets.AWS_DEFAULT_REGION.unwrap().as_str(),
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
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
        Node::new_with_cpu_and_mem(2, 8),
    ]
}

pub fn cloud_provider_aws(context: &Context) -> AWS {
    let secrets = FuncTestsSecrets::new();
    AWS::new(
        context.clone(),
        "u8nb94c7fwxzr2jt",
        ORGANIZATION_ID,
        "QoveryTest",
        secrets.AWS_ACCESS_KEY_ID.unwrap().as_str(),
        secrets.AWS_SECRET_ACCESS_KEY.unwrap().as_str(),
        TerraformStateCredentials {
            access_key_id: secrets.TERRAFORM_AWS_ACCESS_KEY_ID.unwrap(),
            secret_access_key: secrets.TERRAFORM_AWS_SECRET_ACCESS_KEY.unwrap(),
            region: "eu-west-3".to_string(),
        },
    )
}

pub fn eks_options(secrets: FuncTestsSecrets) -> Options {
    Options {
        eks_zone_a_subnet_blocks: vec![
            "10.0.0.0/23".to_string(),
            "10.0.2.0/23".to_string(),
            "10.0.4.0/23".to_string(),
            "10.0.6.0/23".to_string(),
            "10.0.8.0/23".to_string(),
            "10.0.10.0/23".to_string(),
            "10.0.12.0/23".to_string(),
            "10.0.14.0/23".to_string(),
            "10.0.16.0/23".to_string(),
            "10.0.18.0/23".to_string(),
            "10.0.20.0/23".to_string(),
            "10.0.22.0/23".to_string(),
            "10.0.24.0/23".to_string(),
            "10.0.26.0/23".to_string(),
            "10.0.28.0/23".to_string(),
            "10.0.30.0/23".to_string(),
            "10.0.32.0/23".to_string(),
            "10.0.34.0/23".to_string(),
            "10.0.36.0/23".to_string(),
            "10.0.38.0/23".to_string(),
            "10.0.40.0/23".to_string(),
        ],
        eks_zone_b_subnet_blocks: vec![
            "10.0.42.0/23".to_string(),
            "10.0.44.0/23".to_string(),
            "10.0.46.0/23".to_string(),
            "10.0.48.0/23".to_string(),
            "10.0.50.0/23".to_string(),
            "10.0.52.0/23".to_string(),
            "10.0.54.0/23".to_string(),
            "10.0.56.0/23".to_string(),
            "10.0.58.0/23".to_string(),
            "10.0.60.0/23".to_string(),
            "10.0.62.0/23".to_string(),
            "10.0.64.0/23".to_string(),
            "10.0.66.0/23".to_string(),
            "10.0.68.0/23".to_string(),
            "10.0.70.0/23".to_string(),
            "10.0.72.0/23".to_string(),
            "10.0.74.0/23".to_string(),
            "10.0.78.0/23".to_string(),
            "10.0.80.0/23".to_string(),
            "10.0.82.0/23".to_string(),
            "10.0.84.0/23".to_string(),
        ],
        eks_zone_c_subnet_blocks: vec![
            "10.0.86.0/23".to_string(),
            "10.0.88.0/23".to_string(),
            "10.0.90.0/23".to_string(),
            "10.0.92.0/23".to_string(),
            "10.0.94.0/23".to_string(),
            "10.0.96.0/23".to_string(),
            "10.0.98.0/23".to_string(),
            "10.0.100.0/23".to_string(),
            "10.0.102.0/23".to_string(),
            "10.0.104.0/23".to_string(),
            "10.0.106.0/23".to_string(),
            "10.0.108.0/23".to_string(),
            "10.0.110.0/23".to_string(),
            "10.0.112.0/23".to_string(),
            "10.0.114.0/23".to_string(),
            "10.0.116.0/23".to_string(),
            "10.0.118.0/23".to_string(),
            "10.0.120.0/23".to_string(),
            "10.0.122.0/23".to_string(),
            "10.0.124.0/23".to_string(),
            "10.0.126.0/23".to_string(),
        ],
        rds_zone_a_subnet_blocks: vec![
            "10.0.214.0/23".to_string(),
            "10.0.216.0/23".to_string(),
            "10.0.218.0/23".to_string(),
            "10.0.220.0/23".to_string(),
            "10.0.222.0/23".to_string(),
            "10.0.224.0/23".to_string(),
        ],
        rds_zone_b_subnet_blocks: vec![
            "10.0.226.0/23".to_string(),
            "10.0.228.0/23".to_string(),
            "10.0.230.0/23".to_string(),
            "10.0.232.0/23".to_string(),
            "10.0.234.0/23".to_string(),
            "10.0.236.0/23".to_string(),
        ],
        rds_zone_c_subnet_blocks: vec![
            "10.0.238.0/23".to_string(),
            "10.0.240.0/23".to_string(),
            "10.0.242.0/23".to_string(),
            "10.0.244.0/23".to_string(),
            "10.0.246.0/23".to_string(),
            "10.0.248.0/23".to_string(),
        ],
        documentdb_zone_a_subnet_blocks: vec![
            "10.0.196.0/23".to_string(),
            "10.0.198.0/23".to_string(),
            "10.0.200.0/23".to_string(),
        ],
        documentdb_zone_b_subnet_blocks: vec![
            "10.0.202.0/23".to_string(),
            "10.0.204.0/23".to_string(),
            "10.0.206.0/23".to_string(),
        ],
        documentdb_zone_c_subnet_blocks: vec![
            "10.0.208.0/23".to_string(),
            "10.0.210.0/23".to_string(),
            "10.0.212.0/23".to_string(),
        ],
        elasticache_zone_a_subnet_blocks: vec!["10.0.172.0/23".to_string(), "10.0.174.0/23".to_string()],
        elasticache_zone_b_subnet_blocks: vec!["10.0.176.0/23".to_string(), "10.0.178.0/23".to_string()],
        elasticache_zone_c_subnet_blocks: vec!["10.0.180.0/23".to_string(), "10.0.182.0/23".to_string()],
        elasticsearch_zone_a_subnet_blocks: vec!["10.0.184.0/23".to_string(), "10.0.186.0/23".to_string()],
        elasticsearch_zone_b_subnet_blocks: vec!["10.0.188.0/23".to_string(), "10.0.190.0/23".to_string()],
        elasticsearch_zone_c_subnet_blocks: vec!["10.0.192.0/23".to_string(), "10.0.194.0/23".to_string()],
        vpc_cidr_block: "10.0.0.0/16".to_string(),
        eks_cidr_subnet: "23".to_string(),
        eks_access_cidr_blocks: secrets
            .EKS_ACCESS_CIDR_BLOCKS
            .unwrap()
            .replace("\"", "")
            .replace("[", "")
            .replace("]", "")
            .split(",")
            .map(|c| c.to_string())
            .collect(),
        rds_cidr_subnet: "23".to_string(),
        documentdb_cidr_subnet: "23".to_string(),
        elasticache_cidr_subnet: "23".to_string(),
        elasticsearch_cidr_subnet: "23".to_string(),
        qovery_api_url: secrets.QOVERY_API_URL.unwrap(),
        engine_version_controller_token: secrets.QOVERY_ENGINE_CONTROLLER_TOKEN.unwrap(),
        agent_version_controller_token: secrets.QOVERY_AGENT_CONTROLLER_TOKEN.unwrap(),
        grafana_admin_user: "admin".to_string(),
        grafana_admin_password: "qovery".to_string(),
        discord_api_key: secrets.DISCORD_API_URL.unwrap(),
        qovery_nats_url: secrets.QOVERY_NATS_URL.unwrap(),
        qovery_ssh_key: secrets.QOVERY_SSH_USER.unwrap(),
        qovery_nats_user: secrets.QOVERY_NATS_USERNAME.unwrap(),
        qovery_nats_password: secrets.QOVERY_NATS_PASSWORD.unwrap(),
        tls_email_report: secrets.LETS_ENCRYPT_EMAIL_REPORT.unwrap(),
    }
}

pub fn aws_kubernetes_eks<'a>(
    context: &Context,
    cloud_provider: &'a AWS,
    dns_provider: &'a dyn DnsProvider,
    nodes: Vec<Node>,
) -> EKS<'a> {
    let secrets = FuncTestsSecrets::new();
    EKS::<'a>::new(
        context.clone(),
        KUBE_CLUSTER_ID,
        KUBE_CLUSTER_ID,
        AWS_KUBERNETES_VERSION,
        secrets.clone().AWS_DEFAULT_REGION.unwrap().as_str(),
        cloud_provider,
        dns_provider,
        eks_options(secrets),
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

pub fn environment_3_apps_3_routers_3_databases(context: &Context, secrets: FuncTestsSecrets) -> Environment {
    let app_name_1 = format!("{}-{}", "simple-app-1".to_string(), generate_id());
    let app_name_2 = format!("{}-{}", "simple-app-2".to_string(), generate_id());
    let app_name_3 = format!("{}-{}", "simple-app-3".to_string(), generate_id());
    let test_domain = secrets.DEFAULT_TEST_DOMAIN.unwrap();

    // mongoDB management part
    let database_host_mongo = format!("mongodb-{}.{}", generate_id(), &test_domain);
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
    let fqdn = format!("{}.{}", fqdn_id, &test_domain);
    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let database_name = "postgresql".to_string();

    // pSQL 2 management part
    let fqdn_id_2 = "my-postgresql-2".to_string() + generate_id().as_str();
    let fqdn_2 = format!("{}.{}", fqdn_id_2, &test_domain);
    let database_username_2 = "superuser2".to_string();
    let database_name_2 = "postgresql2".to_string();

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
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: "/".to_string(),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
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
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
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
                dockerfile_path: Some(format!("Dockerfile-{}", version_mongo)),
                action: Action::Create,
                root_path: String::from("/"),
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
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
                default_domain: generate_id() + &test_domain,
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
                default_domain: generate_id() + &test_domain,
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
                default_domain: generate_id() + &test_domain,
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

pub fn working_minimal_environment(context: &Context, secrets: FuncTestsSecrets) -> Environment {
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
            dockerfile_path: Some("Dockerfile".to_string()),
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
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
            default_domain: generate_id() + secrets.DEFAULT_TEST_DOMAIN.unwrap().as_ref(),
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

pub fn environnement_2_app_2_routers_1_psql(context: &Context, secrets: FuncTestsSecrets) -> Environment {
    let fqdn_id = "my-postgresql-".to_string() + generate_id().as_str();
    let test_domain = secrets.DEFAULT_TEST_DOMAIN.unwrap();
    let fqdn = format!("{}.{}", fqdn_id.clone(), &test_domain);

    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let database_name = "postgresql".to_string();

    let suffix = generate_id();
    let application_name1 = sanitize_name("postgresql", &format!("{}-{}", "postgresql-app1", &suffix));
    let application_name2 = sanitize_name("postgresql", &format!("{}-{}", "postgresql-app2", &suffix));

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
                name: application_name1.to_string(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
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
                name: application_name2.to_string(),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
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
                default_domain: format!("{}.{}", generate_id(), &test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/".to_string(),
                    application_name: application_name1.to_string(),
                }],
            },
            Router {
                id: generate_id(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}", generate_id(), &test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/coco".to_string(),
                    application_name: application_name2.to_string(),
                }],
            },
        ],

        external_services: vec![],
        clone_from_environment_id: None,
    }
}

pub fn non_working_environment(context: &Context, secrets: FuncTestsSecrets) -> Environment {
    let mut environment = working_minimal_environment(context, secrets);

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
pub fn echo_app_environment(context: &Context, secrets: FuncTestsSecrets) -> Environment {
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
            dockerfile_path: Some("Dockerfile".to_string()),
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
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
            default_domain: generate_id() + secrets.DEFAULT_TEST_DOMAIN.unwrap().as_str(),
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
            dockerfile_path: Some("Dockerfile".to_string()),
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
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

pub fn environment_only_http_server_router(context: &Context, secrets: FuncTestsSecrets) -> Environment {
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
            dockerfile_path: Some("Dockerfile".to_string()),
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
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
        routers: vec![Router {
            id: generate_id(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: generate_id() + secrets.DEFAULT_TEST_DOMAIN.unwrap().as_str(),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                application_name: format!("{}-{}", "mini-http".to_string(), &suffix),
            }],
        }],
        databases: vec![],
        external_services: vec![],
        clone_from_environment_id: None,
    }
}
