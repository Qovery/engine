use crate::helpers;
use crate::helpers::common::ClusterDomain;
use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use ::function_name::named;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::models::CpuArchitecture;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::scaleway::ScwZone;
use qovery_engine::utilities::to_short_id;

use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_id, logger, metrics_registry, FuncTestsSecrets,
};

#[cfg(feature = "test-scw-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_with_env_in_waw_1() {
    let logger = logger();
    let metrics_registry = metrics_registry();
    let zone = ScwZone::Warsaw1;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context_for_cluster(organization_id, cluster_id, None);
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
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            metrics_registry,
            zone.as_str(),
            None,
            ClusterTestType::Classic,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
            Some(&env_action),
        )
    })
}

#[cfg(feature = "test-scw-whole-enchilada")]
#[named]
#[ignore]
#[test]
fn create_and_destroy_kapsule_cluster_with_env_in_par_2() {
    let logger = logger();
    let metrics_registry = metrics_registry();
    let zone = ScwZone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context_for_cluster(organization_id, cluster_id, None);
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
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            metrics_registry,
            zone.as_str(),
            None,
            ClusterTestType::Classic,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
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
    let metrics_registry = metrics_registry();
    let zone = ScwZone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context_for_cluster(organization_id, cluster_id, None);
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
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            metrics_registry,
            zone.as_str(),
            None,
            ClusterTestType::WithPause,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
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
    let metrics_registry = metrics_registry();
    let zone = ScwZone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let context = context_for_cluster(organization_id, cluster_id, None);
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
            Kind::Scw,
            KKind::ScwKapsule,
            context.clone(),
            logger,
            metrics_registry,
            zone.as_str(),
            None,
            ClusterTestType::WithUpgrade,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
            Some(&env_action),
        )
    })
}
