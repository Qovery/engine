use ::function_name::named;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode::WithNatGateways;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::EnvironmentAction;
use test_utilities::aws::{AWS_KUBERNETES_MAJOR_VERSION, AWS_KUBERNETES_MINOR_VERSION};
use test_utilities::common::{cluster_test, ClusterDomain, ClusterTestType};
use test_utilities::utilities::{context, engine_run_test, generate_cluster_id, generate_id, FuncTestsSecrets};

#[cfg(feature = "test-aws-whole-enchilada")]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let region = "eu-west-3";
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(region);
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = test_utilities::common::working_minimal_environment(
        &context,
        organization_id.as_str(),
        cluster_domain.as_str(),
    );
    let env_action = EnvironmentAction::Environment(environment.clone());

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            context.clone(),
            region,
            secrets.clone(),
            ClusterTestType::Classic,
            AWS_KUBERNETES_MAJOR_VERSION,
            AWS_KUBERNETES_MINOR_VERSION,
            ClusterDomain::Custom(cluster_domain),
            Some(WithNatGateways),
            Some(&env_action),
        )
    })
}
