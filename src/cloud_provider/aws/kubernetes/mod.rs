use std::str::FromStr;

use itertools::Itertools;
use rusoto_core::Region;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::kubernetes::roles::get_default_roles_to_create;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{uninstall_cert_manager, Kind, Kubernetes, KubernetesNode};
use crate::cloud_provider::models::WorkerNodeDataTemplate;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd;
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces};
use crate::cmd::structs::HelmChart;
use crate::cmd::terraform::{terraform_exec, terraform_init_validate_plan_apply, terraform_init_validate_state_list};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::error::EngineErrorCause::Internal;
use crate::error::{
    cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope, SimpleError, SimpleErrorKind,
};
use crate::fs::workspace_directory;
use crate::models::{
    Context, Features, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;
use std::env;
use chrono::Utc;

pub mod node;
pub mod roles;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Options {
    // AWS related
    pub eks_zone_a_subnet_blocks: Vec<String>,
    pub eks_zone_b_subnet_blocks: Vec<String>,
    pub eks_zone_c_subnet_blocks: Vec<String>,
    pub rds_zone_a_subnet_blocks: Vec<String>,
    pub rds_zone_b_subnet_blocks: Vec<String>,
    pub rds_zone_c_subnet_blocks: Vec<String>,
    pub documentdb_zone_a_subnet_blocks: Vec<String>,
    pub documentdb_zone_b_subnet_blocks: Vec<String>,
    pub documentdb_zone_c_subnet_blocks: Vec<String>,
    pub elasticache_zone_a_subnet_blocks: Vec<String>,
    pub elasticache_zone_b_subnet_blocks: Vec<String>,
    pub elasticache_zone_c_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_a_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_b_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_c_subnet_blocks: Vec<String>,
    pub vpc_cidr_block: String,
    pub eks_cidr_subnet: String,
    pub eks_access_cidr_blocks: Vec<String>,
    pub rds_cidr_subnet: String,
    pub documentdb_cidr_subnet: String,
    pub elasticache_cidr_subnet: String,
    pub elasticsearch_cidr_subnet: String,
    // Qovery
    pub qovery_api_url: String,
    pub engine_version_controller_token: String,
    pub agent_version_controller_token: String,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub discord_api_key: String,
    pub qovery_nats_url: String,
    pub qovery_nats_user: String,
    pub qovery_nats_password: String,
    pub qovery_ssh_key: String,
    // Others
    pub tls_email_report: String,
}

pub struct EKS<'a> {
    context: Context,
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    dns_provider: &'a dyn DnsProvider,
    s3: S3,
    nodes: Vec<Node>,
    template_directory: String,
    options: Options,
    listeners: Listeners,
}

impl<'a> EKS<'a> {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        version: &str,
        region: &str,
        cloud_provider: &'a AWS,
        dns_provider: &'a dyn DnsProvider,
        options: Options,
        nodes: Vec<Node>,
    ) -> Self {
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        // TODO export this
        let s3 = S3::new(
            context.clone(),
            "s3-temp-id".to_string(),
            "default-s3".to_string(),
            cloud_provider.access_key_id.clone(),
            cloud_provider.secret_access_key.clone(),
        );

        EKS {
            context,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
            dns_provider,
            s3,
            options,
            nodes,
            template_directory,
            listeners: cloud_provider.listeners.clone(), // copy listeners from CloudProvider
        }
    }

    fn kubeconfig_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.id())
    }

    fn tera_context(&self) -> TeraContext {
        let format_ips =
            |ips: &Vec<String>| -> Vec<String> { ips.iter().map(|ip| format!("\"{}\"", ip)).collect::<Vec<_>>() };

        let eks_zone_a_subnet_blocks = format_ips(&self.options.eks_zone_a_subnet_blocks);
        let eks_zone_b_subnet_blocks = format_ips(&self.options.eks_zone_b_subnet_blocks);
        let eks_zone_c_subnet_blocks = format_ips(&self.options.eks_zone_c_subnet_blocks);
        let rds_zone_a_subnet_blocks = format_ips(&self.options.rds_zone_a_subnet_blocks);
        let rds_zone_b_subnet_blocks = format_ips(&self.options.rds_zone_b_subnet_blocks);
        let rds_zone_c_subnet_blocks = format_ips(&self.options.rds_zone_c_subnet_blocks);

        let documentdb_zone_a_subnet_blocks = format_ips(&self.options.documentdb_zone_a_subnet_blocks);
        let documentdb_zone_b_subnet_blocks = format_ips(&self.options.documentdb_zone_b_subnet_blocks);
        let documentdb_zone_c_subnet_blocks = format_ips(&self.options.documentdb_zone_c_subnet_blocks);

        let elasticache_zone_a_subnet_blocks = format_ips(&self.options.elasticache_zone_a_subnet_blocks);
        let elasticache_zone_b_subnet_blocks = format_ips(&self.options.elasticache_zone_b_subnet_blocks);
        let elasticache_zone_c_subnet_blocks = format_ips(&self.options.elasticache_zone_c_subnet_blocks);

        let elasticsearch_zone_a_subnet_blocks = format_ips(&self.options.elasticsearch_zone_a_subnet_blocks);
        let elasticsearch_zone_b_subnet_blocks = format_ips(&self.options.elasticsearch_zone_b_subnet_blocks);
        let elasticsearch_zone_c_subnet_blocks = format_ips(&self.options.elasticsearch_zone_c_subnet_blocks);

        let region_cluster_id = format!("{}-{}", self.region(), self.id());
        let vpc_cidr_block = self.options.vpc_cidr_block.clone();
        let eks_cloudwatch_log_group = format!("/aws/eks/{}/cluster", self.id());
        let eks_cidr_subnet = self.options.eks_cidr_subnet.clone();

        let eks_access_cidr_blocks = format_ips(&self.options.eks_access_cidr_blocks);

        let worker_nodes = self
            .nodes
            .iter()
            .group_by(|e| e.instance_type())
            .into_iter()
            .map(|(instance_type, group)| (instance_type, group.collect::<Vec<_>>()))
            .map(|(instance_type, nodes)| WorkerNodeDataTemplate {
                instance_type: instance_type.to_string(),
                desired_size: "1".to_string(),
                max_size: nodes.len().to_string(),
                min_size: "1".to_string(),
            })
            .collect::<Vec<WorkerNodeDataTemplate>>();

        let qovery_api_url = self.options.qovery_api_url.clone();
        let rds_cidr_subnet = self.options.rds_cidr_subnet.clone();
        let documentdb_cidr_subnet = self.options.documentdb_cidr_subnet.clone();
        let elasticache_cidr_subnet = self.options.elasticache_cidr_subnet.clone();
        let elasticsearch_cidr_subnet = self.options.elasticsearch_cidr_subnet.clone();

        let managed_dns_list = vec![self.dns_provider.name()];
        let managed_dns_domains_helm_format = vec![format!("\"{}\"", self.dns_provider.domain())];
        let managed_dns_domains_terraform_format = terraform_list_format(vec![self.dns_provider.domain().to_string()]);

        let managed_dns_resolvers: Vec<String> = self
            .dns_provider
            .resolvers()
            .iter()
            .map(|x| format!("{}", x.clone().to_string()))
            .collect();

        let managed_dns_resolvers_terraform_format = terraform_list_format(managed_dns_resolvers);

        let mut context = TeraContext::new();
        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("qovery_api_url", &qovery_api_url);

        context.insert(
            "engine_version_controller_token",
            &self.options.engine_version_controller_token,
        );
        context.insert(
            "agent_version_controller_token",
            &self.options.agent_version_controller_token,
        );

        context.insert("test_cluster", &self.context.is_test_cluster());
        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }
        context.insert("force_upgrade", &self.context.requires_forced_upgrade());

        // Qovery features
        context.insert(
            "log_history_enabled",
            &self.context.is_feature_enabled(&Features::LogsHistory),
        );
        context.insert(
            "metrics_history_enabled",
            &self.context.is_feature_enabled(&Features::MetricsHistory),
        );

        // DNS configuration
        context.insert("managed_dns", &managed_dns_list);
        context.insert("managed_dns_domains_helm_format", &managed_dns_domains_helm_format);

        context.insert(
            "managed_dns_domains_terraform_format",
            &managed_dns_domains_terraform_format,
        );

        context.insert(
            "managed_dns_resolvers_terraform_format",
            &managed_dns_resolvers_terraform_format,
        );

        match self.dns_provider.kind() {
            dns_provider::Kind::Cloudflare => {
                context.insert("external_dns_provider", "cloudflare");
                context.insert("cloudflare_api_token", self.dns_provider.token());
                context.insert("cloudflare_email", self.dns_provider.account());
            }
        };

        context.insert("dns_email_report", &self.options.tls_email_report); // Pierre suggested renaming to tls_email_report

        // TLS
        let lets_encrypt_url = match &self.context.is_test_cluster() {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("acme_server_url", lets_encrypt_url);

        // Vault
        context.insert("vault_auth_method", "none");

        if let Some(_) = env::var_os("VAULT_ADDR") {
            // select the correct used method
            match env::var_os("VAULT_ROLE_ID") {
                Some(role_id) => {
                    context.insert("vault_auth_method", "app_role");
                    context.insert("vault_role_id", role_id.to_str().unwrap());

                    match env::var_os("VAULT_SECRET_ID") {
                        Some(secret_id) => context.insert("vault_secret_id", secret_id.to_str().unwrap()),
                        None => error!("VAULT_SECRET_ID environment variable wasn't found"),
                    }
                }
                None => {
                    if let Some(_) = env::var_os("VAULT_TOKEN") {
                        context.insert("vault_auth_method", "token")
                    }
                }
            }
        };

        // AWS
        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);

        // AWS S3 tfstate storage
        context.insert(
            "aws_access_key_tfstates_account",
            self.cloud_provider()
                .terraform_state_credentials()
                .access_key_id
                .as_str(),
        );

        context.insert(
            "aws_secret_key_tfstates_account",
            self.cloud_provider()
                .terraform_state_credentials()
                .secret_access_key
                .as_str(),
        );
        context.insert(
            "aws_region_tfstates_account",
            self.cloud_provider().terraform_state_credentials().region.as_str(),
        );

        context.insert("aws_region", &self.region.name());
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");
        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
        context.insert("vpc_cidr_block", &vpc_cidr_block.clone());
        context.insert("s3_kubeconfig_bucket", &self.kubeconfig_bucket_name());

        // AWS - EKS
        context.insert("eks_cidr_subnet", &eks_cidr_subnet.clone());
        context.insert("kubernetes_cluster_name", &self.name());
        context.insert("kubernetes_cluster_creation_date", &Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
        context.insert("kubernetes_cluster_id", self.id());
        context.insert("eks_region_cluster_id", region_cluster_id.as_str());
        context.insert("eks_worker_nodes", &worker_nodes);
        context.insert("eks_zone_a_subnet_blocks", &eks_zone_a_subnet_blocks);
        context.insert("eks_zone_b_subnet_blocks", &eks_zone_b_subnet_blocks);
        context.insert("eks_zone_c_subnet_blocks", &eks_zone_c_subnet_blocks);
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cloudwatch_log_group", &eks_cloudwatch_log_group);
        context.insert("eks_access_cidr_blocks", &eks_access_cidr_blocks);

        // AWS - RDS
        context.insert("rds_cidr_subnet", &rds_cidr_subnet);
        context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
        context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
        context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);

        // AWS - DocumentDB
        context.insert("documentdb_cidr_subnet", &documentdb_cidr_subnet);
        context.insert("documentdb_zone_a_subnet_blocks", &documentdb_zone_a_subnet_blocks);
        context.insert("documentdb_zone_b_subnet_blocks", &documentdb_zone_b_subnet_blocks);
        context.insert("documentdb_zone_c_subnet_blocks", &documentdb_zone_c_subnet_blocks);

        // AWS - Elasticache
        context.insert("elasticache_cidr_subnet", &elasticache_cidr_subnet);
        context.insert("elasticache_zone_a_subnet_blocks", &elasticache_zone_a_subnet_blocks);
        context.insert("elasticache_zone_b_subnet_blocks", &elasticache_zone_b_subnet_blocks);
        context.insert("elasticache_zone_c_subnet_blocks", &elasticache_zone_c_subnet_blocks);

        // AWS - Elasticsearch
        context.insert("elasticsearch_cidr_subnet", &elasticsearch_cidr_subnet.clone());
        context.insert(
            "elasticsearch_zone_a_subnet_blocks",
            &elasticsearch_zone_a_subnet_blocks,
        );
        context.insert(
            "elasticsearch_zone_b_subnet_blocks",
            &elasticsearch_zone_b_subnet_blocks,
        );
        context.insert(
            "elasticsearch_zone_c_subnet_blocks",
            &elasticsearch_zone_c_subnet_blocks,
        );

        // grafana credentials
        context.insert("grafana_admin_user", self.options.grafana_admin_user.as_str());
        context.insert("grafana_admin_password", self.options.grafana_admin_password.as_str());

        // qovery
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert("qovery_ssh_key", self.options.qovery_ssh_key.as_str());
        context.insert("discord_api_key", self.options.discord_api_key.as_str());

        context
    }
}

impl<'a> Kubernetes for EKS<'a> {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Eks
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn region(&self) -> &str {
        self.region.name()
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.s3
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create(&self) -> Result<(), EngineError> {
        info!("EKS.on_create() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Preparing EKS {} cluster deployment with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        // create AWS IAM roles
        let already_created_roles = get_default_roles_to_create();
        for role in already_created_roles {
            match role.create_service_linked_role(
                self.cloud_provider.access_key_id.as_str(),
                self.cloud_provider.secret_access_key.as_str(),
            ) {
                Ok(_) => info!("Role {} is already present, no need to create", role.role_name),
                Err(e) => error!(
                    "While getting, or creating the role {} : causing by {:?}",
                    role.role_name, e
                ),
            }
        }

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("bootstrap/{}", self.name()),
        );

        // generate terraform files and copy them into temp dir
        let context = self.tera_context();

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Deploying EKS {} cluster deployment with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()),
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                format!("Error while deploying cluster {} with id {}.", self.name(), self.id());
                Err(e)
            }
        }
    }

    fn on_create_error(&self) -> Result<(), EngineError> {
        warn!("EKS.on_create_error() called for {}", self.name());
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster failed on deployment", self.name()),
        ))
    }

    fn on_upgrade(&self) -> Result<(), EngineError> {
        info!("EKS.on_upgrade() called for {}", self.name());
        Ok(())
    }

    fn on_upgrade_error(&self) -> Result<(), EngineError> {
        warn!("EKS.on_upgrade_error() called for {}", self.name());
        Ok(())
    }

    fn on_downgrade(&self) -> Result<(), EngineError> {
        info!("EKS.on_downgrade() called for {}", self.name());
        Ok(())
    }

    fn on_downgrade_error(&self) -> Result<(), EngineError> {
        warn!("EKS.on_downgrade_error() called for {}", self.name());
        Ok(())
    }

    fn on_pause(&self) -> Result<(), EngineError> {
        info!("EKS.on_pause() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Preparing EKS {} cluster pause with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("bootstrap/{}", self.name()),
        );

        // generate terraform files and copy them into temp dir
        let mut context = self.tera_context();

        // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
        let worker_nodes: Vec<WorkerNodeDataTemplate> = Vec::new();
        context.insert("eks_worker_nodes", &worker_nodes);

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        // pause: only select terraform workers elements to pause to avoid applying on the whole config
        // this to avoid failures because of helm deployments on removing workers nodes
        let tf_workers_resources = match terraform_init_validate_state_list(temp_dir.as_str()) {
            Ok(x) => {
                let mut tf_workers_resources_name = Vec::new();
                for name in x {
                    if name.starts_with("aws_eks_node_group.") {
                        tf_workers_resources_name.push(name);
                    }
                }
                tf_workers_resources_name
            }
            Err(e) => {
                return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                    execution_id: self.context.execution_id().to_string(),
                    message: e.message,
                })
            }
        };
        if tf_workers_resources.len() == 0 {
            return Err(EngineError {
                cause: EngineErrorCause::Internal,
                scope: EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                execution_id: self.context.execution_id().to_string(),
                message: Some("No worker nodes present, can't Pause the infrastructure. This can happen if there where a manual operations on the workers or the infrastructure is already pause.".to_string()),
            });
        }

        let kubernetes_config_file_path = self.config_file_path()?;

        // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
        let metric_name = "taskmanager_nb_running_tasks";
        let wait_engine_job_finish = retry::retry(Fibonacci::from_millis(60000).take(60), || {
            return match kubectl_exec_api_custom_metrics(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
                "qovery",
                None,
                metric_name,
            ) {
                Ok(metrics) => {
                    let mut current_engine_jobs = 0;

                    for metric in metrics.items {
                        match metric.value.parse::<i32>() {
                            Ok(job_count) if job_count > 0 => current_engine_jobs += 1,
                            Err(e) => {
                                error!("error while looking at the API metric value {}. {:?}", metric_name, e);
                                return OperationResult::Retry(SimpleError {
                                    kind: SimpleErrorKind::Other,
                                    message: Some(e.to_string()),
                                });
                            }
                            _ => {}
                        }
                    }

                    if current_engine_jobs == 0 {
                        OperationResult::Ok(())
                    } else {
                        OperationResult::Retry(SimpleError {
                            kind: SimpleErrorKind::Other,
                            message: Some("can't pause the infrastructure now, Engine jobs are currently running, retrying later...".to_string()),
                        })
                    }
                }
                Err(e) => {
                    error!("error while looking at the API metric value {}. {:?}", metric_name, e);
                    OperationResult::Retry(e)
                }
            };
        });

        match wait_engine_job_finish {
            Ok(_) => info!("no current running jobs on the Engine, infrastructure pause is allowed to start"),
            Err(Operation { error, .. }) => {
                return Err(EngineError {
                    cause: EngineErrorCause::Internal,
                    scope: EngineErrorScope::Engine,
                    execution_id: self.context.execution_id().to_string(),
                    message: error.message,
                })
            }
            Err(retry::Error::Internal(msg)) => {
                return Err(EngineError::new(
                    EngineErrorCause::Internal,
                    EngineErrorScope::Engine,
                    self.context.execution_id(),
                    Some(msg),
                ))
            }
        }

        let mut terraform_args_string = vec!["apply".to_string(), "-auto-approve".to_string()];
        for x in tf_workers_resources {
            terraform_args_string.push(format!("-target={}", x));
        }
        let terraform_args = terraform_args_string.iter().map(|x| &**x).collect();

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Pausing EKS {} cluster deployment with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            terraform_exec(temp_dir.as_str(), terraform_args),
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                format!("Error while pausing cluster {} with id {}.", self.name(), self.id());
                Err(e)
            }
        }
    }

    fn on_pause_error(&self) -> Result<(), EngineError> {
        warn!("EKS.on_pause_error() called for {}", self.name());
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster failed to pause", self.name()),
        ))
    }

    fn on_delete(&self) -> Result<(), EngineError> {
        info!("EKS.on_delete() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.delete_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Preparing to delete EKS cluster {} with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("bootstrap/{}", self.name()),
        );

        // generate terraform files and copy them into temp dir
        let context = self.tera_context();

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::generate_and_copy_all_files_into_dir(
                self.template_directory.as_str(),
                temp_dir.as_str(),
                &context,
            ),
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::template::copy_non_template_files(
                format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
                common_charts_temp_dir.as_str(),
            ),
        )?;

        let kubernetes_config_file_path = self.config_file_path()?;

        let all_namespaces = kubectl_exec_get_all_namespaces(
            &kubernetes_config_file_path,
            self.cloud_provider().credentials_environment_variables(),
        );

        // should apply before destroy to be sure destroy will compute on all resources
        // don't exit on failure, it can happen if we resume a destroy process
        listeners_helper.delete_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!("Ensuring everything is up to date before deleting",)),
            self.context.execution_id(),
        ));

        info!("Running Terraform apply");
        if let Err(e) = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            cmd::terraform::terraform_init_validate_plan_apply(temp_dir.as_str(), false),
        ) {
            error!("An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy: {:?}", e.message)
        };

        // should make the diff between all namespaces and qovery managed namespaces
        listeners_helper.delete_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Warn,
            Some("Deleting all non-Qovery deployed applications and dependencies".to_string()),
            self.context.execution_id(),
        ));

        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                info!("Deleting non Qovery namespaces");
                for namespace_to_delete in namespaces_to_delete.iter() {
                    info!("Starting namespace {} deletion process", namespace_to_delete);
                    let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                        &kubernetes_config_file_path,
                        namespace_to_delete,
                        self.cloud_provider().credentials_environment_variables(),
                    );

                    match deletion {
                        Ok(_) => info!("Namespace {} is deleted", namespace_to_delete),
                        Err(e) => {
                            if e.message.is_some() && e.message.unwrap().contains("not found") {
                                {}
                            } else {
                                error!("Can't delete the namespace {}", namespace_to_delete);
                            }
                        }
                    }
                }
            }

            Err(e) => error!(
                "Error while getting all namespaces for Kubernetes cluster {}: error {:?}",
                self.name_with_id(),
                e.message
            ),
        }

        listeners_helper.delete_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Warn,
            Some("Deleting all Qovery deployed elements and associated dependencies".to_string()),
            self.context.execution_id(),
        ));

        // delete custom metrics api to avoid stale namespaces on deletion
        let _ = cmd::helm::helm_uninstall_list(
            &kubernetes_config_file_path,
            vec![HelmChart {
                name: "metrics-server".to_string(),
                namespace: "kube-system".to_string(),
            }],
            self.cloud_provider().credentials_environment_variables(),
        );

        // required to avoid namespace stuck on deletion
        match uninstall_cert_manager(
            &kubernetes_config_file_path,
            self.cloud_provider().credentials_environment_variables(),
        ) {
            Ok(_) => {}
            Err(e) => {
                return Err(EngineError::new(
                    Internal,
                    self.engine_error_scope(),
                    self.context().execution_id(),
                    e.message,
                ))
            }
        };

        info!("Deleting Qovery managed helm charts");
        let qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
            info!(
                "Starting Qovery managed charts deletion process in {} namespace",
                qovery_namespace
            );
            let charts_to_delete = cmd::helm::helm_list(
                &kubernetes_config_file_path,
                self.cloud_provider().credentials_environment_variables(),
                Some(qovery_namespace),
            );
            match charts_to_delete {
                Ok(charts) => {
                    for chart in charts {
                        info!("Deleting chart {} in {} namespace", chart.name, chart.namespace);
                        match cmd::helm::helm_exec_uninstall(
                            &kubernetes_config_file_path,
                            &chart.namespace,
                            &chart.name,
                            self.cloud_provider().credentials_environment_variables(),
                        ) {
                            Ok(_) => info!("chart {} deleted", chart.name),
                            Err(e) => error!("{:?}", e),
                        }
                    }
                }
                Err(e) => {
                    if e.message.is_some() && e.message.unwrap().contains("not found") {
                        {}
                    } else {
                        error!("Can't delete the namespace {}", qovery_namespace);
                    }
                }
            }
        }

        info!("Deleting Qovery managed Namespaces");
        for qovery_namespace in qovery_namespaces.iter() {
            info!("Starting namespace {} deletion process", qovery_namespace);
            let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                &kubernetes_config_file_path,
                qovery_namespace,
                self.cloud_provider().credentials_environment_variables(),
            );
            match deletion {
                Ok(_) => info!("Namespace {} is fully deleted", qovery_namespace),
                Err(e) => {
                    if e.message.is_some() && e.message.unwrap().contains("not found") {
                        {}
                    } else {
                        error!("Can't delete the namespace {}", qovery_namespace);
                    }
                }
            }
        }

        info!("Delete all remaining deployed helm applications");
        match cmd::helm::helm_list(
            &kubernetes_config_file_path,
            self.cloud_provider().credentials_environment_variables(),
            None,
        ) {
            Ok(helm_charts) => {
                for chart in helm_charts {
                    info!("Deleting chart {} in progress...", chart.name);
                    let _ = cmd::helm::helm_uninstall_list(
                        &kubernetes_config_file_path,
                        vec![chart],
                        self.cloud_provider().credentials_environment_variables(),
                    );
                }
            }
            Err(_) => error!("Unable to get helm list"),
        }

        info!("Running Terraform destroy");
        listeners_helper.delete_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Starting to delete EKS cluster {} with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        let terraform_result =
            retry::retry(
                Fibonacci::from_millis(60000).take(3),
                || match cast_simple_error_to_engine_error(
                    self.engine_error_scope(),
                    self.context.execution_id(),
                    cmd::terraform::terraform_init_validate_destroy(temp_dir.as_str(), false),
                ) {
                    Ok(_) => OperationResult::Ok(()),
                    Err(e) => OperationResult::Retry(e),
                },
            );

        match terraform_result {
            Ok(_) => Ok(()),
            Err(Operation { error, .. }) => Err(error),
            Err(retry::Error::Internal(msg)) => Err(EngineError::new(
                EngineErrorCause::Internal,
                self.engine_error_scope(),
                self.context().execution_id(),
                Some(format!(
                    "Error while deleting cluster {} with id {}: {}",
                    self.name(),
                    self.id(),
                    msg
                )),
            )),
        }
    }

    fn on_delete_error(&self) -> Result<(), EngineError> {
        warn!("EKS.on_delete_error() called for {}", self.name());

        // FIXME What should we do if something goes wrong while deleting the cluster?

        Ok(())
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("EKS.deploy_environment() called for {}", self.name());
        kubernetes::deploy_environment(self, environment)
    }

    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError> {
        warn!("EKS.deploy_environment_error() called for {}", self.name());
        kubernetes::deploy_environment_error(self, environment)
    }

    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("EKS.pause_environment() called for {}", self.name());
        kubernetes::pause_environment(self, environment)
    }

    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        warn!("EKS.pause_environment_error() called for {}", self.name());
        Ok(())
    }

    fn delete_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("EKS.delete_environment() called for {}", self.name());
        kubernetes::delete_environment(self, environment)
    }

    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        warn!("EKS.delete_environment_error() called for {}", self.name());
        Ok(())
    }
}

impl<'a> Listen for EKS<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
