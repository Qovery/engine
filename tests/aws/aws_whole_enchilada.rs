use crate::helpers;
use crate::helpers::common::{ClusterDomain, NodeManager};
use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use crate::helpers::utilities::{
    FuncTestsSecrets, context_for_cluster, engine_run_test, generate_cluster_id, generate_id, logger, metrics_registry,
};
use ::function_name::named;
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::io_models::models::CpuArchitecture;
use qovery_engine::io_models::models::VpcQoveryNetworkMode::WithNatGateways;
use qovery_engine::utilities::to_short_id;
use std::str::FromStr;

#[cfg(feature = "test-aws-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets
        .AWS_DEFAULT_REGION
        .as_ref()
        .expect("AWS region was not found in secrets");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::Classic,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada")]
#[named]
#[test]
fn create_resize_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets
        .AWS_DEFAULT_REGION
        .as_ref()
        .expect("AWS region was not found in secrets");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to convert the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithNodesResize,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
            None,
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_pause_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets.AWS_DEFAULT_REGION.as_ref().expect("AWS region was not found");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithPause,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets.AWS_DEFAULT_REGION.as_ref().expect("AWS region was not found");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithUpgrade,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Default,
            vec![],
        )
    })
}
