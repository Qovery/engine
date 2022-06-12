use ::function_name::named;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::scaleway::ScwZone;
use test_utilities::common::{cluster_test, ClusterDomain, ClusterTestType};

use test_utilities::scaleway::{SCW_KUBERNETES_MAJOR_VERSION, SCW_KUBERNETES_MINOR_VERSION};
use test_utilities::utilities::{context, engine_run_test, generate_cluster_id, generate_id, logger, FuncTestsSecrets};

#[cfg(feature = "test-scw-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_with_env_in_par_2() {
    let logger = logger();
    let zone = ScwZone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context(organization_id.as_str(), cluster_id.as_str());
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = test_utilities::common::working_minimal_environment(&context, cluster_domain.as_str());
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            zone.as_str(),
            None,
            ClusterTestType::Classic,
            SCW_KUBERNETES_MAJOR_VERSION,
            SCW_KUBERNETES_MINOR_VERSION,
            &ClusterDomain::Custom(cluster_domain),
            None,
            Some(&env_action),
        )
    })
}

#[cfg(feature = "test-scw-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_pause_and_destroy_kapsule_cluster_with_env_in_par_2() {
    let logger = logger();
    let zone = ScwZone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context(organization_id.as_str(), cluster_id.as_str());
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = test_utilities::common::working_minimal_environment(&context, cluster_domain.as_str());
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            zone.as_str(),
            None,
            ClusterTestType::WithPause,
            SCW_KUBERNETES_MAJOR_VERSION,
            SCW_KUBERNETES_MINOR_VERSION,
            &ClusterDomain::Custom(cluster_domain),
            None,
            Some(&env_action),
        )
    })
}

#[cfg(feature = "test-scw-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_upgrade_and_destroy_kapsule_cluster_with_env_in_par_2() {
    let logger = logger();
    let zone = ScwZone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context(organization_id.as_str(), cluster_id.as_str());
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = test_utilities::common::working_minimal_environment(&context, cluster_domain.as_str());
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            zone.as_str(),
            None,
            ClusterTestType::WithUpgrade,
            SCW_KUBERNETES_MAJOR_VERSION,
            SCW_KUBERNETES_MINOR_VERSION,
            &ClusterDomain::Custom(cluster_domain),
            None,
            Some(&env_action),
        )
    })
}
