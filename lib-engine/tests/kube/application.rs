use crate::helpers::common::Infrastructure;
use crate::helpers::database::StorageSize::Resize;
use crate::helpers::utilities::engine_run_test;
use crate::kube::{TestEnvOption, kube_test_env};
use base64::Engine;
use base64::engine::general_purpose;
use function_name::named;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use qovery_engine::environment::action::update_pvcs;
use qovery_engine::environment::models::abort::AbortStatus;
use qovery_engine::environment::models::application::{Application, get_application_with_invalid_storage_size};
use qovery_engine::environment::models::aws::{AwsAppExtraSettings, AwsStorageType};
use qovery_engine::environment::models::types::AWS;
use qovery_engine::infrastructure::models::cloud_provider::DeploymentTarget;
use qovery_engine::infrastructure::models::cloud_provider::service::ServiceType;
use qovery_engine::infrastructure::models::container_registry::InteractWithRegistry;
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::models::{
    EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit, Storage,
};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::kubers_utils::kube_get_resources_by_selector;
use qovery_engine::runtime::block_on;
use std::collections::{BTreeMap, BTreeSet};
use tracing::{Level, span};

#[cfg(feature = "test-aws-self-hosted")]
#[test]
#[named]
fn should_increase_app_storage_size() {
    let test_name = function_name!();

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let (infra_ctx, environment) = kube_test_env(TestEnvOption::WithApp);
        let ea = environment.clone();

        assert!(environment.deploy_environment(&ea, &infra_ctx).is_ok());

        let mut resized_env = environment.clone();
        resized_env.applications[0].storage[0].size_in_gib = Resize.size();
        let resized_app = &resized_env.applications[0];

        let resized_context = infra_ctx.context().clone_not_same_execution_id();
        let test_env = resized_env
            .to_environment_domain(
                &resized_context,
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();
        let deployment_target = DeploymentTarget::new(&infra_ctx, &test_env, &|| AbortStatus::None).unwrap();
        let test_app = &test_env.applications[0];

        let storages = resized_app
            .storage
            .iter()
            .map(|storage| storage.to_storage())
            .collect::<Vec<Storage>>();

        let envs = resized_app
            .environment_vars_with_infos
            .iter()
            .map(|(k, variable_infos)| EnvironmentVariable {
                key: k.to_string(),
                value: variable_infos.value.to_string(),
                is_secret: variable_infos.is_secret,
            })
            .collect::<Vec<EnvironmentVariable>>();
        let app: Application<AWS> = Application::new(
            &resized_context,
            resized_app.long_id,
            *test_app.action(),
            resized_app.name.as_str(),
            resized_app.name.clone(),
            resized_app.public_domain.clone(),
            resized_app.ports.clone(),
            resized_app.min_instances,
            resized_app.max_instances,
            resized_app.to_build(
                infra_ctx.container_registry().registry_info(),
                infra_ctx.context().qovery_api.clone(),
                infra_ctx.kubernetes().cpu_architectures(),
                &QoveryIdentifier::new(*infra_ctx.kubernetes().long_id()),
            ),
            resized_app.command_args.clone(),
            resized_app.entrypoint.clone(),
            storages,
            envs,
            BTreeSet::default(),
            resized_app.readiness_probe.clone().map(|p| p.to_domain()),
            resized_app.liveness_probe.clone().map(|p| p.to_domain()),
            resized_app.advanced_settings.clone(),
            AwsAppExtraSettings {},
            |transmitter| infra_ctx.context().get_event_details(transmitter),
            vec![],
            vec![],
            KubernetesCpuResourceUnit::MilliCpu(resized_app.cpu_request_in_milli),
            KubernetesCpuResourceUnit::MilliCpu(resized_app.cpu_limit_in_milli),
            KubernetesMemoryResourceUnit::MebiByte(resized_app.ram_request_in_mib),
            KubernetesMemoryResourceUnit::MebiByte(resized_app.ram_limit_in_mib),
            true,
        )
        .expect("Unable to create application");

        let invalid_statefulset = match get_application_with_invalid_storage_size(
            &app,
            &deployment_target.kube,
            deployment_target.environment.namespace(),
            deployment_target.environment.event_details(),
        ) {
            Ok(result) => match result {
                Some(invalid_storage) => {
                    assert_eq!(invalid_storage.service_type, ServiceType::Application);
                    assert_eq!(invalid_storage.service_id, test_app.long_id().clone());
                    assert_eq!(invalid_storage.invalid_pvcs.len(), 1);
                    assert_eq!(invalid_storage.invalid_pvcs[0].required_disk_size_in_gib, Resize.size());
                    assert!(
                        invalid_storage.invalid_pvcs[0]
                            .pvc_name
                            .starts_with(&resized_env.applications[0].storage[0].id)
                    );
                    invalid_storage
                }
                None => panic!("No invalid storage returned"),
            },
            Err(e) => panic!("No invalid storage returned: {e}"),
        };

        let ret = update_pvcs(
            test_app.as_service(),
            &invalid_statefulset,
            test_env.namespace(),
            test_env.event_details(),
            &deployment_target.kube,
        );
        assert!(ret.is_ok());

        // assert app can be redeployed
        let rea = resized_env.clone();
        assert!(resized_env.deploy_environment(&rea, &infra_ctx).is_ok());

        // assert edited storage have good size
        let pvcs = match block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
            &deployment_target.kube,
            deployment_target.environment.namespace(),
            &format!("appId={}", test_app.id()),
        )) {
            Ok(result) => result.items,
            Err(_) => panic!("Unable to get pvcs"),
        };

        let pvc = pvcs
            .iter()
            .find(|pvc| match &pvc.metadata.name {
                Some(name) => *name.to_string() == invalid_statefulset.invalid_pvcs[0].pvc_name,
                None => false,
            })
            .expect("Unable to get pvc");

        if let Some(spec) = &pvc.spec {
            if let Some(resources) = &spec.resources {
                if let Some(req) = &resources.requests {
                    assert_eq!(
                        req["storage"].0,
                        format!("{}Gi", invalid_statefulset.invalid_pvcs[0].required_disk_size_in_gib)
                    )
                }
            }
        }

        // clean up
        let mut env_to_delete = environment;
        env_to_delete.action = Action::Delete;
        let ead = env_to_delete.clone();
        assert!(env_to_delete.delete_environment(&ead, &infra_ctx).is_ok());

        test_name.to_string()
    });
}

#[cfg(feature = "test-aws-minimal")]
#[test]
#[named]
fn should_have_mounted_files_as_volume() {
    let test_name = function_name!();

    engine_run_test(|| {
        // setup:
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let (infra_ctx, environment) = kube_test_env(TestEnvOption::WithApp);
        let mut ea = environment.clone();
        let mut application = environment
            .applications
            .first()
            .expect("there is no application in env")
            .clone();

        // removing useless objects for this test
        ea.containers = vec![];
        ea.databases = vec![];
        ea.jobs = vec![];
        ea.routers = vec![];

        // setup mounted file for this app
        let mounted_file_id = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_id.short().to_string(),
            long_id: mounted_file_id.to_uuid(),
            mount_path: "/tmp/app.config.json".to_string(),
            file_content_b64: general_purpose::STANDARD.encode(r#"{"name": "config"}"#),
        };
        let mount_file_env_var_key = "APP_CONFIG";
        let mount_file_env_var_value = mounted_file.mount_path.to_string();

        // Use an app crashing in case file doesn't exists
        application.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
        application.branch = "app-crashing-if-file-doesnt-exist".to_string();
        application.commit_id = "44b889f36c81cce7dee678993bb7986c86899e5d".to_string();
        application.ports = vec![];
        application.mounted_files = vec![mounted_file];
        application.readiness_probe = None;
        application.liveness_probe = None;
        application.environment_vars_with_infos = BTreeMap::from([
            (
                "APP_FILE_PATH_TO_BE_CHECKED".to_string(),
                VariableInfo {
                    value: general_purpose::STANDARD.encode(&mount_file_env_var_value),
                    is_secret: false,
                },
            ), // <- https://github.com/Qovery/engine-testing/blob/app-crashing-if-file-doesnt-exist/src/main.rs#L19
            (
                mount_file_env_var_key.to_string(),
                VariableInfo {
                    value: general_purpose::STANDARD.encode(&mount_file_env_var_value),
                    is_secret: false,
                },
            ), // <- mounted file PATH
        ]);

        // create a statefulset
        let mut statefulset = application.clone();
        let statefulset_id = QoveryIdentifier::new_random();
        statefulset.name = statefulset_id.short().to_string();
        statefulset.kube_name.clone_from(&statefulset.name);
        statefulset.long_id = statefulset_id.to_uuid();
        let storage_id = QoveryIdentifier::new_random();
        statefulset.readiness_probe = None;
        statefulset.liveness_probe = None;
        statefulset.storage = vec![qovery_engine::io_models::application::Storage {
            id: storage_id.short().to_string(),
            long_id: storage_id.to_uuid(),
            name: storage_id.short().to_string(),
            storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
            size_in_gib: 10,
            mount_point: format!("/tmp/{}", storage_id.short()),
            snapshot_retention_in_days: 1,
        }];

        // attaching application & statefulset to env
        ea.applications = vec![application, statefulset];

        // execute & verify
        let deployment_result = environment.deploy_environment(&ea, &infra_ctx);

        // verify:
        assert!(deployment_result.is_ok());

        // clean up:
        let mut env_to_delete = environment;
        env_to_delete.action = Action::Delete;
        let ead = env_to_delete.clone();
        assert!(env_to_delete.delete_environment(&ead, &infra_ctx).is_ok());

        test_name.to_string()
    });
}
