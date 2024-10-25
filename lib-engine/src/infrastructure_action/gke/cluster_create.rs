use crate::cloud_provider::helm::deploy_charts_levels;
use crate::cloud_provider::kubeconfig_helper::put_kubeconfig_file_to_object_storage;
use crate::cloud_provider::kubectl_utils::check_workers_on_create;
use crate::cloud_provider::kubernetes::{is_kubernetes_upgrade_required, Kubernetes};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsGcp};
use crate::cmd::terraform::{terraform_init_validate_plan_apply, terraform_output};
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep};
use crate::io_models::context::Features;

use crate::cmd::terraform_validators::TerraformValidators;
use crate::engine::InfrastructureContext;
use crate::models::domain::ToHelmString;
use crate::models::third_parties::LetsEncryptConfig;
use crate::object_storage::ObjectStorage;
use base64::engine::general_purpose;

use crate::cloud_provider::gcp::kubernetes::Gke;
use crate::infrastructure_action::gke::helm_charts::{gke_helm_charts, GkeChartsConfigPrerequisites};
use crate::infrastructure_action::gke::tera_context::gke_tera_context;
use crate::infrastructure_action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure_action::InfrastructureAction;
use base64::Engine;
use itertools::Itertools;
use std::fs;

pub(super) fn create_gke_cluster(cluster: &Gke, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
    let event_details = cluster.get_event_details(Infrastructure(InfrastructureStep::Create));

    cluster.logger.log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing GKE cluster deployment.".to_string()),
    ));

    if !cluster.context().is_first_cluster_deployment() {
        // upgrade cluster instead if required
        match is_kubernetes_upgrade_required(
            cluster.kubeconfig_local_file_path(),
            cluster.version.clone(),
            infra_ctx.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            cluster.logger(),
            None,
        ) {
            Ok(kubernetes_upgrade_status) => {
                if kubernetes_upgrade_status.required_upgrade_on.is_some() {
                    cluster.upgrade_cluster(infra_ctx, kubernetes_upgrade_status)?;
                } else {
                    cluster.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                    ))
                }
            }
            Err(e) => {
                // Log a warning, this error is not blocking
                cluster.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(
                        "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                        Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                    ),
                ));
            }
        };
    }

    let temp_dir = cluster.temp_dir();

    // generate terraform files and copy them into temp dir
    let context = gke_tera_context(cluster, infra_ctx)?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(cluster.template_directory.as_str(), temp_dir, context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            cluster.template_directory.to_string(),
            temp_dir.to_string_lossy().to_string(),
            e,
        )));
    }

    let dirs_to_be_copied_to = vec![
        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/gcp/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
        (
            format!("{}/common/bootstrap/charts", cluster.context.lib_root_dir()),
            format!("{}/common/charts", temp_dir.to_string_lossy()),
        ),
        // copy lib/common/bootstrap/chart_values directory (and sub directory) into the lib/gcp/bootstrap/common/chart_values directory.
        (
            format!("{}/common/bootstrap/chart_values", cluster.context.lib_root_dir()),
            format!("{}/common/chart_values", temp_dir.to_string_lossy()),
        ),
    ];
    for (source_dir, target_dir) in dirs_to_be_copied_to {
        if let Err(e) = crate::template::copy_non_template_files(&source_dir, target_dir.as_str()) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                source_dir,
                target_dir,
                e,
            )));
        }
    }

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Deploying GKE cluster.".to_string()),
    ));

    // TODO(benjaminch): move this elsewhere
    // Create object-storage buckets
    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Create Qovery managed object storage buckets".to_string()),
    ));
    for bucket_name in [
        cluster.kubeconfig_bucket_name().as_str(),
        cluster.logs_bucket_name().as_str(),
    ] {
        match cluster
            .object_storage
            .create_bucket(bucket_name, cluster.advanced_settings.resource_ttl(), true)
        {
            Ok(existing_bucket) => {
                cluster.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("Object storage bucket {bucket_name} created")),
                ));
                // Update set versioning to true if not activated on the bucket (bucket created before this option was enabled)
                // This can be removed at some point in the future, just here to handle legacy GCP buckets
                // TODO(ENG-1736): remove this update once all existing buckets have versioning activated
                if !existing_bucket.versioning_activated {
                    cluster.object_storage.update_bucket(bucket_name, true).map_err(|e| {
                        let error = EngineError::new_object_storage_error(event_details.clone(), e);
                        cluster.logger().log(EngineEvent::Error(error.clone(), None));
                        error
                    })?;
                }
            }
            Err(e) => {
                let error = EngineError::new_object_storage_error(event_details, e);
                cluster.logger().log(EngineEvent::Error(error.clone(), None));
                return Err(Box::new(error));
            }
        }
    }

    // Terraform deployment dedicated to cloud resources
    if let Err(e) = terraform_init_validate_plan_apply(
        temp_dir.to_string_lossy().as_ref(),
        cluster.context.is_dry_run_deploy(),
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
        &TerraformValidators::Default,
    ) {
        return Err(Box::new(EngineError::new_terraform_error(event_details, e)));
    }

    // Retrieve config generated via Terraform
    let qovery_terraform_config: GkeQoveryTerraformOutput = terraform_output(
        temp_dir.to_string_lossy().as_ref(),
        infra_ctx
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    )
    .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

    put_kubeconfig_file_to_object_storage(cluster, &cluster.object_storage)?;

    // Configure kubectl to be able to connect to cluster
    let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error

    // Ensure all nodes are ready on Kubernetes
    match check_workers_on_create(cluster, infra_ctx.cloud_provider(), None) {
        Ok(_) => cluster.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Kubernetes nodes have been successfully created".to_string()),
        )),
        Err(e) => {
            return Err(Box::new(EngineError::new_k8s_node_not_ready(event_details, e)));
        }
    };

    // Update cluster config to vault
    let kubeconfig_path = cluster.kubeconfig_local_file_path();
    let kubeconfig = fs::read_to_string(&kubeconfig_path).map_err(|e| {
        Box::new(EngineError::new_cannot_retrieve_cluster_config_file(
            event_details.clone(),
            CommandError::new_from_safe_message(format!(
                "Cannot read kubeconfig file {}: {e}",
                kubeconfig_path.to_str().unwrap_or_default()
            )),
        ))
    })?;
    let kubeconfig_b64 = general_purpose::STANDARD.encode(kubeconfig);
    let cluster_secrets = ClusterSecrets::new_google_gke(ClusterSecretsGcp::new(
        cluster.options.gcp_json_credentials.clone().into(),
        cluster.options.gcp_json_credentials.project_id.to_string(),
        cluster.region.clone(),
        Some(kubeconfig_b64),
        Some(qovery_terraform_config.gke_cluster_public_hostname),
        cluster.kind(),
        infra_ctx.cloud_provider().name().to_string(),
        cluster.long_id().to_string(),
        cluster.options.grafana_admin_user.clone(),
        cluster.options.grafana_admin_password.clone(),
        infra_ctx.cloud_provider().organization_long_id().to_string(),
        cluster.context().is_test_cluster(),
    ));
    // vault config is not blocking
    if let Err(e) = cluster.update_gke_vault_config(event_details.clone(), cluster_secrets) {
        cluster.logger.log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new("Cannot push cluster config to Vault".to_string(), Some(e.to_string())),
        ))
    }

    // kubernetes helm deployments on the cluster
    let credentials_environment_variables: Vec<(String, String)> = infra_ctx
        .cloud_provider()
        .credentials_environment_variables()
        .into_iter()
        .map(|x| (x.0.to_string(), x.1.to_string()))
        .collect();

    cluster.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
    ));

    let charts_prerequisites = GkeChartsConfigPrerequisites::new(
        infra_ctx.cloud_provider().organization_id().to_string(),
        infra_ctx.cloud_provider().organization_long_id(),
        cluster.short_id().to_string(),
        cluster.long_id,
        cluster.context.is_feature_enabled(&Features::LogsHistory),
        cluster.context.is_feature_enabled(&Features::MetricsHistory),
        infra_ctx.dns_provider().domain().to_helm_format_string(),
        infra_ctx.dns_provider().domain().root_domain().to_helm_format_string(),
        LetsEncryptConfig::new(cluster.options.tls_email_report.to_string(), cluster.context.is_test_cluster()),
        infra_ctx.dns_provider().provider_configuration(),
        qovery_terraform_config.loki_logging_service_account_email,
        cluster.logs_bucket_name(),
        cluster.options.clone(),
        cluster.advanced_settings().clone(),
    );

    let helm_charts_to_deploy = gke_helm_charts(
        &charts_prerequisites,
        Some(temp_dir.to_string_lossy().as_ref()),
        &kubeconfig_path,
        &*cluster.context.qovery_api,
        cluster.customer_helm_charts_override(),
        infra_ctx.dns_provider().domain(),
    )
    .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?;

    deploy_charts_levels(
        infra_ctx.mk_kube_client()?.client(),
        &kubeconfig_path,
        credentials_environment_variables
            .iter()
            .map(|(l, r)| (l.as_str(), r.as_str()))
            .collect_vec()
            .as_slice(),
        helm_charts_to_deploy,
        cluster.context.is_dry_run_deploy(),
        Some(&infra_ctx.kubernetes().helm_charts_diffs_directory()),
    )
    .map_err(|e| Box::new(EngineError::new_helm_chart_error(event_details.clone(), e)))
}
