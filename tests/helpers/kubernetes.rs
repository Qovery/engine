use crate::helpers::aws_ec2::ec2_kubernetes_instance;
use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::utilities::{init, FuncTestsSecrets};

use crate::helpers::aws::AWS_KUBERNETES_VERSION;
use crate::helpers::scaleway::SCW_KUBERNETES_VERSION;
use core::option::Option;
use core::option::Option::{None, Some};
use core::result::Result::Err;
use qovery_engine::cloud_provider::aws::kubernetes::ec2::EC2;
use qovery_engine::cloud_provider::aws::kubernetes::eks::EKS;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::io::ClusterAdvancedSettings;
use qovery_engine::cloud_provider::kubernetes::{Kind as KubernetesKind, Kubernetes, KubernetesVersion};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::{CloudProvider, Kind};
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine_task::environment_task::EnvironmentTask;
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::logger::Logger;
use qovery_engine::models::scaleway::ScwZone;
use qovery_engine::transaction::{Transaction, TransactionResult};
use std::str::FromStr;
use std::sync::Arc;
use tracing::{span, Level};
use uuid::Uuid;

pub const KUBERNETES_MIN_NODES: i32 = 5;
pub const KUBERNETES_MAX_NODES: i32 = 10;

pub enum ClusterTestType {
    Classic,
    WithPause,
    WithUpgrade,
    WithNodesResize,
}

pub fn get_cluster_test_kubernetes<'a>(
    secrets: FuncTestsSecrets,
    context: &Context,
    cluster_id: String,
    cluster_name: String,
    boot_version: KubernetesVersion,
    localisation: &str,
    aws_zones: Option<Vec<AwsZones>>,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    kubernetes_provider: KubernetesKind,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    logger: Box<dyn Logger>,
    min_nodes: i32,
    max_nodes: i32,
) -> Box<dyn Kubernetes + 'a> {
    let kubernetes: Box<dyn Kubernetes> = match kubernetes_provider {
        KubernetesKind::Eks => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();

            Box::new(
                EKS::new(
                    context.clone(),
                    cluster_id.as_str(),
                    Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version,
                    aws_region,
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
        }
        KubernetesKind::Ec2 => {
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::QoverySide);
            let aws_region = AwsRegion::from_str(localisation).expect("expected correct AWS region");
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }
            let aws_zones = aws_zones.unwrap().into_iter().map(|zone| zone.to_string()).collect();

            Box::new(
                EC2::new(
                    context.clone(),
                    cluster_id.as_str(),
                    Uuid::new_v4(),
                    cluster_name.as_str(),
                    boot_version,
                    aws_region,
                    aws_zones,
                    cloud_provider,
                    dns_provider,
                    options,
                    ec2_kubernetes_instance(),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 7200,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
        }
        KubernetesKind::ScwKapsule => Box::new(
            Kapsule::new(
                context.clone(),
                Uuid::new_v4(),
                cluster_name,
                boot_version,
                ScwZone::from_str(localisation).expect("Unknown zone set for Kapsule"),
                cloud_provider,
                dns_provider,
                Scaleway::kubernetes_nodes(min_nodes, max_nodes),
                Scaleway::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                logger,
                ClusterAdvancedSettings {
                    pleco_resources_ttl: 14400,
                    ..Default::default()
                },
            )
            .unwrap(),
        ),
    };

    kubernetes
}

pub fn cluster_test(
    test_name: &str,
    provider_kind: Kind,
    kubernetes_kind: KubernetesKind,
    context: Context,
    logger: Box<dyn Logger>,
    localisation: &str,
    aws_zones: Option<Vec<AwsZones>>,
    test_type: ClusterTestType,
    cluster_domain: &ClusterDomain,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    environment_to_deploy: Option<&EnvironmentRequest>,
) -> String {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();

    let kubernetes_boot_version = match kubernetes_kind {
        KubernetesKind::Eks => AWS_KUBERNETES_VERSION,
        KubernetesKind::Ec2 => KubernetesVersion::V1_23 {
            prefix: Some('v'.to_string()),
            patch: Some(16),
            suffix: Some("+k3s1".to_string()),
        },
        KubernetesKind::ScwKapsule => SCW_KUBERNETES_VERSION,
    };

    let engine = match provider_kind {
        Kind::Aws => AWS::docker_cr_engine(
            &context,
            logger.clone(),
            localisation,
            kubernetes_kind,
            kubernetes_boot_version.clone(),
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
        Kind::Scw => Scaleway::docker_cr_engine(
            &context,
            logger.clone(),
            localisation,
            kubernetes_kind,
            kubernetes_boot_version.clone(),
            cluster_domain,
            vpc_network_mode.clone(),
            KUBERNETES_MIN_NODES,
            KUBERNETES_MAX_NODES,
            EngineLocation::ClientSide,
        ),
    };
    let mut deploy_tx = Transaction::new(&engine).unwrap();
    let mut delete_tx = Transaction::new(&engine).unwrap();

    let mut aws_zones_string: Vec<String> = Vec::with_capacity(3);
    if let Some(aws_zones) = aws_zones {
        for zone in aws_zones {
            aws_zones_string.push(zone.to_string())
        }
    };

    // Deploy
    if let Err(err) = deploy_tx.create_kubernetes() {
        panic!("{err:?}")
    }
    assert!(matches!(deploy_tx.commit(), TransactionResult::Ok));

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

        env.action = qovery_engine::cloud_provider::service::Action::Create;
        if let Err(ret) = EnvironmentTask::deploy_environment(env, &engine, &|| false) {
            panic!("{ret:?}")
        }
    }

    match test_type {
        // TODO new test type
        ClusterTestType::Classic => {}
        ClusterTestType::WithPause => {
            let mut pause_tx = Transaction::new(&engine).unwrap();
            let mut resume_tx = Transaction::new(&engine).unwrap();

            // Pause
            if let Err(err) = pause_tx.pause_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(pause_tx.commit(), TransactionResult::Ok));

            // Resume
            if let Err(err) = resume_tx.create_kubernetes() {
                panic!("{err:?}")
            }

            assert!(matches!(resume_tx.commit(), TransactionResult::Ok));
        }
        ClusterTestType::WithUpgrade => {
            let upgrade_to_version = kubernetes_boot_version.next_version().unwrap_or_else(|| {
                panic!("Kubernetes version `{kubernetes_boot_version}` has no next version defined for now",)
            });
            let engine = match provider_kind {
                Kind::Aws => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::Eks,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
                Kind::Scw => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::ScwKapsule,
                    upgrade_to_version,
                    cluster_domain,
                    vpc_network_mode,
                    KUBERNETES_MIN_NODES,
                    KUBERNETES_MAX_NODES,
                    EngineLocation::ClientSide,
                ),
            };
            let mut upgrade_tx = Transaction::new(&engine).unwrap();
            let mut delete_tx = Transaction::new(&engine).unwrap();

            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(upgrade_tx.commit(), TransactionResult::Ok));

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

            return test_name.to_string();
        }
        ClusterTestType::WithNodesResize => {
            let min_nodes = 11;
            let max_nodes = 15;
            let engine = match provider_kind {
                Kind::Aws => AWS::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::Eks,
                    kubernetes_boot_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    EngineLocation::ClientSide,
                ),
                Kind::Scw => Scaleway::docker_cr_engine(
                    &context,
                    logger.clone(),
                    localisation,
                    KubernetesKind::ScwKapsule,
                    kubernetes_boot_version,
                    cluster_domain,
                    vpc_network_mode,
                    min_nodes,
                    max_nodes,
                    EngineLocation::ClientSide,
                ),
            };
            let mut upgrade_tx = Transaction::new(&engine).unwrap();
            let mut delete_tx = Transaction::new(&engine).unwrap();
            // Upgrade
            if let Err(err) = upgrade_tx.create_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(upgrade_tx.commit(), TransactionResult::Ok));

            // Delete
            if let Err(err) = delete_tx.delete_kubernetes() {
                panic!("{err:?}")
            }
            assert!(matches!(delete_tx.commit(), TransactionResult::Ok));
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

        env.action = qovery_engine::cloud_provider::service::Action::Delete;
        if let Err(ret) = EnvironmentTask::deploy_environment(env, &engine, &|| false) {
            panic!("{ret:?}")
        }
    }

    // Delete
    if let Err(err) = delete_tx.delete_kubernetes() {
        panic!("{err:?}")
    }
    assert!(matches!(delete_tx.commit(), TransactionResult::Ok));

    test_name.to_string()
}

pub fn get_environment_test_kubernetes(
    context: &Context,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    kubernetes_version: KubernetesVersion,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    logger: Box<dyn Logger>,
    localisation: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
    min_nodes: i32,
    max_nodes: i32,
    engine_location: EngineLocation,
) -> Box<dyn Kubernetes> {
    let secrets = FuncTestsSecrets::new();

    let kubernetes: Box<dyn Kubernetes> = match cloud_provider.kubernetes_kind() {
        KubernetesKind::Eks => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");
            let mut options = AWS::kubernetes_cluster_options(secrets, None, engine_location);
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            Box::new(
                EKS::new(
                    context.clone(),
                    context.cluster_short_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    options,
                    AWS::kubernetes_nodes(min_nodes, max_nodes),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        aws_vpc_enable_flow_logs: true,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
        }
        KubernetesKind::Ec2 => {
            let region = AwsRegion::from_str(localisation).expect("AWS region not supported");
            let mut options = AWS::kubernetes_cluster_options(secrets, None, EngineLocation::QoverySide);
            if let Some(vpc_network_mode) = vpc_network_mode {
                options.vpc_qovery_network_mode = vpc_network_mode;
            }

            Box::new(
                EC2::new(
                    context.clone(),
                    context.cluster_short_id(),
                    Uuid::new_v4(),
                    format!("qovery-{}", context.cluster_short_id()).as_str(),
                    kubernetes_version,
                    region.clone(),
                    region.get_zones_to_string(),
                    cloud_provider,
                    dns_provider,
                    options,
                    ec2_kubernetes_instance(),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 7200,
                        aws_vpc_enable_flow_logs: false,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
        }
        KubernetesKind::ScwKapsule => {
            let zone = ScwZone::from_str(localisation).expect("SCW zone not supported");
            Box::new(
                Kapsule::new(
                    context.clone(),
                    *context.cluster_long_id(),
                    format!("qovery-{}", context.cluster_short_id()),
                    kubernetes_version,
                    zone,
                    cloud_provider,
                    dns_provider,
                    Scaleway::kubernetes_nodes(min_nodes, max_nodes),
                    Scaleway::kubernetes_cluster_options(secrets, None, EngineLocation::ClientSide),
                    logger,
                    ClusterAdvancedSettings {
                        pleco_resources_ttl: 14400,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
        }
    };

    kubernetes
}
