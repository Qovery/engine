use crate::helpers;
use crate::helpers::common::ClusterDomain;
use crate::helpers::digitalocean::{DO_KUBERNETES_MAJOR_VERSION, DO_KUBERNETES_MINOR_VERSION};
use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use crate::helpers::utilities::{context, engine_run_test, generate_cluster_id, generate_id, logger, FuncTestsSecrets};
use ::function_name::named;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::digital_ocean::DoRegion;

#[cfg(feature = "test-do-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_doks_cluster_with_env_in_ams_3() {
    let logger = logger();
    let region = DoRegion::Amsterdam3;

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(region.as_str());
    let context = context(organization_id.as_str(), cluster_id.as_str());

    let secrets = FuncTestsSecrets::new();
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
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
            Kind::Do,
            KKind::Doks,
            context.clone(),
            logger,
            region.as_str(),
            None,
            ClusterTestType::Classic,
            DO_KUBERNETES_MAJOR_VERSION,
            DO_KUBERNETES_MINOR_VERSION,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            Some(&env_action),
        )
    })
}

#[cfg(feature = "test-do-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_pause_and_destroy_doks_cluster_with_env_in_ams_3() {
    let logger = logger();
    let region = DoRegion::Amsterdam3;

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(region.as_str());
    let context = context(organization_id.as_str(), cluster_id.as_str());

    let secrets = FuncTestsSecrets::new();
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
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
            Kind::Do,
            KKind::Doks,
            context.clone(),
            logger,
            region.as_str(),
            None,
            ClusterTestType::WithPause,
            DO_KUBERNETES_MAJOR_VERSION,
            DO_KUBERNETES_MINOR_VERSION,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            Some(&env_action),
        )
    })
}

#[cfg(feature = "test-do-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_upgrade_and_destroy_doks_cluster_with_env_in_ams_3() {
    let logger = logger();
    let region = DoRegion::Amsterdam3;

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(region.as_str());
    let context = context(organization_id.as_str(), cluster_id.as_str());

    let secrets = FuncTestsSecrets::new();
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
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
            Kind::Do,
            KKind::Doks,
            context.clone(),
            logger,
            region.as_str(),
            None,
            ClusterTestType::WithUpgrade,
            DO_KUBERNETES_MAJOR_VERSION,
            DO_KUBERNETES_MINOR_VERSION,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            Some(&env_action),
        )
    })
}
