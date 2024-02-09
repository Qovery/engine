use crate::helpers;
use crate::helpers::common::ClusterDomain;
use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use ::function_name::named;
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::models::CpuArchitecture;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::ToCloudProviderFormat;
use qovery_engine::utilities::to_short_id;

use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_id, logger, metrics_registry, FuncTestsSecrets,
};

#[cfg(feature = "test-gcp-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_with_env_in_europe_west9() {
    let logger = logger();
    let metrics_registry = metrics_registry();
    let region = GcpRegion::EuropeWest9;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(region.to_cloud_provider_format());
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
            Kind::Gcp,
            KKind::Gke,
            context.clone(),
            logger,
            metrics_registry,
            region.to_cloud_provider_format(),
            None,
            ClusterTestType::Classic,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
            Some(&env_action),
        )
    })
}
