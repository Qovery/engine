use ::function_name::named;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::models::{Context, EnvironmentAction};
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use test_utilities::cloudflare::{dns_provider_cloudflare, CloudflareDomain};
use test_utilities::common::deploy_upgrade_destroy_infra_and_env;
use test_utilities::utilities::{context, engine_run_test, generate_cluster_id, generate_id, init, FuncTestsSecrets};
use tracing::{span, Level};

fn create_upgrade_and_destroy_eks_cluster_and_env(
    context: Context,
    cluster_id: &str,
    cluster_domain: &str,
    region: &str,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    _upgrade_to_version: &str,
    environment_action: EnvironmentAction,
    test_name: &str,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let aws_provider = test_utilities::aws::cloud_provider_aws(&context);
        let nodes = test_utilities::aws::aws_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, CloudflareDomain::Custom(cluster_domain.to_string()));

        let eks = EKS::new(
            context,
            cluster_id,
            uuid::Uuid::new_v4(),
            cluster_id,
            boot_version,
            region,
            &aws_provider,
            &cloudflare,
            test_utilities::aws::eks_options(secrets),
            nodes,
        )
        .unwrap();

        deploy_upgrade_destroy_infra_and_env(&mut tx, &eks, &environment_action);

        test_name.to_string()
    });
}

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

    create_upgrade_and_destroy_eks_cluster_and_env(
        context,
        cluster_id.as_str(),
        cluster_domain.as_str(),
        region,
        secrets,
        AWS_KUBERNETES_VERSION,
        "1.19",
        env_action,
        function_name!(),
    );
}
