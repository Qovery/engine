use crate::helpers;
use crate::helpers::common::{ActionableFeature, Infrastructure, PUB_MIRROR_DEBIAN_TAG};
use crate::helpers::kubernetes::TargetCluster;
use crate::helpers::scaleway::scw_infra_config_with_features;
use crate::helpers::utilities::{FuncTestsSecrets, context_for_resource, engine_run_test, logger, metrics_registry};
use ::function_name::named;
use k8s_openapi::api::apps::v1::Deployment;
use kube::Api;
use kube::api::ListParams;
use qovery_engine::cmd::command::CommandKiller;
use qovery_engine::cmd::docker::ContainerImage;
use qovery_engine::errors::Tag;
use qovery_engine::infrastructure::models::container_registry::github_cr::GithubCr;
use qovery_engine::infrastructure::models::container_registry::{InteractWithRegistry, RegistryTags};
use qovery_engine::io_models::Action;
use qovery_engine::io_models::application::{PortIo, Protocol};
use qovery_engine::io_models::container::{Container, Credentials, Registry};
use qovery_engine::io_models::context::{CloneForTest, Context};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::runtime::block_on;
use std::collections::{BTreeMap, BTreeSet};
use tracing::{Level, span};
use url::Url;
use uuid::Uuid;

const GHCR_ORGANIZATION: &str = "qovery";
const PRIVATE_IMAGE_TAG: &str = "qovery-mirroring-disabled";

fn scw_test_context() -> (Context, TargetCluster) {
    let secrets = FuncTestsSecrets::new();
    let context = context_for_resource(
        secrets
            .SCALEWAY_TEST_ORGANIZATION_LONG_ID
            .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
        secrets
            .SCALEWAY_TEST_CLUSTER_LONG_ID
            .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
    );
    let target_cluster = TargetCluster::MutualizedTestCluster {
        kubeconfig: secrets
            .SCALEWAY_TEST_KUBECONFIG_b64
            .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
            .to_string(),
    };
    (context, target_cluster)
}

fn a_container(registry: Registry, image: String, tag: String, public_domain: String) -> Container {
    Container {
        long_id: Uuid::new_v4(),
        name: "mirroring disabled".to_string(),
        kube_name: "mirroring-disabled".to_string(),
        action: Action::Create,
        registry,
        image,
        tag,
        command_args: vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            r#"
            set -e; apt-get update;
            apt-get install -y socat;
            socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
            "#
            .to_string(),
        ],
        entrypoint: None,
        cpu_request_in_milli: 250,
        cpu_limit_in_milli: Some(250),
        ram_request_in_mib: 250,
        ram_limit_in_mib: 250,
        gpu_request: None,
        gpu_limit: None,
        ephemeral_storage_in_gib: None,
        min_instances: 1,
        max_instances: 1,
        public_domain,
        ports: vec![PortIo {
            long_id: Uuid::new_v4(),
            port: 8080,
            name: "http".to_string(),
            is_default: true,
            publicly_accessible: false,
            protocol: Protocol::HTTP,
            service_name: None,
            namespace: None,
            path: None,
            path_rewrite: None,
        }],
        readiness_probe: Some(Probe {
            r#type: ProbeType::Tcp { host: None },
            port: 8080,
            initial_delay_seconds: 1,
            timeout_seconds: 2,
            period_seconds: 3,
            success_threshold: 1,
            failure_threshold: 5,
        }),
        liveness_probe: None,
        storages: vec![],
        environment_vars_with_infos: BTreeMap::new(),
        mounted_files: vec![],
        advanced_settings: Default::default(),
        annotations_group_ids: BTreeSet::new(),
        labels_group_ids: BTreeSet::new(),
        autoscaling: None,
        external_secrets: BTreeMap::new(),
        cpu_architecture: None,
    }
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_pulled_directly_from_private_ghcr_when_mirroring_disabled_on_scw() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let github_token = secrets.GITHUB_ACCESS_TOKEN.clone().expect("GITHUB_ACCESS_TOKEN");
        let (context, target_cluster) = scw_test_context();
        let infra_ctx = scw_infra_config_with_features(
            &target_cluster,
            &context,
            logger.clone(),
            metrics_registry.clone(),
            vec![ActionableFeature::DisabledRegistryMirroring],
        );
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config_with_features(
            &target_cluster,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
            vec![ActionableFeature::DisabledRegistryMirroring],
        );

        // given: a private GHCR package only readable with the token
        let ghcr_registry_name = format!("test-{}", Uuid::new_v4());
        let ghcr = GithubCr::new(
            context.clone(),
            Uuid::new_v4(),
            ghcr_registry_name.as_str(),
            Url::parse("https://ghcr.io").unwrap(),
            GHCR_ORGANIZATION.to_string(),
            github_token.clone(),
        )
        .expect("Cannot instantiate GHCR registry");
        let package = Uuid::new_v4().to_string();
        let image_name = ghcr.registry_info().get_image_name(&package);
        ghcr.create_repository(
            Some(ghcr_registry_name.as_str()),
            ghcr.registry_info().get_repository_name(&package).as_str(),
            0,
            RegistryTags {
                cluster_id: None,
                environment_id: None,
                project_id: None,
                resource_ttl: None,
            },
        )
        .expect("Cannot create GHCR package");
        context
            .docker
            .mirror(
                &ContainerImage::new(
                    Url::parse("https://public.ecr.aws/").unwrap(),
                    "r3m4q3r9/pub-mirror-debian".to_string(),
                    vec![PUB_MIRROR_DEBIAN_TAG.to_string()],
                ),
                &ContainerImage::new(
                    ghcr.get_registry_endpoint(None),
                    image_name.clone(),
                    vec![PRIVATE_IMAGE_TAG.to_string()],
                ),
                &mut |line| println!("[docker mirror stdout] {line}"),
                &mut |line| eprintln!("[docker mirror stderr] {line}"),
                &CommandKiller::never(),
            )
            .expect("Cannot push the test image to GHCR");

        let mut environment = helpers::environment::working_minimal_environment(&context);
        environment.routers = vec![];
        environment.applications = vec![];
        let container = a_container(
            Registry::GenericCr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://ghcr.io").unwrap(),
                credentials: Some(Credentials {
                    login: GHCR_ORGANIZATION.to_string(),
                    password: github_token,
                }),
            },
            image_name.clone(),
            PRIVATE_IMAGE_TAG.to_string(),
            format!("{}.{}", Uuid::new_v4(), infra_ctx.dns_provider().domain()),
        );
        let container_id = container.long_id;
        environment.containers = vec![container];
        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // when
        let ret = environment.deploy_environment(&environment, &infra_ctx);

        let pod_spec = infra_ctx.mk_kube_client().ok().and_then(|kube_client| {
            let deployments: Api<Deployment> = Api::namespaced(kube_client.client(), environment.kube_name.as_str());
            block_on(async {
                deployments
                    .list(&ListParams::default().labels(&format!("qovery.com/service-id={container_id}")))
                    .await
            })
            .ok()?
            .items
            .into_iter()
            .next()?
            .spec?
            .template
            .spec
        });

        // clean up before asserting, so a failure doesn't leave a namespace and a private GHCR package behind
        let delete = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        let package_delete = ghcr.delete_repository(&image_name);

        // then: pods run the GHCR image itself, with a pull secret
        assert!(ret.is_ok(), "deployment failed: {ret:?}");
        let pod_spec = pod_spec.expect("No deployment pod spec for the container");
        assert_eq!(
            pod_spec.containers[0].image.as_deref(),
            Some(format!("ghcr.io/{image_name}:{PRIVATE_IMAGE_TAG}").as_str())
        );
        assert!(
            pod_spec.image_pull_secrets.is_some_and(|secrets| !secrets.is_empty()),
            "pods pull a private image, they need a pull secret"
        );
        assert!(delete.is_ok(), "environment deletion failed: {delete:?}");
        package_delete.expect("Cannot delete the GHCR package");

        function_name!().to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_fails_with_source_image_not_found_when_mirroring_disabled_on_scw() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let (context, target_cluster) = scw_test_context();
        let infra_ctx = scw_infra_config_with_features(
            &target_cluster,
            &context,
            logger.clone(),
            metrics_registry.clone(),
            vec![ActionableFeature::DisabledRegistryMirroring],
        );
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config_with_features(
            &target_cluster,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
            vec![ActionableFeature::DisabledRegistryMirroring],
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);
        environment.routers = vec![];
        environment.applications = vec![];
        environment.containers = vec![a_container(
            Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.aws").unwrap(),
            },
            "r3m4q3r9/pub-mirror-debian".to_string(),
            "tag-that-does-not-exist".to_string(),
            format!("{}.{}", Uuid::new_v4(), infra_ctx.dns_provider().domain()),
        )];
        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // when
        let ret = environment.deploy_environment(&environment, &infra_ctx);

        // then
        let err = ret.expect_err("deployment of a missing image must fail");
        assert_eq!(err.tag(), &Tag::SourceImageNotFound, "unexpected error: {err:?}");

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, Ok(_) | Err(_)));

        function_name!().to_string()
    })
}
