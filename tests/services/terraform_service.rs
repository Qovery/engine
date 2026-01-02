use crate::helpers::common::Infrastructure;
use crate::helpers::utilities::engine_run_test;
use crate::services::utilities::{
    CloudProvider, TestInfra, create_default_test_annotations, create_default_test_labels,
};
use function_name::named;
use qovery_engine::io_models::Action;
use qovery_engine::io_models::terraform::{
    PersistentStorage, TerraformAction, TerraformActionCommand, TerraformBackend, TerraformBackendType,
    TerraformFilesSource, TerraformProvider, TerraformService,
};
use qovery_engine::io_models::variable_utils::VariableInfo;
use std::collections::BTreeMap;
use tracing::{Level, span};
use url::Url;
use uuid::Uuid;

// Helper struct to build TerraformService test instances with common defaults
struct TerraformServiceTestBuilder {
    service_id: Uuid,
    kube_name: String,
    name: String,
    provider: TerraformProvider,
    provider_version: String,
    environment_vars_with_infos: BTreeMap<String, VariableInfo>,
    extra_action_arguments: BTreeMap<String, Vec<String>>,
}

impl TerraformServiceTestBuilder {
    fn new(service_id: Uuid, kube_name: &str) -> Self {
        Self {
            service_id,
            kube_name: kube_name.to_string(),
            name: "terraform service test #####".to_string(),
            provider: TerraformProvider::Terraform,
            provider_version: "1.9.7".to_string(),
            environment_vars_with_infos: Default::default(),
            extra_action_arguments: BTreeMap::new(),
        }
    }

    fn with_provider(mut self, provider: TerraformProvider, version: &str) -> Self {
        self.provider = provider;
        self.provider_version = version.to_string();
        self
    }

    fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    fn with_env_vars(mut self, vars: BTreeMap<String, VariableInfo>) -> Self {
        self.environment_vars_with_infos = vars;
        self
    }

    fn build(
        &self,
        terraform_action: TerraformAction,
        annotations_group_id: Uuid,
        labels_group_id: Uuid,
    ) -> TerraformService {
        TerraformService {
            long_id: self.service_id,
            name: self.name.clone(),
            kube_name: self.kube_name.clone(),
            action: Action::Create,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 256,
            ram_limit_in_mib: 256,
            gpu_request: None,
            gpu_limit: None,
            persistent_storage: PersistentStorage {
                storage_class: "".to_string(),
                size_in_gib: 1,
            },
            tf_files_source: TerraformFilesSource::Git {
                git_url: Url::parse("https://github.com/Qovery/terraform_service_engine_testing.git")
                    .expect("Invalid URL"),
                git_credentials: None,
                commit_id: "6692594dd31285e1b881f85cd504d934a579d7c5".to_string(),
                root_module_path: "/simple_terraform".to_string(),
            },
            tf_var_file_paths: vec!["tfvars/echo.tfvars".to_string()],
            tf_vars: vec![("command_argument".to_string(), "Mr Ripley".to_string())],
            provider: self.provider.clone(),
            provider_version: self.provider_version.clone(),
            terraform_action,
            backend: TerraformBackend {
                backend_type: TerraformBackendType::Kubernetes,
            },
            timeout_sec: 300,
            environment_vars_with_infos: self.environment_vars_with_infos.clone(),
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! { labels_group_id },
            shared_image_feature_enabled: false,
            terraform_credentials: None,
            extra_action_arguments: self.extra_action_arguments.clone(),
            dockerfile_fragment: None,
        }
    }
}

// Runs a terraform service lifecycle test with the given builder and deploy actions
fn run_terraform_lifecycle_test(
    cloud_provider: CloudProvider,
    service_builder: TerraformServiceTestBuilder,
    deploy_actions: Vec<TerraformActionCommand>,
    execution_id: Uuid,
) {
    let infra = TestInfra::new(cloud_provider);
    let mut environment = infra.create_environment();

    let (annotations_group_id, annotations_group) = create_default_test_annotations();
    let (labels_group_id, labels_group) = create_default_test_labels();

    environment.annotations_groups = btreemap! { annotations_group_id => annotations_group };
    environment.labels_groups = btreemap! { labels_group_id => labels_group };

    // Execute each deploy action
    for action_command in deploy_actions {
        environment.terraform_services = vec![service_builder.build(
            TerraformAction {
                command: action_command,
                plan_execution_id: Some(execution_id.to_string()),
            },
            annotations_group_id,
            labels_group_id,
        )];
        assert!(environment.deploy_environment(&environment, &infra.infra_ctx).is_ok());
    }

    // Destroy phase
    let mut environment_for_delete = environment.clone();
    environment_for_delete.action = Action::Delete;
    environment_for_delete.terraform_services = vec![service_builder.build(
        TerraformAction {
            command: TerraformActionCommand::Destroy,
            plan_execution_id: None,
        },
        annotations_group_id,
        labels_group_id,
    )];
    assert!(
        environment_for_delete
            .delete_environment(&environment_for_delete, &infra.infra_ctx_for_delete)
            .is_ok()
    );
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn build_and_deploy_terraform_service_on_aws_eks() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "terraform-service-test");

        run_terraform_lifecycle_test(
            CloudProvider::Aws,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn build_and_deploy_terraform_service_in_apply_mode_on_aws_eks() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "terraform-service-test")
            .with_env_vars(btreemap! {
                "QOVERY_TERRAFORM_Z0172BFB8_NAME".to_string() => VariableInfo {value: "dGVzdC1wZw==".to_string(), is_secret: false},
            });

        run_terraform_lifecycle_test(
            CloudProvider::Aws,
            service_builder,
            vec![TerraformActionCommand::PlanAndApply],
            execution_id,
        );

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn build_and_deploy_opentofu_service_on_aws_eks() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "opentofu-service-test")
            .with_name("opentofu service test #####")
            .with_provider(TerraformProvider::OpenTofu, "1.10.0");

        run_terraform_lifecycle_test(
            CloudProvider::Aws,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

// Scaleway tests
#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn build_and_deploy_terraform_service_on_scaleway() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "terraform-service-test");

        run_terraform_lifecycle_test(
            CloudProvider::Scaleway,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn build_and_deploy_opentofu_service_on_scaleway() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "opentofu-service-test")
            .with_name("opentofu service test #####")
            .with_provider(TerraformProvider::OpenTofu, "1.10.0");

        run_terraform_lifecycle_test(
            CloudProvider::Scaleway,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

// GCP tests
#[cfg(feature = "test-gcp-minimal")]
#[named]
#[test]
fn build_and_deploy_terraform_service_on_gcp_gke() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "terraform-service-test");

        run_terraform_lifecycle_test(
            CloudProvider::Gcp,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

#[cfg(feature = "test-gcp-minimal")]
#[named]
#[test]
fn build_and_deploy_opentofu_service_on_gcp_gke() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "opentofu-service-test")
            .with_name("opentofu service test #####")
            .with_provider(TerraformProvider::OpenTofu, "1.10.0");

        run_terraform_lifecycle_test(
            CloudProvider::Gcp,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

// Azure tests
#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn build_and_deploy_terraform_service_on_azure_aks() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "terraform-service-test");

        run_terraform_lifecycle_test(
            CloudProvider::Azure,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn build_and_deploy_opentofu_service_on_azure_aks() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let service_builder = TerraformServiceTestBuilder::new(service_id, "opentofu-service-test")
            .with_name("opentofu service test #####")
            .with_provider(TerraformProvider::OpenTofu, "1.10.0");

        run_terraform_lifecycle_test(
            CloudProvider::Azure,
            service_builder,
            vec![TerraformActionCommand::PlanOnly, TerraformActionCommand::ApplyFromPlan],
            execution_id,
        );

        "".to_string()
    })
}
