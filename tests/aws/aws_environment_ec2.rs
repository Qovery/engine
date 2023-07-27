use crate::helpers;
use crate::helpers::aws_ec2::aws_ec2_default_infra_config;
use crate::helpers::common::Infrastructure;
use crate::helpers::utilities::{context_for_resource, engine_run_test, init, logger, FuncTestsSecrets};
use ::function_name::named;
use bstr::ByteSlice;
use qovery_engine::cmd::kubectl::kubectl_get_secret;
use qovery_engine::io_models::application::{Port, Protocol};
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::job::{Job, JobSchedule, JobSource};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::transaction::TransactionResult;
use tracing::{span, Level};
use url::Url;
use uuid::Uuid;

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_on_aws_ec2_with_mounted_files_as_volume() {
    // TODO(benjaminch): This test could be moved out of end to end tests as it doesn't require
    // any cloud provider to be performed (can run on local Kubernetes).

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_EC2_TEST_CLUSTER_LONG_ID
                .expect("AWS_EC2_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_ec2_default_infra_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion = aws_ec2_default_infra_config(&context_for_deletion, logger.clone());

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: base64::encode("I exist !"),
        };

        let environment =
            helpers::environment::working_environment_with_application_and_stateful_crashing_if_file_doesnt_exist(
                &context,
                &mounted_file,
            );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // check if secret exists
        let service_id = QoveryIdentifier::new(
            environment
                .applications
                .first()
                .expect("there must be at least one application in environment")
                .long_id,
        )
        .short()
        .to_string();
        let config_maps = kubectl_get_secret(
            infra_ctx.kubernetes().kube_client().expect("kube client is not set"),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                base64::decode(&mounted_file.file_content_b64)
                    .expect("mounted file content cannot be b64 decoded")
                    .to_str(),
                cm.data
                    .expect("data should be set")
                    .get("content")
                    .expect("content should exist")
                    .0
                    .to_str()
            );
        }

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[named]
#[test]
fn deploy_container_on_aws_ec2_with_mounted_files_as_volume() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_EC2_TEST_CLUSTER_LONG_ID
                .expect("AWS_EC2_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_ec2_default_infra_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_ec2_default_infra_config(&context_for_delete, logger.clone());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: base64::encode("I exist !"),
        };

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: "my-little-container".to_string(),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.aws").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    r#"
                apt-get update;
                apt-get install -y socat procps iproute2;
                echo listening on port $PORT;
                env
                cat {}
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#,
                    &mounted_file.mount_path
                ),
            ],
            entrypoint: None,
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    is_default: true,
                    name: "http".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                },
            ],
            storages: vec![],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            environment_vars: btreemap! { "MY_VAR".to_string() => base64::encode("my_value") },
            mounted_files: vec![mounted_file.clone()],
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // check if secret exists
        let service_id = QoveryIdentifier::new(
            environment
                .containers
                .first()
                .expect("there must be at least one container in environment")
                .long_id,
        )
        .short()
        .to_string();
        let config_maps = kubectl_get_secret(
            infra_ctx.kubernetes().kube_client().expect("kube client is not set"),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                base64::decode(&mounted_file.file_content_b64)
                    .expect("mounted file content cannot be b64 decoded")
                    .to_str(),
                cm.data
                    .expect("data should be set")
                    .get("content")
                    .expect("content should exist")
                    .0
                    .to_str()
            );
        }

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-ec2-self-hosted")]
#[test]
fn build_and_deploy_job_on_aws_ec2_with_mounted_files_as_volume() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "build_and_deploy_job_on_aws_eks");
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_EC2_TEST_CLUSTER_LONG_ID
                .expect("AWS_EC2_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_ec2_default_infra_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_ec2_default_infra_config(&context_for_delete, logger.clone());

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist.json".to_string(),
            file_content_b64: base64::encode(
                "{\"foo\": {\"value\": \"bar\", \"sensitive\": true}, \"foo_2\": {\"value\": \"bar_2\"}}",
            ),
        };

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {},
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                git_credentials: None,
                branch: "main".to_string(),
            },
            max_nb_restart: 2,
            max_duration_in_sec: 300,
            default_port: Some(8080),
            //command_args: vec![],
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    "echo starting; sleep 10; cat {} > /qovery-output/qovery-output.json",
                    &mounted_file.mount_path,
                ),
            ],
            entrypoint: None,
            force_trigger: false,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            environment_vars: Default::default(),
            mounted_files: vec![mounted_file.clone()],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // check if secret exists
        let service_id = QoveryIdentifier::new(
            environment
                .jobs
                .first()
                .expect("there must be at least one job in environment")
                .long_id,
        )
        .short()
        .to_string();
        let config_maps = kubectl_get_secret(
            infra_ctx.kubernetes().kube_client().expect("kube client is not set"),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                base64::decode(&mounted_file.file_content_b64)
                    .expect("mounted file content cannot be b64 decoded")
                    .to_str(),
                cm.data
                    .expect("data should be set")
                    .get("content")
                    .expect("content should exist")
                    .0
                    .to_str()
            );
        }

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}
