use crate::helpers::common::Infrastructure;
use crate::helpers::database::StorageSize::Resize;
use crate::helpers::utilities::{engine_run_test, init};
use crate::kube::{kube_test_env, TestEnvOption};
use function_name::named;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use qovery_engine::cloud_provider::service::{DatabaseType, ServiceType};
use qovery_engine::cloud_provider::utilities::update_pvcs;
use qovery_engine::cloud_provider::DeploymentTarget;
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::database::DatabaseOptions;
use qovery_engine::io_models::Action;
use qovery_engine::kubers_utils::kube_get_resources_by_selector;
use qovery_engine::models::abort::AbortStatus;
use qovery_engine::models::database::{get_database_with_invalid_storage_size, Container, Database, PostgresSQL};
use qovery_engine::models::types::{VersionsNumber, AWS};
use qovery_engine::runtime::block_on;
use qovery_engine::transaction::TransactionResult;
use std::str::FromStr;
use tracing::{span, Level};

#[cfg(feature = "test-aws-self-hosted")]
#[test]
#[named]
fn should_increase_db_storage_size() {
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let (infra_ctx, environment) = kube_test_env(TestEnvOption::WithDB);
        let ea = environment.clone();

        assert!(matches!(environment.deploy_environment(&ea, &infra_ctx), TransactionResult::Ok));

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
                encrypt_disk: resized_db.encrypt_disk,
                activate_high_availability: resized_db.activate_high_availability,
                activate_backups: resized_db.activate_backups,
                publicly_accessible: resized_db.publicly_accessible,
            },
            |transmitter| infra_ctx.context().get_event_details(transmitter),
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
        assert!(matches!(
            resized_env.deploy_environment(&rea, &infra_ctx),
            TransactionResult::Ok
        ));

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
        assert!(matches!(
            env_to_delete.delete_environment(&ead, &infra_ctx),
            TransactionResult::Ok
        ));

        test_name.to_string()
    });
}
