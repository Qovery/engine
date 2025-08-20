use crate::helpers::common::{ActionableFeature, Cluster, ClusterDomain, NodeManager};
use crate::helpers::utilities::{FuncTestsSecrets, init};

use crate::helpers::aws::{AWS_KUBERNETES_VERSION, AWS_RESOURCE_TTL_IN_SECONDS};
use crate::helpers::gcp::{GCP_KUBERNETES_VERSION, GCP_RESOURCE_TTL};
use crate::helpers::scaleway::{SCW_KUBERNETES_VERSION, SCW_RESOURCE_TTL_IN_SECONDS};
use chrono::Utc;
use core::option::Option;
use core::option::Option::{None, Some};
use core::result::Result::Err;
use qovery_engine::environment::models::scaleway::ScwZone;
use qovery_engine::environment::task::EnvironmentTask;
use qovery_engine::fs::workspace_directory;
use qovery_engine::infrastructure::models::cloud_provider::aws::AWS;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use qovery_engine::infrastructure::models::cloud_provider::scaleway::Scaleway;
use qovery_engine::infrastructure::models::cloud_provider::{CloudProvider, Kind};
use qovery_engine::infrastructure::models::kubernetes::aws::eks::EKS;
use qovery_engine::infrastructure::models::kubernetes::gcp::Gke;
use qovery_engine::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use qovery_engine::infrastructure::models::kubernetes::{Kind as KubernetesKind, Kubernetes, KubernetesVersion};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::engine_location::EngineLocation;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::models::{CpuArchitecture, StorageClass, VpcQoveryNetworkMode};
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;

use crate::helpers::azure::{AZURE_KUBERNETES_VERSION, AZURE_RESOURCE_TTL_IN_SECONDS, azure_nodes_groups};
use crate::helpers::on_premise::ON_PREMISE_KUBERNETES_VERSION;
use qovery_engine::environment::models::abort::AbortStatus;
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::cloud_provider;
use qovery_engine::infrastructure::models::cloud_provider::azure::Azure;
use qovery_engine::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use qovery_engine::infrastructure::models::cloud_provider::service::Action;
use qovery_engine::infrastructure::models::kubernetes::azure::aks::AKS;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::io_models::metrics::MetricsConfiguration::MetricsInstalledByQovery;
use qovery_engine::io_models::metrics::MetricsParameters;
use std::str::FromStr;
use tracing::{Level, span};

pub const KUBERNETES_MIN_NODES: i32 = 3;
pub const KUBERNETES_MAX_NODES: i32 = 10;

#[derive(Clone)]
pub enum TargetCluster {
    MutualizedTestCluster { kubeconfig: String },
    New,
}

pub enum ClusterTestType {
    Classic,
    WithPause,
    WithUpgrade,
    WithNodesResize,
}

pub fn cluster_test(
    test_name: &str,
    provider_kind: Kind,
    kubernetes_kind: KubernetesKind,
    context: Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    region: &str,
    _zones: Option<Vec<&str>>,
    test_type: ClusterTestType,
    cluster_domain: &ClusterDomain,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    cpu_archi: CpuArchitecture,
    environment_to_deploy: Option<&EnvironmentRequest>,
    node_manager: NodeManager,
    actionable_features: Vec<ActionableFeature>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();

    let kubernetes_boot_version = match kubernetes_kind {
        KubernetesKind::Eks | KubernetesKind::EksSelfManaged | KubernetesKind::EksAnywhere => match test_type {
            ClusterTestType::WithUpgrade => AWS_KUBERNETES_VERSION.previous_version().expect("No previous version"),
            _ => AWS_KUBERNETES_VERSION,
        },
        KubernetesKind::Aks | KubernetesKind::AksSelfManaged => match test_type {
            ClusterTestType::WithUpgrade => AZURE_KUBERNETES_VERSION
                .previous_version()
                .expect("No previous version"),
            _ => AWS_KUBERNETES_VERSION,
        },
        KubernetesKind::ScwKapsule | KubernetesKind::ScwSelfManaged => match test_type {
            ClusterTestType::WithUpgrade => SCW_KUBERNETES_VERSION.previous_version().expect("No previous version"),
            _ => SCW_KUBERNETES_VERSION,
        },
        KubernetesKind::Gke | KubernetesKind::GkeSelfManaged => match test_type {
            ClusterTestType::WithUpgrade => GCP_KUBERNETES_VERSION.previous_version().expect("No previous version"),
            _ => GCP_KUBERNETES_VERSION,
        },
        KubernetesKind::OnPremiseSelfManaged => match test_type {
            ClusterTestType::WithUpgrade => ON_PREMISE_KUBERNETES_VERSION
                .previous_version()
                .expect("No previous version"),
            _ => ON_PREMISE_KUBERNETES_VERSION,
        },
    };

    let mut engine = create_infrastructure_context(
        None,
        &provider_kind,
        &context,
        logger.clone(),
        metrics_registry.clone(),
        region,
        kubernetes_kind,
        &kubernetes_boot_version,
        cluster_domain,
        &vpc_network_mode,
        cpu_archi,
        &node_manager,
        vec![],
    );

    // Bootstrap
    let deploy_tx = engine.kubernetes().as_infra_actions().create_cluster(&engine, false);
    assert!(deploy_tx.is_ok());

    // update
    engine.context_mut().update_is_first_cluster_deployment(false);

    // If no actionable features, then trigger a cluster update as usual
    if actionable_features.is_empty() {
        let deploy_tx = engine.kubernetes().as_infra_actions().create_cluster(&engine, false);
        assert!(deploy_tx.is_ok());
    } else {
        // 1. Enable actionable features
        let engine_with_features_enabled = create_infrastructure_context(
            None,
            &provider_kind,
            &context,
            logger.clone(),
            metrics_registry.clone(),
            region,
            kubernetes_kind,
            &kubernetes_boot_version,
            cluster_domain,
            &vpc_network_mode,
            cpu_archi,
            &node_manager,
            actionable_features,
        );

        let deploy_tx = engine
            .kubernetes()
            .as_infra_actions()
            .create_cluster(&engine_with_features_enabled, false);
        assert!(deploy_tx.is_ok());

        // 2. Disable actionable features
        let deploy_tx = engine.kubernetes().as_infra_actions().create_cluster(&engine, false);
        assert!(deploy_tx.is_ok());
    }

    // Deploy env if any
    if let Some(env) = environment_to_deploy {
        let mut env = env
            .to_environment_domain(
                &context,
                engine.cloud_provider(),
                engine.container_registry(),
                engine.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Create;
        if let Err(ret) = EnvironmentTask::deploy_environment(env, &engine, &|| AbortStatus::None) {
            panic!("{ret:?}")
        }
    }

    match test_type {
        // TODO new test type
        ClusterTestType::Classic => {}
        ClusterTestType::WithPause => {
            // Pause
            let pause_tx = engine.kubernetes().as_infra_actions().pause_cluster(&engine);
            assert!(pause_tx.is_ok());

            // Resume
            let resume_tx = engine.kubernetes().as_infra_actions().create_cluster(&engine, false);
            assert!(resume_tx.is_ok());
        }
        ClusterTestType::WithUpgrade => {
            let upgrade_to_version = kubernetes_boot_version.next_version().unwrap_or_else(|| {
                panic!("Kubernetes version `{kubernetes_boot_version}` has no next version defined for now",)
            });
            let engine = create_infrastructure_context(
                None,
                &provider_kind,
                &context,
                logger.clone(),
                metrics_registry.clone(),
                region,
                KubernetesKind::Eks,
                &upgrade_to_version,
                cluster_domain,
                &vpc_network_mode,
                cpu_archi,
                &node_manager,
                vec![],
            );

            // Upgrade
            let upgrade_tx = engine.kubernetes().as_infra_actions().run(&engine, Action::Create);
            assert!(upgrade_tx.is_ok());

            // Delete
            let delete_tx = engine.kubernetes().as_infra_actions().delete_cluster(&engine);
            assert!(delete_tx.is_ok());

            return test_name.to_string();
        }
        ClusterTestType::WithNodesResize => {
            let engine = create_infrastructure_context(
                Some(test_type),
                &provider_kind,
                &context,
                logger.clone(),
                metrics_registry.clone(),
                region,
                KubernetesKind::Eks,
                &kubernetes_boot_version,
                cluster_domain,
                &vpc_network_mode,
                cpu_archi,
                &node_manager,
                vec![],
            );

            // Upgrade
            let upgrade_tx = engine.kubernetes().as_infra_actions().create_cluster(&engine, false);
            assert!(upgrade_tx.is_ok());

            // Delete
            let delete_tx = engine.kubernetes().as_infra_actions().delete_cluster(&engine);
            assert!(delete_tx.is_ok());

            return test_name.to_string();
        }
    }

    // Destroy env if any
    if let Some(env) = environment_to_deploy {
        let mut env = env
            .to_environment_domain(
                &context,
                engine.cloud_provider(),
                engine.container_registry(),
                engine.kubernetes(),
            )
            .unwrap();

        env.action = Action::Delete;
        if let Err(ret) = EnvironmentTask::deploy_environment(env, &engine, &|| AbortStatus::None) {
            panic!("{ret:?}")
        }
    }

    // Delete
    if let Err(err) = engine.kubernetes().as_infra_actions().delete_cluster(&engine) {
        panic!("{err:?}")
    }

    test_name.to_string()
}

fn create_infrastructure_context(
    cluster_test_type: Option<ClusterTestType>,
    provider_kind: &Kind,
    context: &Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    region: &str,
    kubernetes_kind: KubernetesKind,
    kubernetes_boot_version: &KubernetesVersion,
    cluster_domain: &ClusterDomain,
    vpc_network_mode: &Option<VpcQoveryNetworkMode>,
    cpu_archi: CpuArchitecture,
    node_manager: &NodeManager,
    actionable_features: Vec<ActionableFeature>,
) -> InfrastructureContext {
    match provider_kind {
        Kind::Aws => {
            let (min_nodes, max_nodes) = match cluster_test_type {
                Some(ClusterTestType::WithNodesResize) => (11, 15),
                _ => (KUBERNETES_MIN_NODES, KUBERNETES_MAX_NODES),
            };
            AWS::docker_cr_engine(
                context,
                logger.clone(),
                metrics_registry.clone(),
                region,
                kubernetes_kind,
                kubernetes_boot_version.clone(),
                cluster_domain,
                vpc_network_mode.clone(),
                min_nodes,
                max_nodes,
                cpu_archi,
                EngineLocation::ClientSide,
                None, // <- no kubeconfig provided, new cluster
                node_manager.clone(),
                actionable_features,
            )
        }
        Kind::Scw => {
            let (min_nodes, max_nodes) = match cluster_test_type {
                Some(ClusterTestType::WithNodesResize) => (11, 15),
                _ => (KUBERNETES_MIN_NODES, KUBERNETES_MAX_NODES),
            };
            Scaleway::docker_cr_engine(
                context,
                logger.clone(),
                metrics_registry.clone(),
                region,
                kubernetes_kind,
                kubernetes_boot_version.clone(),
                cluster_domain,
                vpc_network_mode.clone(),
                min_nodes,
                max_nodes,
                cpu_archi,
                EngineLocation::ClientSide,
                None, // <- no kubeconfig provided, new cluster
                node_manager.clone(),
                actionable_features,
            )
        }
        Kind::Gcp => {
            Gke::docker_cr_engine(
                context,
                logger.clone(),
                metrics_registry.clone(),
                region,
                kubernetes_kind,
                kubernetes_boot_version.clone(),
                cluster_domain,
                vpc_network_mode.clone(),
                i32::MIN, // NA due to GKE autopilot
                i32::MAX, // NA due to GKE autopilot
                cpu_archi,
                EngineLocation::ClientSide,
                None, // <- no kubeconfig provided, new cluster
                node_manager.clone(),
                actionable_features,
            )
        }
        Kind::Azure => {
            let (min_nodes, max_nodes) = match cluster_test_type {
                Some(ClusterTestType::WithNodesResize) => (11, 15),
                _ => (KUBERNETES_MIN_NODES, KUBERNETES_MAX_NODES),
            };
            Azure::docker_cr_engine(
                context,
                logger.clone(),
                metrics_registry.clone(),
                region,
                kubernetes_kind,
                kubernetes_boot_version.clone(),
                cluster_domain,
                vpc_network_mode.clone(),
                min_nodes,
                max_nodes,
                cpu_archi,
                EngineLocation::ClientSide,
                None, // <- no kubeconfig provided, new cluster
                node_manager.clone(),
                actionable_features,
            )
        }
        Kind::OnPremise => todo!(),
    }
}

pub fn get_environment_test_kubernetes(
    context: &Context,
    cloud_provider: &dyn CloudProvider,
    kubernetes_version: KubernetesVersion,
    logger: Box<dyn Logger>,
    localisation: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    min_nodes: i32,
    max_nodes: i32,
    cpu_archi: CpuArchitecture,
    engine_location: EngineLocation,
    default_kubernetes_storage_class: StorageClass,
    kubeconfig: Option<String>,
    node_manager: NodeManager,
    actionable_features: Vec<ActionableFeature>,
) -> Box<dyn Kubernetes> {
    let secrets = FuncTestsSecrets::new();

    let temp_dir = workspace_directory(
        context.workspace_root_dir(),
        context.execution_id(),
        format!("bootstrap/{}", context.cluster_short_id()),
    )
    .unwrap();

    let kubernetes: Box<dyn Kubernetes> = match cloud_provider.kubernetes_kind() {
        KubernetesKind::Eks => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");

            let mut options = AWS::kubernetes_cluster_options(
                secrets.clone(),
                QoveryIdentifier::new(*context.cluster_long_id()),
                engine_location,
                None,
            );
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            match node_manager {
                NodeManager::Karpenter { config } => options.karpenter_parameters = Some(config),
                NodeManager::Default => {}
                NodeManager::AutoPilot => {}
            }

            actionable_features.iter().for_each(|feature| {
                match feature {
                    ActionableFeature::Metrics => {
                        options.metrics_parameters = Some(MetricsParameters {
                            config: MetricsInstalledByQovery {
                                install_prometheus_adapter: false, // The prometheus adapter is only enabled for our prod clusters, it's not configurable for clients
                                enable_redundancy: None,
                            },
                        })
                    }
                }
            });

            Box::new(
                EKS::new(
                    context.clone(),
                    *context.cluster_long_id(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    Utc::now(),
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes, cpu_archi),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: AWS_RESOURCE_TTL_IN_SECONDS as i32,
                        aws_vpc_enable_flow_logs: true,
                        aws_eks_ec2_metadata_imds:
                            qovery_engine::infrastructure::models::cloud_provider::io::AwsEc2MetadataImds::Required,
                        aws_eks_enable_alb_controller: true,
                        k8s_storage_class_fast_ssd: cloud_provider::io::StorageClass::from(
                            default_kubernetes_storage_class,
                        ),
                        ..Default::default()
                    },
                    None,
                    kubeconfig,
                    temp_dir,
                    None,
                )
                .unwrap(),
            )
        }
        KubernetesKind::Aks => {
            let region = AzureLocation::from_str(localisation).expect("Azure region not supported");
            Box::new(
                AKS::new(
                    context.clone(),
                    *context.cluster_long_id(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region,
                    cloud_provider,
                    Utc::now(),
                    Azure::kubernetes_cluster_options(
                        secrets.clone(),
                        QoveryIdentifier::new(*context.cluster_long_id()),
                        EngineLocation::ClientSide,
                        None,
                    ),
                    azure_nodes_groups(min_nodes, max_nodes, cpu_archi),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: AZURE_RESOURCE_TTL_IN_SECONDS as i32,
                        k8s_storage_class_fast_ssd: cloud_provider::io::StorageClass::from(
                            default_kubernetes_storage_class,
                        ),
                        ..Default::default()
                    },
                    None,
                    kubeconfig,
                    temp_dir,
                    None,
                )
                .expect("Cannot instantiate AKS"),
            )
        }
        KubernetesKind::ScwKapsule => {
            let zone = ScwZone::from_str(localisation).expect("SCW zone not supported");

            let mut options = Scaleway::kubernetes_cluster_options(
                secrets.clone(),
                QoveryIdentifier::new(*context.cluster_long_id()),
                EngineLocation::ClientSide,
                None,
            );
            actionable_features.iter().for_each(|feature| {
                match feature {
                    ActionableFeature::Metrics => {
                        options.metrics_parameters = Some(MetricsParameters {
                            config: MetricsInstalledByQovery {
                                install_prometheus_adapter: false, // The prometheus adapter is only enabled for our prod clusters, it's not configurable for clients
                                enable_redundancy: None,
                            },
                        })
                    }
                }
            });

            Box::new(
                Kapsule::new(
                    context.clone(),
                    *context.cluster_long_id(),
                    format!("qovery-{}", context.cluster_short_id()),
                    kubernetes_version,
                    zone,
                    cloud_provider,
                    Utc::now(),
                    Scaleway::kubernetes_nodes(min_nodes, max_nodes, cpu_archi),
                    options,
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: SCW_RESOURCE_TTL_IN_SECONDS as i32,
                        k8s_storage_class_fast_ssd: cloud_provider::io::StorageClass::from(
                            default_kubernetes_storage_class,
                        ),
                        ..Default::default()
                    },
                    None,
                    kubeconfig,
                    temp_dir,
                )
                .expect("Cannot instantiate SCW Kapsule"),
            )
        }
        KubernetesKind::Gke => {
            let region = GcpRegion::from_str(localisation).expect("GCP zone not supported");
            let mut options = Gke::kubernetes_cluster_options(
                secrets.clone(),
                QoveryIdentifier::new(*context.cluster_long_id()),
                EngineLocation::ClientSide,
                vpc_network_mode,
            );

            actionable_features.iter().for_each(|feature| {
                match feature {
                    ActionableFeature::Metrics => {
                        options.metrics_parameters = Some(MetricsParameters {
                            config: MetricsInstalledByQovery {
                                install_prometheus_adapter: false, // The prometheus adapter is only enabled for our prod clusters, it's not configurable for clients
                                enable_redundancy: None,
                            },
                        })
                    }
                }
            });

            Box::new(
                Gke::new(
                    context.clone(),
                    *context.cluster_long_id(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    cloud_provider,
                    kubernetes_version,
                    region,
                    Utc::now(),
                    options,
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: GCP_RESOURCE_TTL.as_secs() as i32,
                        k8s_storage_class_fast_ssd: cloud_provider::io::StorageClass::from(
                            default_kubernetes_storage_class,
                        ),
                        ..Default::default()
                    },
                    None,
                    kubeconfig,
                    temp_dir,
                )
                .expect("Cannot instantiate GKE"),
            )
        }
        KubernetesKind::AksSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::GkeSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::ScwSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::EksSelfManaged => todo!(), // TODO: Byok integration
        KubernetesKind::OnPremiseSelfManaged => todo!(), // TODO how to test on-premise clusters ?
        KubernetesKind::EksAnywhere => todo!(),    // TODO how to test on-premise clusters ?
    };

    kubernetes
}
