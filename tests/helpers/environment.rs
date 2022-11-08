use crate::helpers::utilities::{generate_id, generate_password, get_svc_name};
use chrono::Utc;
use qovery_engine::cloud_provider::utilities::sanitize_name;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::application::{Application, GitCredentials, Port, Protocol};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::database::DatabaseMode::CONTAINER;
use qovery_engine::io_models::database::{Database, DatabaseKind};
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::router::{Route, Router};
use qovery_engine::io_models::Action;
use qovery_engine::utilities::to_short_id;
use std::collections::BTreeMap;
use url::Url;
use uuid::Uuid;

pub fn working_environment(
    context: &Context,
    test_domain: &str,
    with_router: bool,
    with_sticky: bool,
) -> EnvironmentRequest {
    let application_id = Uuid::new_v4();
    let application_name = to_short_id(&application_id);
    let router_name = "main".to_string();
    let application_domain = format!("{}.{}.{}", application_name, context.cluster_short_id(), test_domain);
    let mut req = EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: application_id,
        name: "env".to_string(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: application_id,
            name: application_name,
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "basic-app-deploy".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 80,
                is_default: true,
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![],
    };

    if with_router {
        req.routers = vec![Router {
            long_id: Uuid::new_v4(),
            name: router_name,
            action: Action::Create,
            default_domain: application_domain,
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: application_id,
            }],
            sticky_sessions_enabled: with_sticky,
        }]
    }

    req
}
pub fn working_minimal_environment(context: &Context) -> EnvironmentRequest {
    working_environment(context, "", false, false)
}

pub fn working_minimal_environment_with_router(context: &Context, test_domain: &str) -> EnvironmentRequest {
    working_environment(context, test_domain, true, false)
}

pub fn environment_2_app_2_routers_1_psql(
    context: &Context,
    test_domain: &str,
    database_instance_type: &str,
    database_disk_type: &str,
    provider_kind: Kind,
) -> EnvironmentRequest {
    let fqdn = get_svc_name(DatabaseKind::Postgresql, provider_kind).to_string();

    let database_port = 5432;
    let database_username = "superuser".to_string();
    let database_password = generate_password(CONTAINER);
    let database_name = "postgres".to_string();

    let suffix = generate_id();
    let application_id1 = Uuid::new_v4();
    let application_id2 = Uuid::new_v4();

    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        name: "env".to_string(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        databases: vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            long_id: Uuid::new_v4(),
            name: database_name.clone(),
            created_at: Utc::now(),
            version: "11.8.0".to_string(),
            fqdn_id: fqdn.clone(),
            fqdn: fqdn.clone(),
            port: database_port,
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: database_instance_type.to_string(),
            database_disk_type: database_disk_type.to_string(),
            encrypt_disk: true,
            activate_high_availability: false,
            activate_backups: false,
            publicly_accessible: false,
            mode: CONTAINER,
        }],
        applications: vec![
            Application {
                long_id: application_id1,
                name: sanitize_name("postgresql", &format!("{}-{}", "postgresql-app1", &suffix)),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                buildpack_language: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![],
                environment_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_name.clone()),
                     "PG_HOST".to_string() => base64::encode(fqdn.clone()),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username.clone()),
                     "PG_PASSWORD".to_string() => base64::encode(database_password.clone()),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    is_default: true,
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
            Application {
                long_id: application_id2,
                name: sanitize_name("postgresql", &format!("{}-{}", "postgresql-app2", &suffix)),
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "680550d1937b3f90551849c0da8f77c39916913b".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                buildpack_language: None,
                root_path: String::from("/"),
                action: Action::Create,
                git_credentials: Some(GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "xxx".to_string(),
                    expired_at: Utc::now(),
                }),
                storage: vec![],
                environment_vars: btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_name),
                     "PG_HOST".to_string() => base64::encode(fqdn),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username),
                     "PG_PASSWORD".to_string() => base64::encode(database_password),
                },
                branch: "master".to_string(),
                ports: vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    is_default: true,
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }],
                total_cpus: "100m".to_string(),
                total_ram_in_mib: 256,
                min_instances: 1,
                max_instances: 1,
                cpu_burst: "100m".to_string(),
                advanced_settings: Default::default(),
            },
        ],
        containers: vec![],
        jobs: vec![],
        routers: vec![
            Router {
                long_id: Uuid::new_v4(),
                name: "main".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_short_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/".to_string(),
                    service_long_id: application_id1,
                }],
                sticky_sessions_enabled: false,
            },
            Router {
                long_id: Uuid::new_v4(),
                name: "second-router".to_string(),
                action: Action::Create,
                default_domain: format!("{}.{}.{}", generate_id(), context.cluster_short_id(), test_domain),
                public_port: 443,
                custom_domains: vec![],
                routes: vec![Route {
                    path: "/coco".to_string(),
                    service_long_id: application_id2,
                }],
                sticky_sessions_enabled: false,
            },
        ],
    }
}

pub fn non_working_environment(context: &Context) -> EnvironmentRequest {
    let mut environment = working_environment(context, "", false, false);
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
pub fn echo_app_environment(context: &Context, test_domain: &str) -> EnvironmentRequest {
    let suffix = generate_id();
    let application_id = Uuid::new_v4();
    EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: application_id,
        name: "env".to_string(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: Uuid::new_v4(),
            name: format!("{}-{}", "echo-app", &suffix),
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "2205adea1db295547b99f7b17229afd7e879b6ff".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: btreemap! {
                "ECHO_TEXT".to_string() => base64::encode("42"),
            },
            branch: "echo-app".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 5678,
                is_default: true,
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        containers: vec![],
        jobs: vec![],
        routers: vec![Router {
            long_id: Uuid::new_v4(),
            name: "main".to_string(),
            action: Action::Create,
            default_domain: format!("{}.{}.{}", generate_id(), context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: application_id,
            }],
            sticky_sessions_enabled: false,
        }],
        databases: vec![],
    }
}

pub fn environment_only_http_server(
    context: &Context,
    test_domain: &str,
    with_router: bool,
    with_sticky: bool,
) -> EnvironmentRequest {
    let router_name = "main".to_string();
    let suffix = generate_id();
    let application_id = Uuid::new_v4();
    let application_name = format!("{}-{}", "mini-http", &suffix);
    let application_domain = format!("{}.{}.{}", application_name, context.cluster_short_id(), test_domain);

    let mut req = EnvironmentRequest {
        execution_id: context.execution_id().to_string(),
        long_id: Uuid::new_v4(),
        name: "env".to_string(),
        project_long_id: Uuid::new_v4(),
        organization_long_id: Uuid::new_v4(),
        action: Action::Create,
        applications: vec![Application {
            long_id: application_id,
            name: application_name,
            /*name: "simple-app".to_string(),*/
            git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
            commit_id: "a873edd459c97beb51453db056c40bca85f36ef9".to_string(),
            dockerfile_path: Some("Dockerfile".to_string()),
            buildpack_language: None,
            root_path: String::from("/"),
            action: Action::Create,
            git_credentials: Some(GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "xxx".to_string(),
                expired_at: Utc::now(),
            }),
            storage: vec![],
            environment_vars: BTreeMap::default(),
            branch: "mini-http".to_string(),
            ports: vec![Port {
                id: "zdf7d6aad".to_string(),
                long_id: Default::default(),
                port: 3000,
                is_default: true,
                name: None,
                publicly_accessible: true,
                protocol: Protocol::HTTP,
            }],
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 256,
            min_instances: 1,
            max_instances: 1,
            cpu_burst: "100m".to_string(),
            advanced_settings: Default::default(),
        }],
        containers: vec![],
        jobs: vec![],
        routers: vec![],
        databases: vec![],
    };

    if with_router {
        req.routers = vec![Router {
            long_id: Uuid::new_v4(),
            name: router_name,
            action: Action::Create,
            default_domain: application_domain,
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: application_id,
            }],
            sticky_sessions_enabled: with_sticky,
        }]
    }

    req
}

pub fn environment_only_http_server_router(context: &Context, test_domain: &str) -> EnvironmentRequest {
    environment_only_http_server(context, test_domain, true, false)
}

pub fn environment_only_http_server_router_with_sticky_session(
    context: &Context,
    test_domain: &str,
) -> EnvironmentRequest {
    environment_only_http_server(context, test_domain, true, true)
}

/// Test if stick sessions are activated on given routers via cookie.
pub fn session_is_sticky(url: Url, host: String, max_age: u32) -> bool {
    let mut is_ok = true;
    let http_client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true) // this test ignores certificate validity (not its purpose)
        .build()
        .expect("Cannot build reqwest client");

    let http_request_result = http_client.get(url.to_string()).header("Host", host.as_str()).send();

    if http_request_result.is_err() {
        return false;
    }

    let http_response = http_request_result.expect("cannot retrieve HTTP request result");

    is_ok &= match http_response.headers().get("Set-Cookie") {
        None => false,
        Some(value) => match value.to_str() {
            Err(_) => false,
            Ok(s) => s.contains("INGRESSCOOKIE_QOVERY=") && s.contains(format!("Max-Age={}", max_age).as_str()),
        },
    };

    is_ok
}
