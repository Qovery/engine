use std::str::FromStr;

use itertools::Itertools;
use rusoto_core::Region;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesNode};
use crate::cloud_provider::models::WorkerNodeDataTemplate;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd;
use crate::cmd::kubectl::kubectl_exec_get_all_namespaces;
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause};
use crate::fs::workspace_directory;
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;

pub mod node;

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
        let test_cluster = match self.context.metadata() {
            Some(meta) => match meta.test {
                Some(true) => true,
                _ => false,
            },
            _ => false,
        };

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

        context.insert("test_cluster", &test_cluster);

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
        let lets_encrypt_url = match &test_cluster {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("acme_server_url", lets_encrypt_url);

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

        // TODO URGENT change the behavior of self.bucket_name()
        context.insert("aws_region", &self.region.name());
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");
        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
        context.insert("vpc_cidr_block", &vpc_cidr_block.clone());
        context.insert("s3_kubeconfig_bucket", &self.kubeconfig_bucket_name());

        // AWS - EKS
        context.insert("eks_cidr_subnet", &eks_cidr_subnet.clone());
        context.insert("kubernetes_cluster_name", &self.name());
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
                "start to create EKS cluster {} with id {}",
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

        let _ = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            crate::cmd::terraform::terraform_exec_with_init_validate_plan_apply(
                temp_dir.as_str(),
                self.context.is_dry_run_deploy(),
            ),
        )?;

        Ok(())
    }

    fn on_create_error(&self) -> Result<(), EngineError> {
        warn!("EKS.on_create_error() called for {}", self.name());
        // FIXME
        Err(self.engine_error(
            EngineErrorCause::Internal,
            format!("{} Kubernetes cluster rollback not implemented", self.name()),
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

    fn on_delete(&self) -> Result<(), EngineError> {
        info!("EKS.on_delete() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.delete_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Warn,
            Some(format!(
                "start to delete EKS cluster {} with id {}",
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
        info!("Running Terraform apply");
        match cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            cmd::terraform::terraform_exec_with_init_validate_plan_apply(temp_dir.as_str(), false),
        ) {
            Err(e) => error!("An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy: {:?}", e.message),
            _ => {}
        };

        // should make the diff between all namespaces and qovery managed namespaces
        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                info!("Deleting non Qovery namespaces");
                for namespace_to_delete in namespaces_to_delete.iter() {
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

        // https://cert-manager.io/docs/installation/uninstall/kubernetes/
        // required to avoid namespace stuck on deletion
        info!("Delete cert-manager related objects to prepare deletion");
        let cert_manager_objects = vec![
            "Issuers",
            "ClusterIssuers",
            "Certificates",
            "CertificateRequests",
            "Orders",
            "Challenges",
        ];
        for object in cert_manager_objects {
            match kubectl_delete_objects_in_all_namespaces(
                &kubernetes_config_file_path,
                object,
                self.cloud_provider().credentials_environment_variables(),
            ) {
                Ok(_) => {}
                Err(e) => {
                    let error_message = format!(
                        "Wasn't able to delete all objects type {}, it's a blocker to then delete cert-manager namespace. {}",
                        object,
                        format!("{:?}", e.message)
                    );
                    return Err(EngineError::new(
                        Internal,
                        self.engine_error_scope(),
                        self.context().execution_id(),
                        Some(error_message),
                    ));
                }
            };
        }

        info!("Deleting Qovery managed Namespaces");
        let qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
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
        ) {
            Ok(helm_charts) => {
                for chart in helm_charts {
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
        let terraform_result = cast_simple_error_to_engine_error(
            self.engine_error_scope(),
            self.context.execution_id(),
            cmd::terraform::terraform_exec_with_init_plan_destroy(temp_dir.as_str()),
        );

        Ok(())
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
