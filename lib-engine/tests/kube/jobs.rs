use crate::helpers::common::Infrastructure;
use crate::helpers::utilities::{engine_run_test, init};
use crate::kube::{kube_test_env, TestEnvOption};
use function_name::named;
use qovery_engine::io_models::container::Registry;
use qovery_engine::io_models::job::{JobSchedule, JobSource};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::transaction::TransactionResult;
use std::collections::BTreeMap;
use tracing::{span, Level};
use url::Url;
use uuid::Uuid;

#[cfg(feature = "test-aws-minimal")]
#[test]
#[named]
fn should_have_mounted_files_as_volume() {
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        // setup:
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let (infra_ctx, environment) = kube_test_env(TestEnvOption::WithJob);
        let mut ea = environment.clone();
        let mut cron_job = environment.jobs.first().expect("there is no job in env").clone();

        // removing useless objects for this test
        ea.containers = vec![];
        ea.databases = vec![];
        ea.applications = vec![];
        ea.routers = vec![];

        // setup mounted file for this app
        let mounted_file_id = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_id.short().to_string(),
            long_id: mounted_file_id.to_uuid(),
            mount_path: "/tmp/app.config.json".to_string(),
            file_content_b64: base64::encode(r#"{"name": "config"}"#),
        };
        let mount_file_env_var_key = "APP_CONFIG";
        let mount_file_env_var_value = mounted_file.mount_path.to_string();

        // Use an app crashing in case file doesn't exists
        cron_job.command_args = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "apt-get update; apt-get install -y netcat; echo listening on port $PORT; env; test -f $APP_CONFIG; timeout 15 nc -l 8080; exit 0;"
                .to_string(),
        ];
        cron_job.force_trigger = true;
        cron_job.schedule = JobSchedule::Cron {
            schedule: "*/30 * * * *".to_string(), // <- every 30 minutes
        };
        cron_job.source = JobSource::Image {
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.aws").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
        };
        cron_job.max_nb_restart = 1;
        cron_job.max_duration_in_sec = 120;
        cron_job.mounted_files = vec![mounted_file];
        cron_job.environment_vars_with_infos = BTreeMap::from([
            (
                mount_file_env_var_key.to_string(),
                VariableInfo {
                    value: base64::encode(mount_file_env_var_value),
                    is_secret: false,
                },
            ), // <- mounted file PATH
        ]);

        // create a job
        let mut job = cron_job.clone();
        let job_id = QoveryIdentifier::new_random();
        job.name = job_id.short().to_string();
        job.long_id = job_id.to_uuid();
        job.force_trigger = true;
        job.schedule = JobSchedule::OnStart {};

        // attaching job to env
        ea.jobs = vec![cron_job, job];

        // execute & verify
        let deployment_result = environment.deploy_environment(&ea, &infra_ctx);

        // verify:
        assert!(matches!(deployment_result, TransactionResult::Ok));

        // clean up:
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
