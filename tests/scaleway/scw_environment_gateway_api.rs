use crate::helpers;
use crate::helpers::common::Infrastructure;
use crate::helpers::scaleway::scw_infra_config;
use ::function_name::named;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::api::ListParams;
use kube::{Api, ResourceExt};
use qovery_engine::io_models::application::PortIo;

use crate::helpers::kubernetes::TargetCluster;
use crate::helpers::utilities::{FuncTestsSecrets, context_for_resource, engine_run_test, logger, metrics_registry};
use qovery_engine::io_models::application::Protocol::HTTP;
use qovery_engine::io_models::container::{Container, ContainerAdvancedSettings, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::helm_chart::{
    HelmChart, HelmChartAdvancedSettings, HelmChartSource, HelmRawValues, HelmValueSource,
};
use qovery_engine::io_models::router::{CustomDomain, Route, Router};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, QoveryIdentifier};
use qovery_engine::runtime::block_on;
use retry::delay::Fibonacci;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tracing::{Level, span};
use url::Url;
use uuid::Uuid;

/// Helper function to retry listing Gateway API resources with exponential backoff
/// This helps avoid flaky tests due to eventual consistency in Kubernetes
fn retry_list_gateway_api_resources(
    api: &Api<kube::core::DynamicObject>,
) -> Result<kube::api::ObjectList<kube::core::DynamicObject>, String> {
    retry::retry(Fibonacci::from_millis(3000).take(10), || {
        match block_on(async { api.list(&ListParams::default()).await }) {
            Ok(resources) => retry::OperationResult::Ok(resources),
            Err(e) => {
                tracing::warn!("Failed to list Gateway API resources, retrying: {}", e);
                retry::OperationResult::Retry("Failed to list Gateway API resources")
            }
        }
    })
    .map_err(|e| format!("Failed to list Gateway API resources after retries: {e:?}"))
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_cors_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_enable_cors = true;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_cors_allow_origin = "https://example.com,https://test.com".to_string();
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_cors_allow_methods = "GET,POST,PUT,DELETE".to_string();
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_cors_allow_headers = "Content-Type,Authorization,X-Custom-Header".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "cors-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(cors) = spec.get("cors") {
                let allow_origins = cors
                    .get("allowOrigins")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_origins.contains(&"https://example.com"));
                assert!(allow_origins.contains(&"https://test.com"));

                let allow_methods = cors
                    .get("allowMethods")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_methods.contains(&"GET"));
                assert!(allow_methods.contains(&"POST"));
                assert!(allow_methods.contains(&"PUT"));
                assert!(allow_methods.contains(&"DELETE"));

                let allow_headers = cors
                    .get("allowHeaders")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_headers.contains(&"Content-Type"));
                assert!(allow_headers.contains(&"Authorization"));
                assert!(allow_headers.contains(&"X-Custom-Header"));
            } else {
                panic!("SecurityPolicy should have CORS configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_sticky_session_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_sticky_session_enable = true;

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "sticky-session-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(load_balancer) = spec.get("loadBalancer") {
                let lb_type = load_balancer.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(lb_type, "ConsistentHash", "Load balancer type should be ConsistentHash");

                if let Some(consistent_hash) = load_balancer.get("consistentHash") {
                    let hash_type = consistent_hash.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(hash_type, "Cookie", "ConsistentHash type should be Cookie");

                    if let Some(cookie) = consistent_hash.get("cookie") {
                        let cookie_name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(
                            cookie_name, "INGRESSCOOKIE_QOVERY",
                            "Cookie name should be INGRESSCOOKIE_QOVERY"
                        );

                        let ttl = cookie.get("ttl").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(ttl, "85400s", "TTL should be 85400s (1 day)");
                    } else {
                        panic!("ConsistentHash should have cookie configuration");
                    }
                } else {
                    panic!("LoadBalancer should have consistentHash configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have loadBalancer configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_ip_whitelist_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_whitelist_source_range = "10.0.0.0/8,192.168.1.0/24,172.16.0.0/12".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "ip-whitelist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Deny", "Default action should be Deny");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Allow", "Rule action should be Allow");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.1.0/24"), "Should contain 192.168.1.0/24");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Should contain 172.16.0.0/12");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_ip_denylist_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_denylist_source_range = "192.168.0.0/16,10.10.10.0/24".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "ip-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // When only denylist is set, defaultAction should be Allow
                assert_eq!(
                    default_action, "Allow",
                    "Default action should be Allow when only denylist is configured"
                );

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Deny", "Rule action should be Deny");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");
                            assert!(cidrs.contains(&"10.10.10.0/24"), "Should contain 10.10.10.0/24");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_both_whitelist_and_denylist_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_whitelist_source_range = "10.0.0.0/8,172.16.0.0/12".to_string();
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_denylist_source_range = "10.10.10.0/24".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // When both whitelist and denylist are set, defaultAction should be Deny (whitelist takes precedence)
                assert_eq!(
                    default_action, "Deny",
                    "Default action should be Deny when both whitelist and denylist are configured"
                );

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert_eq!(rules.len(), 2, "Should have 2 authorization rules");

                    // First rule should be Allow (whitelist)
                    let allow_rule = &rules[0];
                    let allow_action = allow_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(allow_action, "Allow", "First rule action should be Allow");

                    if let Some(principal) = allow_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Whitelist should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Whitelist should contain 172.16.0.0/12");
                        } else {
                            panic!("Allow rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Allow rule should have principal");
                    }

                    // Second rule should be Deny (denylist)
                    let deny_rule = &rules[1];
                    let deny_action = deny_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(deny_action, "Deny", "Second rule action should be Deny");

                    if let Some(principal) = deny_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.10.10.0/24"), "Denylist should contain 10.10.10.0/24");
                        } else {
                            panic!("Deny rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Deny rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_basic_auth_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_basic_auth_env_var = "HTPASSWD_CONTENT".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        let router_kube_name = format!("router-{suffix}");
        environment.routers = vec![Router {
            long_id: router_id,
            name: "basic-auth-test-router".to_string(),
            kube_name: router_kube_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(basic_auth) = spec.get("basicAuth") {
                if let Some(users) = basic_auth.get("users") {
                    let secret_name = users.get("name").and_then(|v| v.as_str()).unwrap_or("");

                    let expected_secret_name = format!("htaccess-{router_kube_name}",);
                    assert_eq!(
                        secret_name, expected_secret_name,
                        "BasicAuth secret name should match htaccess-{{router_kube_name}}"
                    );
                } else {
                    panic!("BasicAuth should have users configuration");
                }
            } else {
                panic!("SecurityPolicy should have basicAuth configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_rate_limit_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_rpm = Some(1000);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_rps = Some(100);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_source_cidrs = "10.0.0.0/8,192.168.0.0/16".to_string();
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_headers = "X-API-Key,X-User-ID".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "rate-limit-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(rate_limit) = spec.get("rateLimit") {
                let rate_limit_type = rate_limit.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(rate_limit_type, "Local", "Rate limit type should be Local");

                if let Some(local) = rate_limit.get("local") {
                    if let Some(rules) = local.get("rules").and_then(|v| v.as_array()) {
                        assert_eq!(rules.len(), 2, "Should have 2 rate limit rules (RPM and RPS)");

                        // Verify RPM rule
                        let rpm_rule = &rules[0];
                        if let Some(limit) = rpm_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 1000, "RPM limit should be 1000");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Minute", "RPM unit should be Minute");
                        } else {
                            panic!("RPM rule should have limit");
                        }

                        // Verify RPM clientSelectors
                        if let Some(client_selectors) = rpm_rule.get("clientSelectors").and_then(|v| v.as_array()) {
                            // Check for source CIDR selectors
                            let source_cidr_selectors: Vec<_> = client_selectors
                                .iter()
                                .filter(|s| s.get("sourceCIDR").is_some())
                                .collect();
                            assert_eq!(source_cidr_selectors.len(), 2, "Should have 2 source CIDR selectors");

                            let cidrs: Vec<&str> = source_cidr_selectors
                                .iter()
                                .filter_map(|s| s.get("sourceCIDR")?.get("value")?.as_str())
                                .collect();
                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");

                            // Check for header selectors
                            let header_selectors: Vec<_> =
                                client_selectors.iter().filter(|s| s.get("headers").is_some()).collect();
                            assert_eq!(header_selectors.len(), 1, "Should have 1 header selector group");

                            if let Some(headers) = header_selectors[0].get("headers").and_then(|v| v.as_array()) {
                                assert_eq!(headers.len(), 2, "Should have 2 headers");
                                let header_names: Vec<&str> =
                                    headers.iter().filter_map(|h| h.get("name")?.as_str()).collect();
                                assert!(header_names.contains(&"X-API-Key"), "Should contain X-API-Key header");
                                assert!(header_names.contains(&"X-User-ID"), "Should contain X-User-ID header");
                            } else {
                                panic!("Header selector should have headers array");
                            }
                        } else {
                            panic!("RPM rule should have clientSelectors");
                        }

                        // Verify RPS rule
                        let rps_rule = &rules[1];
                        if let Some(limit) = rps_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 100, "RPS limit should be 100");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Second", "RPS unit should be Second");
                        } else {
                            panic!("RPS rule should have limit");
                        }

                        // Verify RPS clientSelectors (should be same as RPM)
                        if let Some(client_selectors) = rps_rule.get("clientSelectors").and_then(|v| v.as_array()) {
                            let source_cidr_selectors: Vec<_> = client_selectors
                                .iter()
                                .filter(|s| s.get("sourceCIDR").is_some())
                                .collect();
                            assert_eq!(source_cidr_selectors.len(), 2, "RPS should have 2 source CIDR selectors");
                        } else {
                            panic!("RPS rule should have clientSelectors");
                        }
                    } else {
                        panic!("Local rate limit should have rules");
                    }
                } else {
                    panic!("Rate limit should have local configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have rateLimit configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_custom_headers_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;

        // Add response headers
        let mut add_headers = BTreeMap::new();
        add_headers.insert("X-Custom-Response-Header".to_string(), "response-value".to_string());
        add_headers.insert("X-Frame-Options".to_string(), "DENY".to_string());
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_add_headers = add_headers;

        // Add request headers
        let mut proxy_set_headers = BTreeMap::new();
        proxy_set_headers.insert("X-Forwarded-Proto".to_string(), "https".to_string());
        proxy_set_headers.insert("X-Real-IP".to_string(), "$remote_addr".to_string());
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_proxy_set_headers = proxy_set_headers;

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        let router_kube_name = format!("router-{suffix}");
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-headers-test-router".to_string(),
            kube_name: router_kube_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let http_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!http_routes.items.is_empty());

        let router_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_route.is_some(), "HTTPRoute for router should exist");

        let route = router_route.unwrap();

        if let Some(spec) = route.data.get("spec") {
            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert!(!rules.is_empty(), "HTTPRoute should have rules");

                // Check the first rule for filters
                let first_rule = &rules[0];
                if let Some(filters) = first_rule.get("filters").and_then(|v| v.as_array()) {
                    // Check for ResponseHeaderModifier
                    let response_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "ResponseHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(response_header_modifier.is_some(), "Should have ResponseHeaderModifier filter");

                    if let Some(modifier) = response_header_modifier {
                        if let Some(response_modifier) = modifier.get("responseHeaderModifier") {
                            if let Some(add) = response_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 response headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Custom-Response-Header", "response-value")),
                                    "Should contain X-Custom-Response-Header"
                                );
                                assert!(
                                    headers.contains(&("X-Frame-Options", "DENY")),
                                    "Should contain X-Frame-Options"
                                );
                            } else {
                                panic!("ResponseHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have responseHeaderModifier");
                        }
                    }

                    // Check for RequestHeaderModifier
                    let request_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "RequestHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(request_header_modifier.is_some(), "Should have RequestHeaderModifier filter");

                    if let Some(modifier) = request_header_modifier {
                        if let Some(request_modifier) = modifier.get("requestHeaderModifier") {
                            if let Some(add) = request_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 request headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Forwarded-Proto", "https")),
                                    "Should contain X-Forwarded-Proto"
                                );
                                assert!(headers.contains(&("X-Real-IP", "$remote_addr")), "Should contain X-Real-IP");
                            } else {
                                panic!("RequestHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have requestHeaderModifier");
                        }
                    }
                } else {
                    panic!("HTTPRoute rule should have filters");
                }
            } else {
                panic!("HTTPRoute should have rules");
            }
        } else {
            panic!("HTTPRoute should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}
#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_sticky_session_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_sticky_session_enable = true;

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-sticky-session-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for GRPC router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(load_balancer) = spec.get("loadBalancer") {
                let lb_type = load_balancer.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(lb_type, "ConsistentHash", "Load balancer type should be ConsistentHash");

                if let Some(consistent_hash) = load_balancer.get("consistentHash") {
                    let hash_type = consistent_hash.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(hash_type, "Cookie", "ConsistentHash type should be Cookie");

                    if let Some(cookie) = consistent_hash.get("cookie") {
                        let cookie_name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(
                            cookie_name, "INGRESSCOOKIE_QOVERY",
                            "Cookie name should be INGRESSCOOKIE_QOVERY"
                        );

                        let ttl = cookie.get("ttl").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(ttl, "85400s", "TTL should be 85400s (1 day)");
                    } else {
                        panic!("ConsistentHash should have cookie configuration");
                    }
                } else {
                    panic!("LoadBalancer should have consistentHash configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have loadBalancer configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_ip_whitelist_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_whitelist_source_range = "10.0.0.0/8,192.168.1.0/24,172.16.0.0/12".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-ip-whitelist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for GRPC router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Deny", "Default action should be Deny");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Allow", "Rule action should be Allow");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.1.0/24"), "Should contain 192.168.1.0/24");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Should contain 172.16.0.0/12");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_ip_denylist_enabled_on_scw_kapsule_grp() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_denylist_source_range = "192.168.0.0/16,10.10.10.0/24".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-ip-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for GRPC router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(
                    default_action, "Allow",
                    "Default action should be Allow when only denylist is configured"
                );

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Deny", "Rule action should be Deny");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");
                            assert!(cidrs.contains(&"10.10.10.0/24"), "Should contain 10.10.10.0/24");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_both_whitelist_and_denylist_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_whitelist_source_range = "10.0.0.0/8,172.16.0.0/12".to_string();
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_denylist_source_range = "10.10.10.0/24".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-whitelist-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for GRPC router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(
                    default_action, "Deny",
                    "Default action should be Deny when both whitelist and denylist are configured"
                );

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert_eq!(rules.len(), 2, "Should have 2 authorization rules");

                    // First rule should be Allow (whitelist)
                    let allow_rule = &rules[0];
                    let allow_action = allow_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(allow_action, "Allow", "First rule action should be Allow");

                    if let Some(principal) = allow_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Whitelist should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Whitelist should contain 172.16.0.0/12");
                        } else {
                            panic!("Allow rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Allow rule should have principal");
                    }

                    // Second rule should be Deny (denylist)
                    let deny_rule = &rules[1];
                    let deny_action = deny_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(deny_action, "Deny", "Second rule action should be Deny");

                    if let Some(principal) = deny_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.10.10.0/24"), "Denylist should contain 10.10.10.0/24");
                        } else {
                            panic!("Deny rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Deny rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_basic_auth_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_basic_auth_env_var = "HTPASSWD_CONTENT".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        let router_kube_name = format!("router-{suffix}");
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-basic-auth-test-router".to_string(),
            kube_name: router_kube_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for GRPC router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(basic_auth) = spec.get("basicAuth") {
                if let Some(users) = basic_auth.get("users") {
                    let secret_name = users.get("name").and_then(|v| v.as_str()).unwrap_or("");

                    let expected_secret_name = format!("htaccess-{router_kube_name}",);
                    assert_eq!(
                        secret_name, expected_secret_name,
                        "BasicAuth secret name should match htaccess-{{router_kube_name}}"
                    );
                } else {
                    panic!("BasicAuth should have users configuration");
                }
            } else {
                panic!("SecurityPolicy should have basicAuth configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_rate_limit_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_rpm = Some(1000);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_rps = Some(100);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_source_cidrs = "10.0.0.0/8,192.168.0.0/16".to_string();
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_route_limit_headers = "X-API-Key,X-User-ID".to_string();

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-rate-limit-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for GRPC router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(rate_limit) = spec.get("rateLimit") {
                let rate_limit_type = rate_limit.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(rate_limit_type, "Local", "Rate limit type should be Local");

                if let Some(local) = rate_limit.get("local") {
                    if let Some(rules) = local.get("rules").and_then(|v| v.as_array()) {
                        assert_eq!(rules.len(), 2, "Should have 2 rate limit rules (RPM and RPS)");

                        // Verify RPM rule
                        let rpm_rule = &rules[0];
                        if let Some(limit) = rpm_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 1000, "RPM limit should be 1000");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Minute", "RPM unit should be Minute");
                        } else {
                            panic!("RPM rule should have limit");
                        }

                        // Verify RPS rule
                        let rps_rule = &rules[1];
                        if let Some(limit) = rps_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 100, "RPS limit should be 100");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Second", "RPS unit should be Second");
                        } else {
                            panic!("RPS rule should have limit");
                        }
                    } else {
                        panic!("Local rate limit should have rules");
                    }
                } else {
                    panic!("Rate limit should have local configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have rateLimit configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_custom_headers_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;

        // Add response headers
        let mut add_headers = BTreeMap::new();
        add_headers.insert("X-Custom-Response-Header".to_string(), "response-value".to_string());
        add_headers.insert("X-Frame-Options".to_string(), "DENY".to_string());
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_add_headers = add_headers;

        // Add request headers
        let mut proxy_set_headers = BTreeMap::new();
        proxy_set_headers.insert("X-Forwarded-Proto".to_string(), "https".to_string());
        proxy_set_headers.insert("X-Real-IP".to_string(), "$remote_addr".to_string());
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_proxy_set_headers = proxy_set_headers;

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 50051,
            is_default: true,
            name: format!("grpc-{suffix}"),
            publicly_accessible: true,
            protocol: GRPC,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        let router_kube_name = format!("router-{suffix}");
        environment.routers = vec![Router {
            long_id: router_id,
            name: "grpc-custom-headers-test-router".to_string(),
            kube_name: router_kube_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "GRPCRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "grpcroutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let grpc_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!grpc_routes.items.is_empty());

        let router_route = grpc_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_route.is_some(), "GRPCRoute for router should exist");

        let route = router_route.unwrap();

        if let Some(spec) = route.data.get("spec") {
            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert!(!rules.is_empty(), "GRPCRoute should have rules");

                // Check the first rule for filters
                let first_rule = &rules[0];
                if let Some(filters) = first_rule.get("filters").and_then(|v| v.as_array()) {
                    // Check for ResponseHeaderModifier
                    let response_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "ResponseHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(response_header_modifier.is_some(), "Should have ResponseHeaderModifier filter");

                    if let Some(modifier) = response_header_modifier {
                        if let Some(response_modifier) = modifier.get("responseHeaderModifier") {
                            if let Some(add) = response_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 response headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Custom-Response-Header", "response-value")),
                                    "Should contain X-Custom-Response-Header"
                                );
                                assert!(
                                    headers.contains(&("X-Frame-Options", "DENY")),
                                    "Should contain X-Frame-Options"
                                );
                            } else {
                                panic!("ResponseHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have responseHeaderModifier");
                        }
                    }

                    // Check for RequestHeaderModifier
                    let request_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "RequestHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(request_header_modifier.is_some(), "Should have RequestHeaderModifier filter");

                    if let Some(modifier) = request_header_modifier {
                        if let Some(request_modifier) = modifier.get("requestHeaderModifier") {
                            if let Some(add) = request_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 request headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Forwarded-Proto", "https")),
                                    "Should contain X-Forwarded-Proto"
                                );
                                assert!(headers.contains(&("X-Real-IP", "$remote_addr")), "Should contain X-Real-IP");
                            } else {
                                panic!("RequestHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have requestHeaderModifier");
                        }
                    }
                } else {
                    panic!("GRPCRoute rule should have filters");
                }
            } else {
                panic!("GRPCRoute should have rules");
            }
        } else {
            panic!("GRPCRoute should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_cors_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "cors-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_enable_cors: true,
                network_gateway_api_cors_allow_origin: "https://example.com,https://test.com".to_string(),
                network_gateway_api_cors_allow_methods: "GET,POST,PUT,DELETE".to_string(),
                network_gateway_api_cors_allow_headers: "Content-Type,Authorization,X-Custom-Header".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "cors-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(cors) = spec.get("cors") {
                let allow_origins = cors
                    .get("allowOrigins")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_origins.contains(&"https://example.com"));
                assert!(allow_origins.contains(&"https://test.com"));

                let allow_methods = cors
                    .get("allowMethods")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_methods.contains(&"GET"));
                assert!(allow_methods.contains(&"POST"));
                assert!(allow_methods.contains(&"PUT"));
                assert!(allow_methods.contains(&"DELETE"));

                let allow_headers = cors
                    .get("allowHeaders")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_headers.contains(&"Content-Type"));
                assert!(allow_headers.contains(&"Authorization"));
                assert!(allow_headers.contains(&"X-Custom-Header"));
            } else {
                panic!("SecurityPolicy should have CORS configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_sticky_session_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "sticky-session-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_sticky_session_enable: true,
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "sticky-session-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(load_balancer) = spec.get("loadBalancer") {
                let lb_type = load_balancer.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(lb_type, "ConsistentHash", "Load balancer type should be ConsistentHash");

                if let Some(consistent_hash) = load_balancer.get("consistentHash") {
                    let hash_type = consistent_hash.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(hash_type, "Cookie", "ConsistentHash type should be Cookie");

                    if let Some(cookie) = consistent_hash.get("cookie") {
                        let cookie_name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(
                            cookie_name, "INGRESSCOOKIE_QOVERY",
                            "Cookie name should be INGRESSCOOKIE_QOVERY"
                        );

                        let ttl = cookie.get("ttl").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(ttl, "85400s", "TTL should be 85400s (1 day)");
                    } else {
                        panic!("ConsistentHash should have cookie configuration");
                    }
                } else {
                    panic!("LoadBalancer should have consistentHash configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have loadBalancer configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_ip_whitelist_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "ip-whitelist-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/8,192.168.1.0/24,172.16.0.0/12".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "ip-whitelist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Deny", "Default action should be Deny");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Allow", "Rule action should be Allow");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.1.0/24"), "Should contain 192.168.1.0/24");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Should contain 172.16.0.0/12");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_ip_denylist_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "ip-denylist-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_denylist_source_range: "192.168.0.0/16,10.10.10.0/24".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "ip-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Allow", "Default action should be Allow");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Deny", "Rule action should be Deny");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");
                            assert!(cidrs.contains(&"10.10.10.0/24"), "Should contain 10.10.10.0/24");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_both_whitelist_and_denylist_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "whitelist-denylist-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/8,172.16.0.0/12".to_string(),
                network_gateway_api_denylist_source_range: "10.10.10.0/24".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Deny", "Default action should be Deny");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert_eq!(rules.len(), 2, "Should have 2 authorization rules");

                    // First rule should be Allow (whitelist)
                    let allow_rule = &rules[0];
                    let allow_action = allow_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(allow_action, "Allow", "First rule action should be Allow");

                    if let Some(principal) = allow_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Whitelist should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Whitelist should contain 172.16.0.0/12");
                        } else {
                            panic!("Allow rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Allow rule should have principal");
                    }

                    // Second rule should be Deny (denylist)
                    let deny_rule = &rules[1];
                    let deny_action = deny_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(deny_action, "Deny", "Second rule action should be Deny");

                    if let Some(principal) = deny_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.10.10.0/24"), "Denylist should contain 10.10.10.0/24");
                        } else {
                            panic!("Deny rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Deny rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_basic_auth_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "basic-auth-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_basic_auth_env_var: "HTPASSWD_CONTENT".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        let router_kube_name = format!("router-{suffix}");
        environment.routers = vec![Router {
            long_id: router_id,
            name: "basic-auth-test-router".to_string(),
            kube_name: router_kube_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(basic_auth) = spec.get("basicAuth") {
                if let Some(users) = basic_auth.get("users") {
                    let secret_name = users.get("name").and_then(|v| v.as_str()).unwrap_or("");

                    let expected_secret_name = format!("htaccess-{router_kube_name}",);
                    assert_eq!(
                        secret_name, expected_secret_name,
                        "BasicAuth secret name should match htaccess-{{router_kube_name}}"
                    );
                } else {
                    panic!("BasicAuth should have users configuration");
                }
            } else {
                panic!("SecurityPolicy should have basicAuth configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_rate_limit_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "rate-limit-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_route_limit_rpm: Some(1000),
                network_gateway_api_route_limit_rps: Some(100),
                network_gateway_api_route_limit_source_cidrs: "10.0.0.0/8,192.168.0.0/16".to_string(),
                network_gateway_api_route_limit_headers: "X-API-Key,X-User-ID".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "rate-limit-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(rate_limit) = spec.get("rateLimit") {
                let rate_limit_type = rate_limit.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(rate_limit_type, "Local", "Rate limit type should be Local");

                if let Some(local) = rate_limit.get("local") {
                    if let Some(rules) = local.get("rules").and_then(|v| v.as_array()) {
                        assert_eq!(rules.len(), 2, "Should have 2 rate limit rules (RPM and RPS)");

                        // Verify RPM rule
                        let rpm_rule = &rules[0];
                        if let Some(limit) = rpm_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 1000, "RPM limit should be 1000");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Minute", "RPM unit should be Minute");
                        } else {
                            panic!("RPM rule should have limit");
                        }

                        // Verify RPM clientSelectors
                        if let Some(client_selectors) = rpm_rule.get("clientSelectors").and_then(|v| v.as_array()) {
                            // Check for source CIDR selectors
                            let source_cidr_selectors: Vec<_> = client_selectors
                                .iter()
                                .filter(|s| s.get("sourceCIDR").is_some())
                                .collect();
                            assert_eq!(source_cidr_selectors.len(), 2, "Should have 2 source CIDR selectors");

                            let cidrs: Vec<&str> = source_cidr_selectors
                                .iter()
                                .filter_map(|s| s.get("sourceCIDR")?.get("value")?.as_str())
                                .collect();
                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");

                            // Check for header selectors
                            let header_selectors: Vec<_> =
                                client_selectors.iter().filter(|s| s.get("headers").is_some()).collect();
                            assert_eq!(header_selectors.len(), 1, "Should have 1 header selector group");

                            if let Some(headers) = header_selectors[0].get("headers").and_then(|v| v.as_array()) {
                                assert_eq!(headers.len(), 2, "Should have 2 headers");
                                let header_names: Vec<&str> =
                                    headers.iter().filter_map(|h| h.get("name")?.as_str()).collect();
                                assert!(header_names.contains(&"X-API-Key"), "Should contain X-API-Key header");
                                assert!(header_names.contains(&"X-User-ID"), "Should contain X-User-ID header");
                            } else {
                                panic!("Header selector should have headers array");
                            }
                        } else {
                            panic!("RPM rule should have clientSelectors");
                        }

                        // Verify RPS rule
                        let rps_rule = &rules[1];
                        if let Some(limit) = rps_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 100, "RPS limit should be 100");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Second", "RPS unit should be Second");
                        } else {
                            panic!("RPS rule should have limit");
                        }

                        // Verify RPS clientSelectors (should be same as RPM)
                        if let Some(client_selectors) = rps_rule.get("clientSelectors").and_then(|v| v.as_array()) {
                            let source_cidr_selectors: Vec<_> = client_selectors
                                .iter()
                                .filter(|s| s.get("sourceCIDR").is_some())
                                .collect();
                            assert_eq!(source_cidr_selectors.len(), 2, "RPS should have 2 source CIDR selectors");
                        } else {
                            panic!("RPS rule should have clientSelectors");
                        }
                    } else {
                        panic!("Local rate limit should have rules");
                    }
                } else {
                    panic!("Rate limit should have local configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have rateLimit configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_custom_headers_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();

        // Add response headers
        let mut add_headers = BTreeMap::new();
        add_headers.insert("X-Custom-Response-Header".to_string(), "response-value".to_string());
        add_headers.insert("X-Frame-Options".to_string(), "DENY".to_string());

        // Add request headers
        let mut proxy_set_headers = BTreeMap::new();
        proxy_set_headers.insert("X-Forwarded-Proto".to_string(), "https".to_string());
        proxy_set_headers.insert("X-Real-IP".to_string(), "$remote_addr".to_string());

        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "custom-headers-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_add_headers: add_headers,
                network_gateway_api_proxy_set_headers: proxy_set_headers,
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-headers-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let http_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!http_routes.items.is_empty());

        let router_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_route.is_some(), "HTTPRoute for router should exist");

        let route = router_route.unwrap();

        if let Some(spec) = route.data.get("spec") {
            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert!(!rules.is_empty(), "HTTPRoute should have rules");

                // Check the first rule for filters
                let first_rule = &rules[0];
                if let Some(filters) = first_rule.get("filters").and_then(|v| v.as_array()) {
                    // Check for ResponseHeaderModifier
                    let response_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "ResponseHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(response_header_modifier.is_some(), "Should have ResponseHeaderModifier filter");

                    if let Some(modifier) = response_header_modifier {
                        if let Some(response_modifier) = modifier.get("responseHeaderModifier") {
                            if let Some(add) = response_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 response headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Custom-Response-Header", "response-value")),
                                    "Should contain X-Custom-Response-Header"
                                );
                                assert!(
                                    headers.contains(&("X-Frame-Options", "DENY")),
                                    "Should contain X-Frame-Options"
                                );
                            } else {
                                panic!("ResponseHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have responseHeaderModifier");
                        }
                    }

                    // Check for RequestHeaderModifier
                    let request_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "RequestHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(request_header_modifier.is_some(), "Should have RequestHeaderModifier filter");

                    if let Some(modifier) = request_header_modifier {
                        if let Some(request_modifier) = modifier.get("requestHeaderModifier") {
                            if let Some(add) = request_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 request headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Forwarded-Proto", "https")),
                                    "Should contain X-Forwarded-Proto"
                                );
                                assert!(headers.contains(&("X-Real-IP", "$remote_addr")), "Should contain X-Real-IP");
                            } else {
                                panic!("RequestHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have requestHeaderModifier");
                        }
                    }
                } else {
                    panic!("HTTPRoute rule should have filters");
                }
            } else {
                panic!("HTTPRoute should have rules");
            }
        } else {
            panic!("HTTPRoute should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_sticky_session_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "sticky-session-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_sticky_session_enable: true,
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "sticky-session-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(load_balancer) = spec.get("loadBalancer") {
                let lb_type = load_balancer.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(lb_type, "ConsistentHash", "Load balancer type should be ConsistentHash");

                if let Some(consistent_hash) = load_balancer.get("consistentHash") {
                    let hash_type = consistent_hash.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(hash_type, "Cookie", "ConsistentHash type should be Cookie");

                    if let Some(cookie) = consistent_hash.get("cookie") {
                        let cookie_name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(
                            cookie_name, "INGRESSCOOKIE_QOVERY",
                            "Cookie name should be INGRESSCOOKIE_QOVERY"
                        );

                        let ttl = cookie.get("ttl").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(ttl, "85400s", "TTL should be 85400s (1 day)");
                    } else {
                        panic!("ConsistentHash should have cookie configuration");
                    }
                } else {
                    panic!("LoadBalancer should have consistentHash configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have loadBalancer configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_ip_whitelist_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "ip-whitelist-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/8,192.168.1.0/24,172.16.0.0/12".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "ip-whitelist-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Deny", "Default action should be Deny");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Allow", "Rule action should be Allow");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.1.0/24"), "Should contain 192.168.1.0/24");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Should contain 172.16.0.0/12");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_ip_denylist_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "ip-denylist-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_denylist_source_range: "192.168.0.0/16,10.10.10.0/24".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "ip-denylist-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Allow", "Default action should be Allow");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty(), "Should have authorization rules");

                    let first_rule = &rules[0];
                    let action = first_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(action, "Deny", "Rule action should be Deny");

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");
                            assert!(cidrs.contains(&"10.10.10.0/24"), "Should contain 10.10.10.0/24");
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_both_whitelist_and_denylist_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "whitelist-denylist-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/8,172.16.0.0/12".to_string(),
                network_gateway_api_denylist_source_range: "10.10.10.0/24".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-denylist-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                let default_action = authorization
                    .get("defaultAction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                assert_eq!(default_action, "Deny", "Default action should be Deny");

                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert_eq!(rules.len(), 2, "Should have 2 authorization rules");

                    // First rule should be Allow (whitelist)
                    let allow_rule = &rules[0];
                    let allow_action = allow_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(allow_action, "Allow", "First rule action should be Allow");

                    if let Some(principal) = allow_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.0.0.0/8"), "Whitelist should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"172.16.0.0/12"), "Whitelist should contain 172.16.0.0/12");
                        } else {
                            panic!("Allow rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Allow rule should have principal");
                    }

                    // Second rule should be Deny (denylist)
                    let deny_rule = &rules[1];
                    let deny_action = deny_rule.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(deny_action, "Deny", "Second rule action should be Deny");

                    if let Some(principal) = deny_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<&str> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();

                            assert!(cidrs.contains(&"10.10.10.0/24"), "Denylist should contain 10.10.10.0/24");
                        } else {
                            panic!("Deny rule principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Deny rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_basic_auth_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "basic-auth-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_basic_auth_env_var: "HTPASSWD_CONTENT".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        let router_kube_name = format!("router-{suffix}");
        environment.routers = vec![Router {
            long_id: router_id,
            name: "basic-auth-test-router-grpc".to_string(),
            kube_name: router_kube_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "SecurityPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(basic_auth) = spec.get("basicAuth") {
                if let Some(users) = basic_auth.get("users") {
                    let secret_name = users.get("name").and_then(|v| v.as_str()).unwrap_or("");

                    let expected_secret_name = format!("htaccess-{router_kube_name}",);
                    assert_eq!(
                        secret_name, expected_secret_name,
                        "BasicAuth secret name should match htaccess-{{router_kube_name}}"
                    );
                } else {
                    panic!("BasicAuth should have users configuration");
                }
            } else {
                panic!("SecurityPolicy should have basicAuth configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_rate_limit_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "rate-limit-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_route_limit_rpm: Some(1000),
                network_gateway_api_route_limit_rps: Some(100),
                network_gateway_api_route_limit_source_cidrs: "10.0.0.0/8,192.168.0.0/16".to_string(),
                network_gateway_api_route_limit_headers: "X-API-Key,X-User-ID".to_string(),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "rate-limit-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(rate_limit) = spec.get("rateLimit") {
                let rate_limit_type = rate_limit.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(rate_limit_type, "Local", "Rate limit type should be Local");

                if let Some(local) = rate_limit.get("local") {
                    if let Some(rules) = local.get("rules").and_then(|v| v.as_array()) {
                        assert_eq!(rules.len(), 2, "Should have 2 rate limit rules (RPM and RPS)");

                        // Verify RPM rule
                        let rpm_rule = &rules[0];
                        if let Some(limit) = rpm_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 1000, "RPM limit should be 1000");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Minute", "RPM unit should be Minute");
                        } else {
                            panic!("RPM rule should have limit");
                        }

                        // Verify RPM clientSelectors
                        if let Some(client_selectors) = rpm_rule.get("clientSelectors").and_then(|v| v.as_array()) {
                            // Check for source CIDR selectors
                            let source_cidr_selectors: Vec<_> = client_selectors
                                .iter()
                                .filter(|s| s.get("sourceCIDR").is_some())
                                .collect();
                            assert_eq!(source_cidr_selectors.len(), 2, "Should have 2 source CIDR selectors");

                            let cidrs: Vec<&str> = source_cidr_selectors
                                .iter()
                                .filter_map(|s| s.get("sourceCIDR")?.get("value")?.as_str())
                                .collect();
                            assert!(cidrs.contains(&"10.0.0.0/8"), "Should contain 10.0.0.0/8");
                            assert!(cidrs.contains(&"192.168.0.0/16"), "Should contain 192.168.0.0/16");

                            // Check for header selectors
                            let header_selectors: Vec<_> =
                                client_selectors.iter().filter(|s| s.get("headers").is_some()).collect();
                            assert_eq!(header_selectors.len(), 1, "Should have 1 header selector group");

                            if let Some(headers) = header_selectors[0].get("headers").and_then(|v| v.as_array()) {
                                assert_eq!(headers.len(), 2, "Should have 2 headers");
                                let header_names: Vec<&str> =
                                    headers.iter().filter_map(|h| h.get("name")?.as_str()).collect();
                                assert!(header_names.contains(&"X-API-Key"), "Should contain X-API-Key header");
                                assert!(header_names.contains(&"X-User-ID"), "Should contain X-User-ID header");
                            } else {
                                panic!("Header selector should have headers array");
                            }
                        } else {
                            panic!("RPM rule should have clientSelectors");
                        }

                        // Verify RPS rule
                        let rps_rule = &rules[1];
                        if let Some(limit) = rps_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 100, "RPS limit should be 100");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Second", "RPS unit should be Second");
                        } else {
                            panic!("RPS rule should have limit");
                        }

                        // Verify RPS clientSelectors (should be same as RPM)
                        if let Some(client_selectors) = rps_rule.get("clientSelectors").and_then(|v| v.as_array()) {
                            let source_cidr_selectors: Vec<_> = client_selectors
                                .iter()
                                .filter(|s| s.get("sourceCIDR").is_some())
                                .collect();
                            assert_eq!(source_cidr_selectors.len(), 2, "RPS should have 2 source CIDR selectors");
                        } else {
                            panic!("RPS rule should have clientSelectors");
                        }
                    } else {
                        panic!("Local rate limit should have rules");
                    }
                } else {
                    panic!("Rate limit should have local configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have rateLimit configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_custom_headers_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();

        // Add response headers
        let mut add_headers = BTreeMap::new();
        add_headers.insert("X-Custom-Response-Header".to_string(), "response-value".to_string());
        add_headers.insert("X-Frame-Options".to_string(), "DENY".to_string());

        // Add request headers
        let mut proxy_set_headers = BTreeMap::new();
        proxy_set_headers.insert("X-Forwarded-Proto".to_string(), "https".to_string());
        proxy_set_headers.insert("X-Real-IP".to_string(), "$remote_addr".to_string());

        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "custom-headers-test-container-grpc".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:50051,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_add_headers: add_headers,
                network_gateway_api_proxy_set_headers: proxy_set_headers,
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-headers-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "GRPCRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "grpcroutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let grpc_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!grpc_routes.items.is_empty());

        let router_route = grpc_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_route.is_some(), "GRPCRoute for router should exist");

        let route = router_route.unwrap();

        if let Some(spec) = route.data.get("spec") {
            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert!(!rules.is_empty(), "GRPCRoute should have rules");

                // Check the first rule for filters
                let first_rule = &rules[0];
                if let Some(filters) = first_rule.get("filters").and_then(|v| v.as_array()) {
                    // Check for ResponseHeaderModifier
                    let response_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "ResponseHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(response_header_modifier.is_some(), "Should have ResponseHeaderModifier filter");

                    if let Some(modifier) = response_header_modifier {
                        if let Some(response_modifier) = modifier.get("responseHeaderModifier") {
                            if let Some(add) = response_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 response headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Custom-Response-Header", "response-value")),
                                    "Should contain X-Custom-Response-Header"
                                );
                                assert!(
                                    headers.contains(&("X-Frame-Options", "DENY")),
                                    "Should contain X-Frame-Options"
                                );
                            } else {
                                panic!("ResponseHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have responseHeaderModifier");
                        }
                    }

                    // Check for RequestHeaderModifier
                    let request_header_modifier = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "RequestHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(request_header_modifier.is_some(), "Should have RequestHeaderModifier filter");

                    if let Some(modifier) = request_header_modifier {
                        if let Some(request_modifier) = modifier.get("requestHeaderModifier") {
                            if let Some(add) = request_modifier.get("add").and_then(|v| v.as_array()) {
                                assert_eq!(add.len(), 2, "Should have 2 request headers");

                                let headers: Vec<(&str, &str)> = add
                                    .iter()
                                    .filter_map(|h| Some((h.get("name")?.as_str()?, h.get("value")?.as_str()?)))
                                    .collect();

                                assert!(
                                    headers.contains(&("X-Forwarded-Proto", "https")),
                                    "Should contain X-Forwarded-Proto"
                                );
                                assert!(headers.contains(&("X-Real-IP", "$remote_addr")), "Should contain X-Real-IP");
                            } else {
                                panic!("RequestHeaderModifier should have add array");
                            }
                        } else {
                            panic!("Filter should have requestHeaderModifier");
                        }
                    }
                } else {
                    panic!("GRPCRoute rule should have filters");
                }
            } else {
                panic!("GRPCRoute should have rules");
            }
        } else {
            panic!("GRPCRoute should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_force_ssl_redirect_on_scw_kapsule_http() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_force_ssl_redirect = true;

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 8080,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "http-ssl-redirect-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let http_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!http_routes.items.is_empty());

        // Find the main HTTPRoute (without -ssl-redirect suffix)
        let router_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
                && !route.name_any().contains("-ssl-redir")
        });

        assert!(router_route.is_some(), "HTTPRoute for router should exist");

        let route = router_route.unwrap();

        // Verify main route only has HTTPS listener
        if let Some(spec) = route.data.get("spec") {
            if let Some(parent_refs) = spec.get("parentRefs").and_then(|v| v.as_array()) {
                assert!(!parent_refs.is_empty(), "HTTPRoute should have parentRefs");

                // Should only have HTTPS listener when force SSL redirect is enabled
                let https_refs: Vec<_> = parent_refs
                    .iter()
                    .filter(|p| {
                        p.get("sectionName")
                            .and_then(|s| s.as_str())
                            .map(|s| s == "https")
                            .unwrap_or(false)
                    })
                    .collect();

                let http_refs: Vec<_> = parent_refs
                    .iter()
                    .filter(|p| {
                        p.get("sectionName")
                            .and_then(|s| s.as_str())
                            .map(|s| s == "http")
                            .unwrap_or(false)
                    })
                    .collect();

                assert!(!https_refs.is_empty(), "Should have HTTPS parentRef");
                assert!(
                    http_refs.is_empty(),
                    "Should NOT have HTTP parentRef when force SSL redirect is enabled"
                );
            } else {
                panic!("HTTPRoute should have parentRefs");
            }
        } else {
            panic!("HTTPRoute should have spec");
        }

        // Find the SSL redirect HTTPRoute
        let redirect_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
                && route.name_any().contains("-ssl-redir")
        });

        assert!(
            redirect_route.is_some(),
            "SSL redirect HTTPRoute should exist when force SSL redirect is enabled"
        );

        let redirect = redirect_route.unwrap();

        // Verify redirect route listens on HTTP and redirects to HTTPS
        if let Some(spec) = redirect.data.get("spec") {
            if let Some(parent_refs) = spec.get("parentRefs").and_then(|v| v.as_array()) {
                assert_eq!(parent_refs.len(), 1, "Redirect route should have exactly 1 parentRef");

                let section_name = parent_refs[0].get("sectionName").and_then(|s| s.as_str()).unwrap_or("");

                assert_eq!(section_name, "http", "Redirect route should listen on HTTP");
            } else {
                panic!("Redirect route should have parentRefs");
            }

            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert_eq!(
                    rules.len(),
                    2,
                    "Redirect route should have exactly 2 rules (ACME pass-through + redirect)"
                );

                // Rule 0: ACME HTTP-01 challenge pass-through — must NOT redirect, so that
                // cert-manager's solver HTTPRoute can handle /.well-known/acme-challenge/
                // requests without being 301-redirected to HTTPS first.
                let acme_rule = &rules[0];
                if let Some(matches) = acme_rule.get("matches").and_then(|v| v.as_array()) {
                    let acme_match = matches.iter().find(|m| {
                        m.get("path")
                            .and_then(|p| p.get("value"))
                            .and_then(|v| v.as_str())
                            .map(|v| v.starts_with("/.well-known/acme-challenge/"))
                            .unwrap_or(false)
                    });
                    assert!(
                        acme_match.is_some(),
                        "Rule 0 should match /.well-known/acme-challenge/ for ACME HTTP-01 pass-through"
                    );
                } else {
                    panic!("ACME pass-through rule should have matches");
                }

                // Rule 1: Redirect all other HTTP traffic to HTTPS.
                let redirect_rule = &rules[1];
                if let Some(filters) = redirect_rule.get("filters").and_then(|v| v.as_array()) {
                    let redirect_filter = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "RequestRedirect")
                            .unwrap_or(false)
                    });

                    assert!(redirect_filter.is_some(), "Should have RequestRedirect filter");

                    if let Some(filter) = redirect_filter {
                        if let Some(redirect_config) = filter.get("requestRedirect") {
                            let scheme = redirect_config.get("scheme").and_then(|s| s.as_str()).unwrap_or("");
                            let status_code = redirect_config.get("statusCode").and_then(|s| s.as_i64()).unwrap_or(0);

                            assert_eq!(scheme, "https", "Should redirect to HTTPS");
                            assert_eq!(status_code, 301, "Should use 301 status code");
                        } else {
                            panic!("RequestRedirect filter should have requestRedirect config");
                        }
                    }
                } else {
                    panic!("Redirect route should have filters");
                }
            } else {
                panic!("Redirect route should have rules");
            }
        } else {
            panic!("Redirect route should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_force_ssl_redirect_on_scw_kapsule_http() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "ssl-redirect-test-container-http".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:8080,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 8080,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_force_ssl_redirect: true,
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "container-http-ssl-redirect-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let http_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!http_routes.items.is_empty());

        // Find the main HTTPRoute (without -ssl-redirect suffix)
        let router_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
                && !route.name_any().contains("-ssl-redir")
        });

        assert!(router_route.is_some(), "HTTPRoute for router should exist");

        let route = router_route.unwrap();

        // Verify main route only has HTTPS listener
        if let Some(spec) = route.data.get("spec") {
            if let Some(parent_refs) = spec.get("parentRefs").and_then(|v| v.as_array()) {
                assert!(!parent_refs.is_empty(), "HTTPRoute should have parentRefs");

                // Should only have HTTPS listener when force SSL redirect is enabled
                let https_refs: Vec<_> = parent_refs
                    .iter()
                    .filter(|p| {
                        p.get("sectionName")
                            .and_then(|s| s.as_str())
                            .map(|s| s == "https")
                            .unwrap_or(false)
                    })
                    .collect();

                let http_refs: Vec<_> = parent_refs
                    .iter()
                    .filter(|p| {
                        p.get("sectionName")
                            .and_then(|s| s.as_str())
                            .map(|s| s == "http")
                            .unwrap_or(false)
                    })
                    .collect();

                assert!(!https_refs.is_empty(), "Should have HTTPS parentRef");
                assert!(
                    http_refs.is_empty(),
                    "Should NOT have HTTP parentRef when force SSL redirect is enabled"
                );
            } else {
                panic!("HTTPRoute should have parentRefs");
            }
        } else {
            panic!("HTTPRoute should have spec");
        }

        // Find the SSL redirect HTTPRoute
        let redirect_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
                && route.name_any().contains("-ssl-redir")
        });

        assert!(
            redirect_route.is_some(),
            "SSL redirect HTTPRoute should exist when force SSL redirect is enabled"
        );

        let redirect = redirect_route.unwrap();

        // Verify redirect route listens on HTTP and redirects to HTTPS
        if let Some(spec) = redirect.data.get("spec") {
            if let Some(parent_refs) = spec.get("parentRefs").and_then(|v| v.as_array()) {
                assert_eq!(parent_refs.len(), 1, "Redirect route should have exactly 1 parentRef");

                let section_name = parent_refs[0].get("sectionName").and_then(|s| s.as_str()).unwrap_or("");

                assert_eq!(section_name, "http", "Redirect route should listen on HTTP");
            } else {
                panic!("Redirect route should have parentRefs");
            }

            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert_eq!(
                    rules.len(),
                    2,
                    "Redirect route should have exactly 2 rules (ACME pass-through + redirect)"
                );

                // Rule 0: ACME HTTP-01 challenge pass-through — must NOT redirect, so that
                // cert-manager's solver HTTPRoute can handle /.well-known/acme-challenge/
                // requests without being 301-redirected to HTTPS first.
                let acme_rule = &rules[0];
                if let Some(matches) = acme_rule.get("matches").and_then(|v| v.as_array()) {
                    let acme_match = matches.iter().find(|m| {
                        m.get("path")
                            .and_then(|p| p.get("value"))
                            .and_then(|v| v.as_str())
                            .map(|v| v.starts_with("/.well-known/acme-challenge/"))
                            .unwrap_or(false)
                    });
                    assert!(
                        acme_match.is_some(),
                        "Rule 0 should match /.well-known/acme-challenge/ for ACME HTTP-01 pass-through"
                    );
                } else {
                    panic!("ACME pass-through rule should have matches");
                }

                // Rule 1: Redirect all other HTTP traffic to HTTPS.
                let redirect_rule = &rules[1];
                if let Some(filters) = redirect_rule.get("filters").and_then(|v| v.as_array()) {
                    let redirect_filter = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "RequestRedirect")
                            .unwrap_or(false)
                    });

                    assert!(redirect_filter.is_some(), "Should have RequestRedirect filter");

                    if let Some(filter) = redirect_filter {
                        if let Some(redirect_config) = filter.get("requestRedirect") {
                            let scheme = redirect_config.get("scheme").and_then(|s| s.as_str()).unwrap_or("");
                            let status_code = redirect_config.get("statusCode").and_then(|s| s.as_i64()).unwrap_or(0);

                            assert_eq!(scheme, "https", "Should redirect to HTTPS");
                            assert_eq!(status_code, 301, "Should use 301 status code");
                        } else {
                            panic!("RequestRedirect filter should have requestRedirect config");
                        }
                    }
                } else {
                    panic!("Redirect route should have filters");
                }
            } else {
                panic!("Redirect route should have rules");
            }
        } else {
            panic!("Redirect route should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_cors_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "cors-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: cors-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_enable_cors: true,
                network_gateway_api_cors_allow_origin: "https://example.com,https://test.com".to_string(),
                network_gateway_api_cors_allow_methods: "GET,POST,PUT,DELETE".to_string(),
                network_gateway_api_cors_allow_headers: "Content-Type,Authorization,X-Custom-Header".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "cors-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(cors) = spec.get("cors") {
                let allow_origins = cors
                    .get("allowOrigins")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_origins.contains(&"https://example.com"));
                assert!(allow_origins.contains(&"https://test.com"));

                let allow_methods = cors
                    .get("allowMethods")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_methods.contains(&"GET"));
                assert!(allow_methods.contains(&"POST"));
                assert!(allow_methods.contains(&"PUT"));
                assert!(allow_methods.contains(&"DELETE"));

                let allow_headers = cors
                    .get("allowHeaders")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                assert!(allow_headers.contains(&"Content-Type"));
                assert!(allow_headers.contains(&"Authorization"));
                assert!(allow_headers.contains(&"X-Custom-Header"));
            } else {
                panic!("SecurityPolicy should have CORS configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_sticky_session_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "sticky-session-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: sticky-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_sticky_session_enable: true,
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "sticky-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(load_balancer) = spec.get("loadBalancer") {
                let lb_type = load_balancer.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(lb_type, "ConsistentHash", "Load balancer type should be ConsistentHash");

                if let Some(consistent_hash) = load_balancer.get("consistentHash") {
                    let hash_type = consistent_hash.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(hash_type, "Cookie", "ConsistentHash type should be Cookie");

                    if let Some(cookie) = consistent_hash.get("cookie") {
                        let cookie_name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(
                            cookie_name, "INGRESSCOOKIE_QOVERY",
                            "Cookie name should be INGRESSCOOKIE_QOVERY"
                        );

                        let ttl = cookie.get("ttl").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(ttl, "85400s", "TTL should be 85400s (1 day)");
                    } else {
                        panic!("ConsistentHash should have cookie configuration");
                    }
                } else {
                    panic!("LoadBalancer should have consistentHash configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have loadBalancer configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_ip_whitelist_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "whitelist-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: whitelist-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/16,192.168.1.0/24".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty());

                    let first_rule = &rules[0];
                    if let Some(action) = first_rule.get("action").and_then(|v| v.as_str()) {
                        assert_eq!(action, "Allow");
                    }

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<_> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();
                            assert!(cidrs.contains(&"10.0.0.0/16"));
                            assert!(cidrs.contains(&"192.168.1.0/24"));
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_ip_denylist_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "denylist-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: denylist-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_denylist_source_range: "192.0.2.0/24,198.51.100.0/24".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty());

                    let first_rule = &rules[0];
                    if let Some(action) = first_rule.get("action").and_then(|v| v.as_str()) {
                        assert_eq!(action, "Deny");
                    }

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<_> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();
                            assert!(cidrs.contains(&"192.0.2.0/24"));
                            assert!(cidrs.contains(&"198.51.100.0/24"));
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_both_whitelist_and_denylist_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "whitelist-denylist-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: whitelist-denylist-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/16".to_string(),
                network_gateway_api_denylist_source_range: "10.0.1.0/24".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-denylist-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert_eq!(rules.len(), 2);

                    let deny_rule = rules.iter().find(|rule| {
                        rule.get("action")
                            .and_then(|a| a.as_str())
                            .map(|a| a == "Deny")
                            .unwrap_or(false)
                    });
                    assert!(deny_rule.is_some());

                    let allow_rule = rules.iter().find(|rule| {
                        rule.get("action")
                            .and_then(|a| a.as_str())
                            .map(|a| a == "Allow")
                            .unwrap_or(false)
                    });
                    assert!(allow_rule.is_some());
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_basic_auth_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        let basic_auth_value = "dXNlcjE6JGFwcjEkSDZuWlg0OEkkWUpOWFJuSExLcy9KL3kxZUpMbHhZLgo="; // user1:password1

        let mut env_vars = BTreeMap::new();
        env_vars.insert(
            "BASIC_AUTH".to_string(),
            VariableInfo {
                value: basic_auth_value.to_string(),
                is_secret: true,
            },
        );

        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "basic-auth-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: basic-auth-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: env_vars,
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_basic_auth_env_var: "BASIC_AUTH".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "basic-auth-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(basic_auth) = spec.get("basicAuth") {
                let users_ref = basic_auth.get("users").and_then(|v| v.as_object());
                assert!(users_ref.is_some(), "BasicAuth should have users reference");
            } else {
                panic!("SecurityPolicy should have basicAuth configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_rate_limit_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "rate-limit-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: rate-limit-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_route_limit_rps: Some(10),
                network_gateway_api_route_limit_rpm: Some(600),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "rate-limit-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let backend_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!backend_policies.items.is_empty());

        let router_policy = backend_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(rate_limit) = spec.get("rateLimit") {
                let rate_limit_type = rate_limit.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(rate_limit_type, "Local", "Rate limit type should be Local");

                if let Some(local) = rate_limit.get("local") {
                    if let Some(rules) = local.get("rules").and_then(|v| v.as_array()) {
                        assert_eq!(rules.len(), 2, "Should have 2 rate limit rules (RPM and RPS)");

                        // Verify RPM rule
                        let rpm_rule = &rules[0];
                        if let Some(limit) = rpm_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 600, "RPM limit should be 600");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Minute", "RPM unit should be Minute");
                        } else {
                            panic!("RPM rule should have limit");
                        }

                        // Verify RPS rule
                        let rps_rule = &rules[1];
                        if let Some(limit) = rps_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 10, "RPS limit should be 10");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Second", "RPS unit should be Second");
                        } else {
                            panic!("RPS rule should have limit");
                        }
                    } else {
                        panic!("Local rate limit should have rules");
                    }
                } else {
                    panic!("Rate limit should have local configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have rateLimit configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_custom_headers_enabled_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        let mut custom_headers = BTreeMap::new();
        custom_headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());
        custom_headers.insert("X-Another-Header".to_string(), "another-value".to_string());

        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "custom-headers-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: custom-headers-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_add_headers: custom_headers,
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-headers-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let http_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!http_routes.items.is_empty());

        let router_route = http_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
                && !route.name_any().contains("-ssl-redir")
        });

        assert!(router_route.is_some());

        let route = router_route.unwrap();

        if let Some(spec) = route.data.get("spec") {
            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert!(!rules.is_empty());

                let first_rule = &rules[0];
                if let Some(filters) = first_rule.get("filters").and_then(|v| v.as_array()) {
                    let header_filter = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "ResponseHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(header_filter.is_some(), "Should have ResponseHeaderModifier filter");

                    if let Some(filter) = header_filter {
                        if let Some(response_header_modifier) = filter.get("responseHeaderModifier") {
                            if let Some(add) = response_header_modifier.get("add").and_then(|v| v.as_array()) {
                                let has_custom_header = add.iter().any(|h| {
                                    h.get("name").and_then(|n| n.as_str()) == Some("X-Custom-Header")
                                        && h.get("value").and_then(|v| v.as_str()) == Some("custom-value")
                                });

                                let has_another_header = add.iter().any(|h| {
                                    h.get("name").and_then(|n| n.as_str()) == Some("X-Another-Header")
                                        && h.get("value").and_then(|v| v.as_str()) == Some("another-value")
                                });

                                assert!(has_custom_header, "Should have X-Custom-Header");
                                assert!(has_another_header, "Should have X-Another-Header");
                            } else {
                                panic!("ResponseHeaderModifier should have add section");
                            }
                        } else {
                            panic!("Filter should have responseHeaderModifier");
                        }
                    }
                } else {
                    panic!("HTTPRoute rules should have filters");
                }
            } else {
                panic!("HTTPRoute should have rules");
            }
        } else {
            panic!("HTTPRoute should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

// Helm GRPC Tests

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_sticky_session_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "sticky-session-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: sticky-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_sticky_session_enable: true,
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "sticky-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!traffic_policies.items.is_empty());

        let router_policy = traffic_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(load_balancer) = spec.get("loadBalancer") {
                let lb_type = load_balancer.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(lb_type, "ConsistentHash", "Load balancer type should be ConsistentHash");

                if let Some(consistent_hash) = load_balancer.get("consistentHash") {
                    let hash_type = consistent_hash.get("type").and_then(|v| v.as_str()).unwrap_or("");

                    assert_eq!(hash_type, "Cookie", "ConsistentHash type should be Cookie");

                    if let Some(cookie) = consistent_hash.get("cookie") {
                        let cookie_name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(
                            cookie_name, "INGRESSCOOKIE_QOVERY",
                            "Cookie name should be INGRESSCOOKIE_QOVERY"
                        );

                        let ttl = cookie.get("ttl").and_then(|v| v.as_str()).unwrap_or("");

                        assert_eq!(ttl, "85400s", "TTL should be 85400s (1 day)");
                    } else {
                        panic!("ConsistentHash should have cookie configuration");
                    }
                } else {
                    panic!("LoadBalancer should have consistentHash configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have loadBalancer configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_ip_whitelist_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "whitelist-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: whitelist-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/16,192.168.1.0/24".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty());

                    let first_rule = &rules[0];
                    if let Some(action) = first_rule.get("action").and_then(|v| v.as_str()) {
                        assert_eq!(action, "Allow");
                    }

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<_> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();
                            assert!(cidrs.contains(&"10.0.0.0/16"));
                            assert!(cidrs.contains(&"192.168.1.0/24"));
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_ip_denylist_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "denylist-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: denylist-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_denylist_source_range: "192.0.2.0/24,198.51.100.0/24".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "denylist-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert!(!rules.is_empty());

                    let first_rule = &rules[0];
                    if let Some(action) = first_rule.get("action").and_then(|v| v.as_str()) {
                        assert_eq!(action, "Deny");
                    }

                    if let Some(principal) = first_rule.get("principal") {
                        if let Some(client_cidrs) = principal.get("clientCIDRs").and_then(|v| v.as_array()) {
                            let cidrs: Vec<_> = client_cidrs.iter().filter_map(|v| v.as_str()).collect();
                            assert!(cidrs.contains(&"192.0.2.0/24"));
                            assert!(cidrs.contains(&"198.51.100.0/24"));
                        } else {
                            panic!("Principal should have clientCIDRs");
                        }
                    } else {
                        panic!("Rule should have principal");
                    }
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_both_whitelist_and_denylist_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "whitelist-denylist-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: whitelist-denylist-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_whitelist_source_range: "10.0.0.0/16".to_string(),
                network_gateway_api_denylist_source_range: "10.0.1.0/24".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "whitelist-denylist-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(authorization) = spec.get("authorization") {
                if let Some(rules) = authorization.get("rules").and_then(|v| v.as_array()) {
                    assert_eq!(rules.len(), 2);

                    let deny_rule = rules.iter().find(|rule| {
                        rule.get("action")
                            .and_then(|a| a.as_str())
                            .map(|a| a == "Deny")
                            .unwrap_or(false)
                    });
                    assert!(deny_rule.is_some());

                    let allow_rule = rules.iter().find(|rule| {
                        rule.get("action")
                            .and_then(|a| a.as_str())
                            .map(|a| a == "Allow")
                            .unwrap_or(false)
                    });
                    assert!(allow_rule.is_some());
                } else {
                    panic!("Authorization should have rules");
                }
            } else {
                panic!("SecurityPolicy should have authorization configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_basic_auth_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        let basic_auth_value = "dXNlcjE6JGFwcjEkSDZuWlg0OEkkWUpOWFJuSExLcy9KL3kxZUpMbHhZLgo="; // user1:password1

        let mut env_vars = BTreeMap::new();
        env_vars.insert(
            "BASIC_AUTH".to_string(),
            VariableInfo {
                value: basic_auth_value.to_string(),
                is_secret: true,
            },
        );

        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "basic-auth-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: basic-auth-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: env_vars,
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_basic_auth_env_var: "BASIC_AUTH".to_string(),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "basic-auth-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "SecurityPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "securitypolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let security_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!security_policies.items.is_empty());

        let router_policy = security_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(basic_auth) = spec.get("basicAuth") {
                let users_ref = basic_auth.get("users").and_then(|v| v.as_object());
                assert!(users_ref.is_some(), "BasicAuth should have users reference");
            } else {
                panic!("SecurityPolicy should have basicAuth configuration");
            }
        } else {
            panic!("SecurityPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_rate_limit_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "rate-limit-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: rate-limit-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_route_limit_rps: Some(10),
                network_gateway_api_route_limit_rpm: Some(600),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "rate-limit-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let backend_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!backend_policies.items.is_empty());

        let router_policy = backend_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_policy.is_some());

        let policy = router_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(rate_limit) = spec.get("rateLimit") {
                let rate_limit_type = rate_limit.get("type").and_then(|v| v.as_str()).unwrap_or("");

                assert_eq!(rate_limit_type, "Local", "Rate limit type should be Local");

                if let Some(local) = rate_limit.get("local") {
                    if let Some(rules) = local.get("rules").and_then(|v| v.as_array()) {
                        assert_eq!(rules.len(), 2, "Should have 2 rate limit rules (RPM and RPS)");

                        // Verify RPM rule
                        let rpm_rule = &rules[0];
                        if let Some(limit) = rpm_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 600, "RPM limit should be 600");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Minute", "RPM unit should be Minute");
                        } else {
                            panic!("RPM rule should have limit");
                        }

                        // Verify RPS rule
                        let rps_rule = &rules[1];
                        if let Some(limit) = rps_rule.get("limit") {
                            let requests = limit.get("requests").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                            assert_eq!(requests, 10, "RPS limit should be 10");

                            let unit = limit.get("unit").and_then(|v| v.as_str()).unwrap_or("");
                            assert_eq!(unit, "Second", "RPS unit should be Second");
                        } else {
                            panic!("RPS rule should have limit");
                        }
                    } else {
                        panic!("Local rate limit should have rules");
                    }
                } else {
                    panic!("Rate limit should have local configuration");
                }
            } else {
                panic!("BackendTrafficPolicy should have rateLimit configuration");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_custom_headers_enabled_on_scw_kapsule_grpc() {
    use qovery_engine::io_models::application::Protocol::GRPC;

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        let mut custom_headers = BTreeMap::new();
        custom_headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());
        custom_headers.insert("X-Another-Header".to_string(), "another-value".to_string());

        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "custom-headers-test-helm-grpc".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: custom-headers-test-grpc".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_add_headers: custom_headers,
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 50051,
                is_default: true,
                name: format!("grpc-{suffix}"),
                publicly_accessible: true,
                protocol: GRPC,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-headers-test-router-grpc".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "GRPCRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "grpcroutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client, namespace, &api_resource);

        let grpc_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        assert!(!grpc_routes.items.is_empty());

        let router_route = grpc_routes.items.iter().find(|route| {
            route
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(router_route.is_some());

        let route = router_route.unwrap();

        if let Some(spec) = route.data.get("spec") {
            if let Some(rules) = spec.get("rules").and_then(|v| v.as_array()) {
                assert!(!rules.is_empty());

                let first_rule = &rules[0];
                if let Some(filters) = first_rule.get("filters").and_then(|v| v.as_array()) {
                    let header_filter = filters.iter().find(|f| {
                        f.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "ResponseHeaderModifier")
                            .unwrap_or(false)
                    });

                    assert!(header_filter.is_some(), "Should have ResponseHeaderModifier filter");

                    if let Some(filter) = header_filter {
                        if let Some(response_header_modifier) = filter.get("responseHeaderModifier") {
                            if let Some(add) = response_header_modifier.get("add").and_then(|v| v.as_array()) {
                                let has_custom_header = add.iter().any(|h| {
                                    h.get("name").and_then(|n| n.as_str()) == Some("X-Custom-Header")
                                        && h.get("value").and_then(|v| v.as_str()) == Some("custom-value")
                                });

                                let has_another_header = add.iter().any(|h| {
                                    h.get("name").and_then(|n| n.as_str()) == Some("X-Another-Header")
                                        && h.get("value").and_then(|v| v.as_str()) == Some("another-value")
                                });

                                assert!(has_custom_header, "Should have X-Custom-Header");
                                assert!(has_another_header, "Should have X-Another-Header");
                            } else {
                                panic!("ResponseHeaderModifier should have add section");
                            }
                        } else {
                            panic!("Filter should have responseHeaderModifier");
                        }
                    }
                } else {
                    panic!("GRPCRoute rules should have filters");
                }
            } else {
                panic!("GRPCRoute should have rules");
            }
        } else {
            panic!("GRPCRoute should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_custom_http_errors_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_custom_http_errors = Some(vec![404, 503]);

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-errors-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        // Verify ConfigMap was created
        let cm_api: Api<ConfigMap> = Api::namespaced(kube_client.clone(), namespace);
        let configmap_name = format!("router-{suffix}-error-pages");
        let configmap = block_on(async { cm_api.get(&configmap_name).await });
        assert!(configmap.is_ok(), "ConfigMap should be created");

        let cm = configmap.unwrap();
        assert!(cm.data.is_some(), "ConfigMap should have data");

        let data = cm.data.unwrap();
        assert!(data.contains_key("404.html"), "ConfigMap should have 404.html");
        assert!(data.contains_key("503.html"), "ConfigMap should have 503.html");

        let error_404 = data.get("404.html").unwrap();
        assert!(error_404.contains("404"), "404 page should contain status code");
        assert!(error_404.contains("Not Found"), "404 page should contain error message");

        let error_503 = data.get("503.html").unwrap();
        assert!(error_503.contains("503"), "503 page should contain status code");
        assert!(
            error_503.contains("Service Unavailable"),
            "503 page should contain error message"
        );

        // Verify BackendTrafficPolicy was created
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let backend_traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        let policy = backend_traffic_policies.items.iter().find(|p| {
            p.metadata
                .name
                .as_ref()
                .map(|name| name.contains("traffic-policy"))
                .unwrap_or(false)
        });

        assert!(policy.is_some(), "BackendTrafficPolicy should be created");

        let btp = policy.unwrap();

        if let Some(spec) = btp.data.get("spec") {
            if let Some(response_override) = spec.get("responseOverride").and_then(|v| v.as_array()) {
                assert_eq!(response_override.len(), 2, "Should have 2 response overrides");

                let has_404 = response_override.iter().any(|override_entry| {
                    override_entry
                        .get("match")
                        .and_then(|m| m.get("statusCodes"))
                        .and_then(|sc| sc.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|code| code.get("value"))
                        .and_then(|v| v.as_u64())
                        == Some(404)
                });

                let has_503 = response_override.iter().any(|override_entry| {
                    override_entry
                        .get("match")
                        .and_then(|m| m.get("statusCodes"))
                        .and_then(|sc| sc.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|code| code.get("value"))
                        .and_then(|v| v.as_u64())
                        == Some(503)
                });

                assert!(has_404, "Should have 404 status code override");
                assert!(has_503, "Should have 503 status code override");

                // Verify ConfigMap reference
                for override_entry in response_override {
                    if let Some(response) = override_entry.get("response")
                        && let Some(body) = response.get("body")
                    {
                        let body_type = body.get("type").and_then(|t| t.as_str());
                        assert_eq!(body_type, Some("ValueRef"), "Should use ValueRef for body");

                        if let Some(value_ref) = body.get("valueRef") {
                            let cm_name = value_ref.get("name").and_then(|n| n.as_str());
                            assert_eq!(cm_name, Some(configmap_name.as_str()), "Should reference correct ConfigMap");
                        }
                    }
                }
            } else {
                panic!("BackendTrafficPolicy should have responseOverride");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_custom_http_errors_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "custom-errors-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:80,bind=0.0.0.0,reuseaddr,fork STDOUT"
                    .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", container_id, infra_ctx.dns_provider().domain()),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            readiness_probe: None,
            liveness_probe: None,
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_custom_http_errors: Some(vec![400, 500, 502]),
                ..Default::default()
            },
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-errors-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        // Verify ConfigMap was created
        let cm_api: Api<ConfigMap> = Api::namespaced(kube_client.clone(), namespace);
        let configmap_name = format!("router-{suffix}-error-pages");
        let configmap = block_on(async { cm_api.get(&configmap_name).await });
        assert!(configmap.is_ok(), "ConfigMap should be created");

        let cm = configmap.unwrap();
        assert!(cm.data.is_some(), "ConfigMap should have data");

        let data = cm.data.unwrap();
        assert!(data.contains_key("400.html"), "ConfigMap should have 400.html");
        assert!(data.contains_key("500.html"), "ConfigMap should have 500.html");
        assert!(data.contains_key("502.html"), "ConfigMap should have 502.html");

        // Verify BackendTrafficPolicy was created
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let backend_traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        let policy = backend_traffic_policies.items.iter().find(|p| {
            p.metadata
                .name
                .as_ref()
                .map(|name| name.contains("traffic-policy"))
                .unwrap_or(false)
        });

        assert!(policy.is_some(), "BackendTrafficPolicy should be created");

        let btp = policy.unwrap();

        if let Some(spec) = btp.data.get("spec") {
            if let Some(response_override) = spec.get("responseOverride").and_then(|v| v.as_array()) {
                assert_eq!(response_override.len(), 3, "Should have 3 response overrides");

                let status_codes: Vec<u64> = response_override
                    .iter()
                    .filter_map(|override_entry| {
                        override_entry
                            .get("match")
                            .and_then(|m| m.get("statusCodes"))
                            .and_then(|sc| sc.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|code| code.get("value"))
                            .and_then(|v| v.as_u64())
                    })
                    .collect();

                assert!(status_codes.contains(&400), "Should have 400 status code");
                assert!(status_codes.contains(&500), "Should have 500 status code");
                assert!(status_codes.contains(&502), "Should have 502 status code");
            } else {
                panic!("BackendTrafficPolicy should have responseOverride");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_custom_http_errors_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "custom-errors-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: custom-errors-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_custom_http_errors: Some(vec![401, 403, 404]),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "custom-errors-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        // Verify ConfigMap was created
        let cm_api: Api<ConfigMap> = Api::namespaced(kube_client.clone(), namespace);
        let configmap_name = format!("router-{suffix}-error-pages");
        let configmap = block_on(async { cm_api.get(&configmap_name).await });
        assert!(configmap.is_ok(), "ConfigMap should be created");

        let cm = configmap.unwrap();
        assert!(cm.data.is_some(), "ConfigMap should have data");

        let data = cm.data.unwrap();
        assert!(data.contains_key("401.html"), "ConfigMap should have 401.html");
        assert!(data.contains_key("403.html"), "ConfigMap should have 403.html");
        assert!(data.contains_key("404.html"), "ConfigMap should have 404.html");

        let error_401 = data.get("401.html").unwrap();
        assert!(error_401.contains("401"), "401 page should contain status code");
        assert!(error_401.contains("Unauthorized"), "401 page should contain error message");

        let error_403 = data.get("403.html").unwrap();
        assert!(error_403.contains("403"), "403 page should contain status code");
        assert!(error_403.contains("Forbidden"), "403 page should contain error message");

        // Verify BackendTrafficPolicy was created
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let backend_traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        let policy = backend_traffic_policies.items.iter().find(|p| {
            p.metadata
                .name
                .as_ref()
                .map(|name| name.contains("traffic-policy"))
                .unwrap_or(false)
        });

        assert!(policy.is_some(), "BackendTrafficPolicy should be created");

        let btp = policy.unwrap();

        if let Some(spec) = btp.data.get("spec") {
            if let Some(response_override) = spec.get("responseOverride").and_then(|v| v.as_array()) {
                assert_eq!(response_override.len(), 3, "Should have 3 response overrides");

                let status_codes: Vec<u64> = response_override
                    .iter()
                    .filter_map(|override_entry| {
                        override_entry
                            .get("match")
                            .and_then(|m| m.get("statusCodes"))
                            .and_then(|sc| sc.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|code| code.get("value"))
                            .and_then(|v| v.as_u64())
                    })
                    .collect();

                assert!(status_codes.contains(&401), "Should have 401 status code");
                assert!(status_codes.contains(&403), "Should have 403 status code");
                assert!(status_codes.contains(&404), "Should have 404 status code");

                // Verify ConfigMap reference in each response override
                for override_entry in response_override {
                    if let Some(response) = override_entry.get("response")
                        && let Some(body) = response.get("body")
                    {
                        let body_type = body.get("type").and_then(|t| t.as_str());
                        assert_eq!(body_type, Some("ValueRef"), "Should use ValueRef for body");

                        if let Some(value_ref) = body.get("valueRef") {
                            let cm_name = value_ref.get("name").and_then(|n| n.as_str());
                            assert_eq!(cm_name, Some(configmap_name.as_str()), "Should reference correct ConfigMap");

                            let kind = value_ref.get("kind").and_then(|k| k.as_str());
                            assert_eq!(kind, Some("ConfigMap"), "Should reference ConfigMap kind");
                        }
                    }
                }
            } else {
                panic!("BackendTrafficPolicy should have responseOverride");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}
#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_circuit_breaker_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let mut environment = helpers::environment::working_minimal_environment_with_router(&context, test_domain);

        environment.applications[0]
            .advanced_settings
            .network_gateway_api_circuit_breaker_max_connections = Some(100);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_circuit_breaker_max_pending_requests = Some(50);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_circuit_breaker_max_parallel_requests = Some(200);
        environment.applications[0].public_domain = format!("app-{suffix}.{test_domain}");

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        // First check if HTTPRoute was created
        let httproute_api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };
        let httproute_api: Api<kube::core::DynamicObject> =
            Api::namespaced_with(kube_client.clone(), namespace, &httproute_api_resource);
        let httproutes =
            retry_list_gateway_api_resources(&httproute_api).expect("Failed to list HTTPRoutes after retries");

        println!("Found {} HTTPRoutes in namespace {}", httproutes.items.len(), namespace);
        for route in &httproutes.items {
            println!("  - HTTPRoute: {:?}", route.metadata.name);
        }

        // Verify BackendTrafficPolicy was created with circuit breaker settings
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let backend_traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        println!(
            "Found {} BackendTrafficPolicies in namespace {}",
            backend_traffic_policies.items.len(),
            namespace
        );
        for policy in &backend_traffic_policies.items {
            println!("  - BackendTrafficPolicy: {:?}", policy.metadata.name);
        }

        let policy = backend_traffic_policies.items.iter().find(|p| {
            p.metadata
                .name
                .as_ref()
                .map(|name| name.contains("traffic-policy"))
                .unwrap_or(false)
        });

        assert!(
            policy.is_some(),
            "BackendTrafficPolicy should be created. HTTPRoutes: {}, BackendTrafficPolicies: {}",
            httproutes.items.len(),
            backend_traffic_policies.items.len()
        );

        let btp = policy.unwrap();

        if let Some(spec) = btp.data.get("spec") {
            if let Some(circuit_breaker) = spec.get("circuitBreaker") {
                let max_connections = circuit_breaker.get("maxConnections").and_then(|v| v.as_u64());
                let max_pending_requests = circuit_breaker.get("maxPendingRequests").and_then(|v| v.as_u64());
                let max_parallel_requests = circuit_breaker.get("maxParallelRequests").and_then(|v| v.as_u64());

                assert_eq!(max_connections, Some(100), "maxConnections should be 100");
                assert_eq!(max_pending_requests, Some(50), "maxPendingRequests should be 50");
                assert_eq!(max_parallel_requests, Some(200), "maxParallelRequests should be 200");
            } else {
                panic!("BackendTrafficPolicy should have circuitBreaker");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_circuit_breaker_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let container_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: container_id,
            name: "circuit-breaker-test-container".to_string(),
            kube_name: format!("container-{suffix}"),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.scw").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "apt-get update; apt-get install -y socat; socat TCP-LISTEN:3000,fork EXEC:/bin/cat".to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            gpu_request: None,
            gpu_limit: None,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("container-{suffix}.{test_domain}"),
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 3000,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
            storages: vec![],
            environment_vars_with_infos: BTreeMap::new(),
            mounted_files: vec![],
            advanced_settings: ContainerAdvancedSettings {
                network_gateway_api_circuit_breaker_max_connections: Some(150),
                network_gateway_api_circuit_breaker_max_pending_requests: Some(75),
                network_gateway_api_circuit_breaker_max_parallel_requests: Some(300),
                ..Default::default()
            },
            readiness_probe: None,
            liveness_probe: None,
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            autoscaling: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "circuit-breaker-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: container_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        // Verify BackendTrafficPolicy was created with circuit breaker settings
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let backend_traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        let policy = backend_traffic_policies.items.iter().find(|p| {
            p.metadata
                .name
                .as_ref()
                .map(|name| name.contains("traffic-policy"))
                .unwrap_or(false)
        });

        assert!(policy.is_some(), "BackendTrafficPolicy should be created");

        let btp = policy.unwrap();

        if let Some(spec) = btp.data.get("spec") {
            if let Some(circuit_breaker) = spec.get("circuitBreaker") {
                let max_connections = circuit_breaker.get("maxConnections").and_then(|v| v.as_u64());
                let max_pending_requests = circuit_breaker.get("maxPendingRequests").and_then(|v| v.as_u64());
                let max_parallel_requests = circuit_breaker.get("maxParallelRequests").and_then(|v| v.as_u64());

                assert_eq!(max_connections, Some(150), "maxConnections should be 150");
                assert_eq!(max_pending_requests, Some(75), "maxPendingRequests should be 75");
                assert_eq!(max_parallel_requests, Some(300), "maxParallelRequests should be 300");
            } else {
                panic!("BackendTrafficPolicy should have circuitBreaker");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_helm_with_circuit_breaker_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let helm_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![];
        environment.helms = vec![HelmChart {
            long_id: helm_id,
            name: "circuit-breaker-test-helm".to_string(),
            kube_name: format!("helm-{suffix}"),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "values.yaml".to_string(),
                    content: "nameOverride: circuit-breaker-test".to_string(),
                }],
            },
            set_values: vec![],
            set_string_values: vec![("serviceId".to_string(), helm_id.to_string())],
            set_json_values: vec![],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: HelmChartAdvancedSettings {
                network_gateway_api_circuit_breaker_max_connections: Some(250),
                network_gateway_api_circuit_breaker_max_pending_requests: Some(125),
                network_gateway_api_circuit_breaker_max_parallel_requests: Some(500),
                ..Default::default()
            },
            ports: vec![PortIo {
                long_id: Uuid::new_v4(),
                port: 80,
                is_default: true,
                name: format!("http-{suffix}"),
                publicly_accessible: true,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                path: Some("/".to_string()),
                path_rewrite: None,
            }],
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "circuit-breaker-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: helm_id,
            }],
        }];

        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        // Verify BackendTrafficPolicy was created with circuit breaker settings
        let api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let backend_traffic_policies =
            retry_list_gateway_api_resources(&api).expect("Failed to list Gateway API resources after retries");

        let policy = backend_traffic_policies.items.iter().find(|p| {
            p.metadata
                .name
                .as_ref()
                .map(|name| name.contains("traffic-policy"))
                .unwrap_or(false)
        });

        assert!(policy.is_some(), "BackendTrafficPolicy should be created");

        let btp = policy.unwrap();

        if let Some(spec) = btp.data.get("spec") {
            if let Some(circuit_breaker) = spec.get("circuitBreaker") {
                let max_connections = circuit_breaker.get("maxConnections").and_then(|v| v.as_u64());
                let max_pending_requests = circuit_breaker.get("maxPendingRequests").and_then(|v| v.as_u64());
                let max_parallel_requests = circuit_breaker.get("maxParallelRequests").and_then(|v| v.as_u64());

                assert_eq!(max_connections, Some(250), "maxConnections should be 250");
                assert_eq!(max_pending_requests, Some(125), "maxPendingRequests should be 125");
                assert_eq!(max_parallel_requests, Some(500), "maxParallelRequests should be 500");
            } else {
                panic!("BackendTrafficPolicy should have circuitBreaker");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_application_with_timeout_settings_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_tcp_keepalive_idle_time_seconds = Some(7200);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_tcp_keepalive_interval_seconds = Some(120);
        environment.applications[0]
            .advanced_settings
            .network_gateway_api_http_request_timeout_seconds = Some(90);

        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        environment.routers = vec![Router {
            long_id: router_id,
            name: "timeout-test-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();

        let backend_api_resource = kube::api::ApiResource {
            group: "gateway.envoyproxy.io".to_string(),
            version: "v1alpha1".to_string(),
            kind: "BackendTrafficPolicy".to_string(),
            api_version: "gateway.envoyproxy.io/v1alpha1".to_string(),
            plural: "backendtrafficpolicies".to_string(),
        };

        let backend_api: Api<kube::core::DynamicObject> =
            Api::namespaced_with(kube_client.clone(), namespace, &backend_api_resource);

        let backend_policies =
            retry_list_gateway_api_resources(&backend_api).expect("Failed to list Gateway API resources after retries");

        assert!(!backend_policies.items.is_empty());

        let backend_policy = backend_policies.items.iter().find(|policy| {
            policy
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("qovery.com/service-id"))
                .map(|id| *id == router_id.to_string())
                .unwrap_or(false)
        });

        assert!(backend_policy.is_some(), "BackendTrafficPolicy for router should exist");

        let policy = backend_policy.unwrap();

        if let Some(spec) = policy.data.get("spec") {
            if let Some(timeout) = spec.get("timeout") {
                if let Some(http) = timeout.get("http") {
                    let request_timeout = http.get("requestTimeout").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(request_timeout, "90s");
                } else {
                    panic!("BackendTrafficPolicy should have timeout.http");
                }
            } else {
                panic!("BackendTrafficPolicy should have timeout");
            }

            if let Some(tcp_keepalive) = spec.get("tcpKeepalive") {
                let idle_time = tcp_keepalive.get("idleTime").and_then(|v| v.as_str()).unwrap_or("");
                assert_eq!(idle_time, "7200s");

                let interval = tcp_keepalive.get("interval").and_then(|v| v.as_str()).unwrap_or("");
                assert_eq!(interval, "120s");

                let probes = tcp_keepalive.get("probes").and_then(|v| v.as_u64()).unwrap_or(0);
                assert_eq!(probes, 3);
            } else {
                panic!("BackendTrafficPolicy should have tcpKeepalive");
            }
        } else {
            panic!("BackendTrafficPolicy should have spec");
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_router_with_multiple_domains_splits_into_multiple_routes_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCW_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCW_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCW_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        // setup:
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .SCALEWAY_DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("SCALEWAY_DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        let app_id = environment.applications[0].long_id;
        environment.applications[0].ports = vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 80,
            is_default: true,
            name: format!("http-{suffix}"),
            publicly_accessible: true,
            protocol: HTTP,
            service_name: None,
            namespace: None,
            path: Some("/".to_string()),
            path_rewrite: None,
        }];

        let router_id = Uuid::new_v4();
        let router_name = format!("router-{suffix}");

        // Create 10 custom domains to trigger route splitting
        let custom_domains: Vec<CustomDomain> = (0..10)
            .map(|i| CustomDomain {
                domain: format!("custom-{i}-{suffix}.{test_domain}"),
                target_domain: format!("custom-{i}-{suffix}.{}.{}", context.cluster_short_id(), test_domain),
                generate_certificate: true,
                use_cdn: true, // speed-up DNS propagation for tests
            })
            .collect();

        environment.routers = vec![Router {
            long_id: router_id,
            name: "multi-domain-router".to_string(),
            kube_name: router_name.clone(),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains,
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: app_id,
            }],
        }];

        environment.containers = vec![];
        environment.jobs = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // execute:
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok(), "Deployment should succeed");

        // verify:
        let kube_client = infra_ctx.mk_kube_client().expect("kube client is not set").client();
        let namespace = environment.kube_name.as_str();
        let api_resource = kube::api::ApiResource {
            group: "gateway.networking.k8s.io".to_string(),
            version: "v1".to_string(),
            kind: "HTTPRoute".to_string(),
            api_version: "gateway.networking.k8s.io/v1".to_string(),
            plural: "httproutes".to_string(),
        };

        let api: Api<kube::core::DynamicObject> = Api::namespaced_with(kube_client.clone(), namespace, &api_resource);

        let http_routes =
            retry_list_gateway_api_resources(&api).expect("Failed to list HTTPRoute resources after retries");

        assert!(!http_routes.items.is_empty(), "HTTPRoutes should exist");

        // Find all route parts with the qovery.com/router-name label
        let router_routes: Vec<_> = http_routes
            .items
            .iter()
            .filter(|route| {
                route
                    .metadata
                    .labels
                    .as_ref()
                    .and_then(|labels| labels.get("qovery.com/router-name"))
                    .map(|name| name == &router_name)
                    .unwrap_or(false)
            })
            .collect();

        // With 10 custom domains, should create 3 route parts
        // Each domain generates 2 http_hosts entries (port-prefixed + bare domain);
        // default domain also generates 2 entries → total 22 entries; ceil(22/8) = 3 routes.
        assert_eq!(router_routes.len(), 3, "Should have 3 HTTPRoute parts for 10 custom domains");

        // Verify route names
        let route_names: Vec<String> = router_routes.iter().map(|route| route.name_any()).collect();

        assert!(
            route_names.contains(&format!("{router_name}-1")),
            "Should have {router_name}-1 route"
        );
        assert!(
            route_names.contains(&format!("{router_name}-2")),
            "Should have {router_name}-2 route"
        );
        assert!(
            route_names.contains(&format!("{router_name}-3")),
            "Should have {router_name}-3 route"
        );

        // Verify qovery.com/router-name label exists on all routes
        for route in &router_routes {
            let labels = route.metadata.labels.as_ref().expect("Route should have labels");
            assert_eq!(
                labels.get("qovery.com/router-name").map(|s| s.as_str()),
                Some(router_name.as_str()),
                "Route should have router-name label"
            );
        }

        // clean up:
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}
