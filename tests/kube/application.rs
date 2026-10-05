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
use qovery_engine::environment::models::aws::AwsAppExtraSettings;
use qovery_engine::environment::models::types::AWS;
use qovery_engine::infrastructure::models::cloud_provider::DeploymentTarget;
use qovery_engine::infrastructure::models::cloud_provider::service::ServiceType;
use qovery_engine::infrastructure::models::container_registry::InteractWithRegistry;
use qovery_engine::infrastructure::models::kubernetes::aws::AwsStorageType;
use qovery_engine::io_models::application::PortIo;
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
            long_id: mounted_file_id.to_uuid(),
            kube_name: mounted_file_id.short().to_string(),
            mount_path: "/tmp/app.config.json".to_string(),
            file_content_b64: general_purpose::STANDARD.encode(r#"{"name": "config"}"#),
        };
        let mount_file_env_var_key = "APP_CONFIG";
        let mount_file_env_var_value = mounted_file.mount_path.to_string();

        // Use an app crashing in case file doesn't exists
        application.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
        application.branch = "app-crashing-if-file-doesnt-exist".to_string();
        application.commit_id = "7e86c06be6c985a41d14e720843b62e67eb110e2".to_string();
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
        let id = QoveryIdentifier::new_random();
        statefulset.mounted_files[0].long_id = id.to_uuid();
        statefulset.mounted_files[0].kube_name = id.short().to_string();
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
