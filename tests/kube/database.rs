use crate::helpers::common::Infrastructure;
use crate::helpers::database::StorageSize::Resize;
use crate::helpers::utilities::engine_run_test;
use crate::kube::{TestEnvOption, kube_test_env};
use function_name::named;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use qovery_engine::environment::action::update_pvcs;
use qovery_engine::environment::models::abort::AbortStatus;
use qovery_engine::environment::models::database::{
    Container, Database, PostgresSQL, get_database_with_invalid_storage_size,
};
use qovery_engine::environment::models::types::{AWS, VersionsNumber};
use qovery_engine::infrastructure::models::cloud_provider::DeploymentTarget;
use qovery_engine::infrastructure::models::cloud_provider::service::{DatabaseType, ServiceType};
use qovery_engine::io_models::Action;
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::database::{DatabaseOptions, DiskIOPS};
use qovery_engine::kubers_utils::kube_get_resources_by_selector;
use qovery_engine::runtime::block_on;
use std::str::FromStr;
use tracing::{Level, span};

#[cfg(feature = "test-aws-self-hosted")]
#[test]
#[named]
fn should_increase_db_storage_size() {
    let test_name = function_name!();

    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let (infra_ctx, environment) = kube_test_env(TestEnvOption::WithDB);
        let ea = environment.clone();

        assert!(environment.deploy_environment(&ea, &infra_ctx).is_ok());

        let mut resized_env = environment.clone();
        resized_env.databases[0].disk_size_in_gib = Resize.size();
        let resized_db = &resized_env.databases[0];

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
        let test_db = &test_env.databases[0];

        let db: Database<AWS, Container, PostgresSQL> = Database::new(
            &resized_context,
            resized_db.long_id,
            *test_db.action(),
            resized_db.name.as_str(),
            resized_db.name.clone(),
            VersionsNumber::from_str(&resized_db.version).expect("Unable to parse db version"),
            resized_db.created_at,
            &resized_db.fqdn,
            &resized_db.fqdn_id,
            resized_db.cpu_request_in_milli,
            resized_db.cpu_limit_in_milli,
            resized_db.ram_request_in_mib,
            resized_db.ram_limit_in_mib,
            resized_db.disk_size_in_gib,
            None,
            resized_db.publicly_accessible,
            resized_db.port,
            DatabaseOptions {
                login: resized_db.username.to_string(),
                password: resized_db.password.to_string(),
                host: resized_db.fqdn.to_string(),
                port: resized_db.port,
                mode: resized_db.mode.clone(),
                disk_size_in_gib: resized_db.disk_size_in_gib,
                database_disk_type: resized_db.database_disk_type.to_string(),
                database_disk_iops: match resized_db.database_disk_iops {
                    Some(iops) => DiskIOPS::Provisioned(iops),
                    None => DiskIOPS::Default,
                },
                encrypt_disk: resized_db.encrypt_disk,
                activate_high_availability: resized_db.activate_high_availability,
                activate_backups: resized_db.activate_backups,
                publicly_accessible: resized_db.publicly_accessible,
            },
            |transmitter| infra_ctx.context().get_event_details(transmitter),
            vec![],
            vec![],
            vec![],
        )
        .expect("Unable to create database");

        let invalid_statefulset = match get_database_with_invalid_storage_size(
            &db,
            &deployment_target.kube,
            deployment_target.environment.namespace(),
            deployment_target.environment.event_details(),
        ) {
            Ok(result) => match result {
                Some(invalid_storage) => {
                    assert_eq!(invalid_storage.service_type, ServiceType::Database(DatabaseType::PostgreSQL));
                    assert_eq!(invalid_storage.service_id, test_db.long_id().clone());
                    assert_eq!(invalid_storage.invalid_pvcs.len(), 1);
                    assert_eq!(invalid_storage.invalid_pvcs[0].required_disk_size_in_gib, Resize.size());
                    invalid_storage
                }
                None => panic!("No invalid storage returned"),
            },
            Err(e) => panic!("No invalid storage returned: {e}"),
        };

        let ret = update_pvcs(
            test_db.as_service(),
            &invalid_statefulset,
            test_env.namespace(),
            test_env.event_details(),
            &deployment_target.kube,
        );
        assert!(ret.is_ok());

        //assert app can be redeployed
        let rea = resized_env.clone();
        assert!(resized_env.deploy_environment(&rea, &infra_ctx).is_ok());

        // assert edited storage have good size
        let pvcs = match block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
            &deployment_target.kube,
            deployment_target.environment.namespace(),
            &format!("app={}", invalid_statefulset.statefulset_name),
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
