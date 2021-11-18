use ::function_name::named;
use qovery_engine::cloud_provider::digitalocean::application::Region;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::models::{Context, EnvironmentAction};
use test_utilities::cloudflare::{dns_provider_cloudflare, CloudflareDomain};
use test_utilities::common::deploy_upgrade_destroy_infra_and_env;
use test_utilities::digitalocean::DO_KUBERNETES_VERSION;
use test_utilities::utilities::{context, engine_run_test, generate_cluster_id, generate_id, init, FuncTestsSecrets};
use tracing::{span, Level};

fn create_upgrade_and_destroy_doks_cluster_and_env(
    context: Context,
    cluster_id: &str,
    cluster_domain: &str,
    region: Region,
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

        let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_provider = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
        let nodes = test_utilities::digitalocean::do_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, CloudflareDomain::Custom(cluster_domain.to_string()));

        let doks = DOKS::new(
            context,
            cluster_id.to_string(),
            uuid::Uuid::new_v4(),
            cluster_id.to_string(),
            boot_version.to_string(),
            region,
            &do_provider,
            &cloudflare,
            nodes,
            test_utilities::digitalocean::do_kubernetes_cluster_options(secrets, cluster_id.to_string()),
        )
        .unwrap();

        deploy_upgrade_destroy_infra_and_env(&mut tx, &doks, &environment_action);

        test_name.to_string()
    });
}

#[cfg(feature = "test-do-whole-enchilada")]
#[named]
#[test]
fn create_upgrade_and_destroy_doks_cluster_with_env_in_ams_3() {
    let context = context();
    let region = Region::Amsterdam3;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(region.as_str());
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

    create_upgrade_and_destroy_doks_cluster_and_env(
        context,
        cluster_id.as_str(),
        cluster_domain.as_str(),
        region,
        secrets,
        DO_KUBERNETES_VERSION,
        "1.20",
        env_action,
        function_name!(),
    );
}
