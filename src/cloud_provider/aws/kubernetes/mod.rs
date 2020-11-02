use std::borrow::Borrow;
use std::env;
use std::iter::FromIterator;
use std::ops::Deref;
use std::rc::Rc;
use std::str::FromStr;

use itertools::Itertools;
use rusoto_core::Region;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tera::Context as TeraContext;

use crate::cloud_provider::aws::common::{do_stateless_service_cleanup, kubernetes_config_path};
use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{
    check_kubernetes_has_enough_resources_to_deploy_environment, Kind, Kubernetes, KubernetesError,
    KubernetesNode, Resources,
};
use crate::cloud_provider::service::{Service, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd;
use crate::cmd::helm::helm_uninstall_list;
use crate::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_all_namespaces};
use crate::cmd::utilities::CmdError;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::cloudflare::Cloudflare;
use crate::dns_provider::DnsProvider;
use crate::dns_provider::Kind::CLOUDFLARE;
use crate::fs::workspace_directory;
use crate::models::{
    Context, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressListener,
    ProgressScope,
};
use crate::string::terraform_list_format;
use crate::unit_conversion::{cpu_string_to_float, ki_to_mi};
use crate::{dns_provider, dynamo_db, s3};

pub mod node;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Options {
    pub eks_zone_a_subnet_blocks: Vec<String>,
    pub eks_zone_b_subnet_blocks: Vec<String>,
    pub eks_zone_c_subnet_blocks: Vec<String>,
    pub rds_zone_a_subnet_blocks: Vec<String>,
    pub rds_zone_b_subnet_blocks: Vec<String>,
    pub rds_zone_c_subnet_blocks: Vec<String>,
    pub documentdb_zone_a_subnet_blocks: Vec<String>,
    pub documentdb_zone_b_subnet_blocks: Vec<String>,
    pub documentdb_zone_c_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_a_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_b_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_c_subnet_blocks: Vec<String>,
    pub vpc_cidr_block: String,
    pub eks_cidr_subnet: String,
    pub qovery_api_url: String,
    pub tls_email_report: String,
    pub rds_cidr_subnet: String,
    pub documentdb_cidr_subnet: String,
    pub elasticsearch_cidr_subnet: String,
    pub engine_version_controller_token: String,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub discord_api_key: String,
    pub qovery_nats_url: String,
    pub qovery_ssh_key: String,
    pub eks_access_cidr_blocks: String,
}

pub struct EKS<'a> {
    context: Context,
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    dns_provider: &'a dyn DnsProvider,
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

        EKS {
            context,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
            dns_provider,
            options,
            nodes,
            template_directory,
            listeners: cloud_provider.listeners.clone(), // copy listeners from CloudProvider
        }
    }

    fn tera_context(&self) -> TeraContext {
        let format_ips = |ips: &Vec<String>| -> Vec<String> {
            ips.iter()
                .map(|ip| format!("\"{}\"", ip))
                .collect::<Vec<_>>()
        };

        let eks_zone_a_subnet_blocks = format_ips(&self.options.eks_zone_a_subnet_blocks);
        let eks_zone_b_subnet_blocks = format_ips(&self.options.eks_zone_b_subnet_blocks);
        let eks_zone_c_subnet_blocks = format_ips(&self.options.eks_zone_c_subnet_blocks);
        let rds_zone_a_subnet_blocks = format_ips(&self.options.rds_zone_a_subnet_blocks);
        let rds_zone_b_subnet_blocks = format_ips(&self.options.rds_zone_b_subnet_blocks);
        let rds_zone_c_subnet_blocks = format_ips(&self.options.rds_zone_c_subnet_blocks);

        let documentdb_zone_a_subnet_blocks =
            format_ips(&self.options.documentdb_zone_a_subnet_blocks);

        let documentdb_zone_b_subnet_blocks =
            format_ips(&self.options.documentdb_zone_b_subnet_blocks);

        let documentdb_zone_c_subnet_blocks =
            format_ips(&self.options.documentdb_zone_c_subnet_blocks);

        let elasticsearch_zone_a_subnet_blocks =
            format_ips(&self.options.elasticsearch_zone_a_subnet_blocks);

        let elasticsearch_zone_b_subnet_blocks =
            format_ips(&self.options.elasticsearch_zone_b_subnet_blocks);

        let elasticsearch_zone_c_subnet_blocks =
            format_ips(&self.options.elasticsearch_zone_c_subnet_blocks);

        let region_cluster_id = format!("{}-{}", self.region(), self.id());
        let vpc_cidr_block = self.options.vpc_cidr_block.clone();
        let eks_cloudwatch_log_group = format!("/aws/eks/{}/cluster", self.id());
        let eks_cidr_subnet = self.options.eks_cidr_subnet.clone();
        let worker_nodes = self
            .nodes
            .iter()
            .group_by(|e| e.instance_type())
            .into_iter()
            .map(|(instance_type, group)| (instance_type, group.collect::<Vec<_>>()))
            .map(|(instance_type, nodes)| WorkerNodeDataTemplate {
                instance_type: instance_type.to_string(),
                desired_size: nodes.len().to_string(),
                max_size: nodes.len().to_string(),
                min_size: nodes.len().to_string(),
            })
            .collect::<Vec<WorkerNodeDataTemplate>>();

        let s3_kubeconfig_bucket = get_s3_kubeconfig_bucket_name(self.id.clone());

        let qovery_api_url = self.options.qovery_api_url.clone();
        let rds_cidr_subnet = self.options.rds_cidr_subnet.clone();
        let documentdb_cidr_subnet = self.options.documentdb_cidr_subnet.clone();
        let elasticsearch_cidr_subnet = self.options.elasticsearch_cidr_subnet.clone();

        let managed_dns_list = vec![self.dns_provider.name()];
        let managed_dns_domains_helm_format = vec![format!("\"{}\"", self.dns_provider.domain())];
        let managed_dns_domains_terraform_format =
            terraform_list_format(vec![self.dns_provider.domain().to_string()]);
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

        // DNS configuration
        context.insert("managed_dns", &managed_dns_list);
        context.insert(
            "managed_dns_domains_helm_format",
            &managed_dns_domains_helm_format,
        );
        context.insert(
            "managed_dns_domains_terraform_format",
            &managed_dns_domains_terraform_format,
        );
        context.insert(
            "managed_dns_resolvers_terraform_format",
            &managed_dns_resolvers_terraform_format,
        );

        match self.dns_provider.kind() {
            dns_provider::Kind::CLOUDFLARE => {
                context.insert("external_dns_provider", "cloudflare");
                context.insert("cloudflare_api_token", self.dns_provider.token());
                context.insert("cloudflare_email", self.dns_provider.account());
            }
        };

        context.insert("dns_email_report", &self.options.tls_email_report); // Pierre suggested renaming to tls_email_report

        // TLS
        let lets_encrypt_url = match self.context.metadata() {
            Some(meta) => match meta.test {
                Some(true) => "https://acme-staging-v02.api.letsencrypt.org/directory",
                _ => "https://acme-v02.api.letsencrypt.org/directory",
            },
            _ => "https://acme-v02.api.letsencrypt.org/directory",
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
            self.cloud_provider()
                .terraform_state_credentials()
                .region
                .as_str(),
        );

        // TODO URGENT change the behavior of self.bucket_name()
        context.insert("aws_region", &self.region.name());
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");
        context.insert(
            "aws_terraform_backend_dynamodb_table",
            "qovery-terrafom-tfstates",
        );
        context.insert("vpc_cidr_block", &vpc_cidr_block.clone());
        context.insert("s3_kubeconfig_bucket", &s3_kubeconfig_bucket);

        // AWS - EKS
        context.insert("eks_cidr_subnet", &eks_cidr_subnet.clone());
        context.insert("eks_cluster_name", &self.name());
        context.insert("eks_cluster_id", self.id());
        context.insert("eks_region_cluster_id", region_cluster_id.as_str());
        context.insert("eks_worker_nodes", &worker_nodes);
        context.insert("eks_zone_a_subnet_blocks", &eks_zone_a_subnet_blocks);
        context.insert("eks_zone_b_subnet_blocks", &eks_zone_b_subnet_blocks);
        context.insert("eks_zone_c_subnet_blocks", &eks_zone_c_subnet_blocks);
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cloudwatch_log_group", &eks_cloudwatch_log_group);
        context.insert(
            "eks_access_cidr_blocks",
            self.options.eks_access_cidr_blocks.as_str(),
        );

        // AWS - RDS
        context.insert("rds_cidr_subnet", &rds_cidr_subnet);
        context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
        context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
        context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);

        // AWS - DocumentDB
        context.insert("documentdb_cidr_subnet", &documentdb_cidr_subnet);
        context.insert(
            "documentdb_zone_a_subnet_blocks",
            &documentdb_zone_a_subnet_blocks,
        );
        context.insert(
            "documentdb_zone_b_subnet_blocks",
            &documentdb_zone_b_subnet_blocks,
        );

        context.insert(
            "documentdb_zone_c_subnet_blocks",
            &documentdb_zone_c_subnet_blocks,
        );

        // AWS - Elasticsearch
        context.insert(
            "elasticsearch_cidr_subnet",
            &elasticsearch_cidr_subnet.clone(),
        );

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
        context.insert(
            "grafana_admin_user",
            self.options.grafana_admin_user.as_str(),
        );

        context.insert(
            "grafana_admin_password",
            self.options.grafana_admin_password.as_str(),
        );

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
        Kind::EKS
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

    fn is_valid(&self) -> Result<(), KubernetesError> {
        Ok(())
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn resources(&self, environment: &Environment) -> Result<Resources, KubernetesError> {
        let aws = self
            .cloud_provider()
            .as_any()
            .downcast_ref::<AWS>()
            .unwrap();

        let kubernetes_config_file_path = common::kubernetes_config_path(
            self.context.workspace_root_dir(),
            environment.organization_id.as_str(),
            &self.id,
            aws.access_key_id.as_str(),
            aws.secret_access_key.as_str(),
            self.region(),
        )?;

        let aws_credentials_envs = vec![
            (AWS_ACCESS_KEY_ID, aws.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, aws.secret_access_key.as_str()),
        ];

        let nodes =
            cmd::kubectl::kubectl_exec_get_node(kubernetes_config_file_path, aws_credentials_envs)?;

        let mut resources = Resources {
            free_cpu: 0.0,
            max_cpu: 0.0,
            free_ram_in_mib: 0,
            max_ram_in_mib: 0,
            free_pods: 0,
            max_pods: 0,
            running_nodes: 0,
        };

        for node in nodes.items {
            resources.free_cpu += cpu_string_to_float(node.status.allocatable.cpu);
            resources.max_cpu += cpu_string_to_float(node.status.capacity.cpu);
            resources.free_ram_in_mib += ki_to_mi(node.status.allocatable.memory);
            resources.max_ram_in_mib += ki_to_mi(node.status.capacity.memory);
            resources.free_pods = match node.status.allocatable.pods.parse::<u16>() {
                Ok(v) => v,
                _ => 0,
            };
            resources.max_pods = match node.status.capacity.pods.parse::<u16>() {
                Ok(v) => v,
                _ => 0,
            };
            resources.running_nodes += 1;
        }

        Ok(resources)
    }

    fn on_create(&self) -> Result<(), KubernetesError> {
        info!("EKS.on_create() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
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
        let _ = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            &context,
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = crate::template::copy_non_template_files(
            format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
            common_charts_temp_dir.as_str(),
        )?;

        let _ = crate::cmd::terraform::terraform_exec_with_init_validate_plan_apply(
            temp_dir.as_str(),
            true,
        )?;

        Ok(())
    }

    fn on_create_error(&self) -> Result<(), KubernetesError> {
        warn!("EKS.on_create_error() called for {}", self.name());
        // FIXME
        Err(KubernetesError::Error)
    }

    fn on_upgrade(&self) -> Result<(), KubernetesError> {
        info!("EKS.on_upgrade() called for {}", self.name());
        unimplemented!()
    }

    fn on_upgrade_error(&self) -> Result<(), KubernetesError> {
        warn!("EKS.on_upgrade_error() called for {}", self.name());
        unimplemented!()
    }

    fn on_downgrade(&self) -> Result<(), KubernetesError> {
        info!("EKS.on_downgrade() called for {}", self.name());
        unimplemented!()
    }

    fn on_downgrade_error(&self) -> Result<(), KubernetesError> {
        warn!("EKS.on_downgrade_error() called for {}", self.name());
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), KubernetesError> {
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
        let _ = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            &context,
        )?;

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let _ = crate::template::copy_non_template_files(
            format!("{}/common/bootstrap/charts", self.context.lib_root_dir()),
            common_charts_temp_dir.as_str(),
        )?;
        let aws_credentials_envs = vec![
            (
                AWS_ACCESS_KEY_ID,
                self.cloud_provider.access_key_id.as_str(),
            ),
            (
                AWS_SECRET_ACCESS_KEY,
                &self.cloud_provider.secret_access_key.as_str(),
            ),
        ];

        let kubernetes_config_file_path = kubernetes_config_path(
            self.context.workspace_root_dir(),
            self.cloud_provider.organization_id.as_str(),
            self.id(),
            self.cloud_provider.access_key_id.as_str(),
            self.cloud_provider.secret_access_key.as_str(),
            self.region(),
        )?;
        let all_namespaces = kubectl_exec_get_all_namespaces(
            kubernetes_config_file_path,
            aws_credentials_envs.clone(),
        );

        // should make the diff between all namespaces and qovery managed namespaces
        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);
                info!("Deleting non Qovery Namespaces");
                for namespace_to_delete in namespaces_to_delete.iter() {
                    let kubernetes_config_file_path0 = kubernetes_config_path(
                        self.context.workspace_root_dir(),
                        self.cloud_provider.organization_id.as_str(),
                        self.id(),
                        self.cloud_provider.access_key_id.as_str(),
                        self.cloud_provider.secret_access_key.as_str(),
                        self.region(),
                    )?;

                    let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                        &kubernetes_config_file_path0,
                        namespace_to_delete,
                        aws_credentials_envs.clone(),
                    );
                    match deletion {
                        Ok(out) => info!("Namespace {} is deleted", namespace_to_delete),
                        Err(e) => {
                            error!(
                                "Can't delete the namespace {}, quiting now",
                                namespace_to_delete
                            );
                        }
                    }
                }
            }
            Err(e) => error!("Error while getting all namespaces {}", e),
        }
        info!("Deleting Qovery managed Namespaces");
        let kubernetes_config_file_path2 = kubernetes_config_path(
            self.context.workspace_root_dir(),
            self.cloud_provider.organization_id.as_str(),
            self.id(),
            self.cloud_provider.access_key_id.as_str(),
            self.cloud_provider.secret_access_key.as_str(),
            self.region(),
        )?;
        // TODO use label instead fixed names
        let mut qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
            let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                &kubernetes_config_file_path2,
                qovery_namespace,
                aws_credentials_envs.clone(),
            );
            match deletion {
                Ok(out) => info!("Namespace {} is fully deleted", qovery_namespace),
                Err(e) => {
                    error!(
                        "Can't delete the namespace {}, quiting now",
                        qovery_namespace
                    );
                }
            }
        }
        info!("Delete all remaining deployed helm applications");

        match cmd::helm::helm_list(&kubernetes_config_file_path2, aws_credentials_envs.clone()) {
            Ok(helm_list) => {
                cmd::helm::helm_uninstall_list(
                    &kubernetes_config_file_path2,
                    helm_list,
                    aws_credentials_envs.clone(),
                );
            }
            Err(e) => error!("Unable to get helm list"),
        }
        info!("Running Terraform destroy");
        let terraform_result =
            cmd::terraform::terraform_exec_with_init_validate_destroy(temp_dir.as_str())?;
        // we should delete the bucket containing the kubeconfig after
        // to prevent to loose connection from terraform to kube cluster
        match terraform_result {
            () => {
                info!("Deleting S3 Bucket containing Kubeconfig");
                let s3_kubeconfig_bucket = get_s3_kubeconfig_bucket_name(self.id.clone());
                let _region = Region::from_str(self.region()).unwrap();
                s3::delete_bucket(
                    self.cloud_provider.access_key_id.as_str(),
                    self.cloud_provider.secret_access_key.as_str(),
                    s3_kubeconfig_bucket.clone().as_str(),
                );
            }
            _ => {
                error!("Something is wrong with terraform destroy, Kubeconfig S3 location will not be deleting preventing to loose kube accessibility");
            }
        }

        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), KubernetesError> {
        warn!("EKS.on_delete_error() called for {}", self.name());

        // FIXME What should we do if something goes wrong while deleting the cluster?

        Ok(())
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.deploy_environment() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };

        // do not deploy if there is not enough resources
        check_kubernetes_has_enough_resources_to_deploy_environment(self, environment)?;

        // create all stateful services (database)
        for service in &environment.stateful_services {
            let progress_scope = service.progress_scope();

            listeners_helper.start_in_progress(ProgressInfo::new(
                progress_scope.clone(),
                ProgressLevel::Info,
                Some(format!(
                    "let's deploy {} {}",
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                self.context.execution_id(),
            ));

            match service.exec_action(&stateful_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateful service {} , id: {} => {:?}",
                        service.name(),
                        service.id(),
                        err
                    );

                    listeners_helper.error(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Error,
                        Some(format!(
                            "error while deploying {} {} : error => {:?}",
                            service.service_type().name().to_lowercase(),
                            service.name(),
                            err
                        )),
                        self.context.execution_id(),
                    ));

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {
                    listeners_helper.start_in_progress(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Info,
                        Some(format!(
                            "deployment succeeded for {} {}",
                            service.service_type().name().to_lowercase(),
                            service.name()
                        )),
                        self.context.execution_id(),
                    ));
                }
            }
        }

        // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        // create all stateless services (router, application...)
        for service in &environment.stateless_services {
            let progress_scope = service.progress_scope();

            listeners_helper.start_in_progress(ProgressInfo::new(
                progress_scope.clone(),
                ProgressLevel::Info,
                Some(format!(
                    "let's deploy {} {}",
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                self.context.execution_id(),
            ));

            match service.exec_action(&stateless_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateless service {} , id: {} => {:?}",
                        service.name(),
                        service.id(),
                        err
                    );

                    listeners_helper.error(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Error,
                        Some(format!(
                            "error while deploying {} {} : error => {:?}",
                            service.service_type().name().to_lowercase(),
                            service.name(),
                            err
                        )),
                        self.context.execution_id(),
                    ));

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {
                    listeners_helper.start_in_progress(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Info,
                        Some(format!(
                            "deployment succeeded for {} {}",
                            service.service_type().name().to_lowercase(),
                            service.name()
                        )),
                        self.context.execution_id(),
                    ));
                }
            }
        }

        // check all deployed services
        for service in &environment.stateful_services {
            let progress_scope = service.progress_scope();

            listeners_helper.start_in_progress(ProgressInfo::new(
                progress_scope.clone(),
                ProgressLevel::Info,
                Some(format!(
                    "check {} {}",
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                self.context.execution_id(),
            ));

            match service.on_create_check() {
                Err(err) => {
                    error!(
                        "error with stateful service while checking it {} , id: {} => {:?}",
                        service.name(),
                        service.id(),
                        err
                    );

                    listeners_helper.error(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Error,
                        Some(format!(
                            "error while checking {} {} : error => {:?}",
                            service.service_type().name().to_lowercase(),
                            service.name(),
                            err
                        )),
                        self.context.execution_id(),
                    ));

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {
                    listeners_helper.started(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Info,
                        Some(format!(
                            "{} {} is up and running",
                            service.service_type().name().to_lowercase(),
                            service.name()
                        )),
                        self.context.execution_id(),
                    ));
                }
            }
        }

        for service in &environment.stateless_services {
            let progress_scope = service.progress_scope();

            listeners_helper.start_in_progress(ProgressInfo::new(
                progress_scope.clone(),
                ProgressLevel::Info,
                Some(format!(
                    "check {} {}",
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                self.context.execution_id(),
            ));

            match service.on_create_check() {
                Err(err) => {
                    error!(
                        "error with stateless service while checking it {} , id: {} => {:?}",
                        service.name(),
                        service.id(),
                        err
                    );

                    listeners_helper.error(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Error,
                        Some(format!(
                            "error while checking {} {} : error => {:?}",
                            service.service_type().name().to_lowercase(),
                            service.name(),
                            err
                        )),
                        self.context.execution_id(),
                    ));

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {
                    listeners_helper.start_in_progress(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Info,
                        Some(format!(
                            "{} {} is up and running",
                            service.service_type().name().to_lowercase(),
                            service.name()
                        )),
                        self.context.execution_id(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError> {
        warn!("EKS.deploy_environment_error() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Environment {
                id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Warn,
            Some(
                "An error occurred while trying to deploy the environment, so let's revert changes",
            ),
            self.context.execution_id(),
        ));

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };

        // clean up all stateful services (database)
        for service in &environment.stateful_services {
            let progress_scope = service.progress_scope();

            listeners_helper.start_in_progress(ProgressInfo::new(
                progress_scope.clone(),
                ProgressLevel::Info,
                Some(format!(
                    "reverting changes for {} {}",
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                self.context.execution_id(),
            ));

            match service.on_create_error(&stateful_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateful service {} , id: {} => {:?}",
                        service.name(),
                        service.id(),
                        err
                    );

                    listeners_helper.error(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Error,
                        Some(format!(
                            "error while reverting changes for {} {} : error => {:?}",
                            service.service_type().name().to_lowercase(),
                            service.name(),
                            err
                        )),
                        self.context.execution_id(),
                    ));

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {
                    listeners_helper.start_in_progress(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Info,
                        Some(format!(
                            "reverting changes succeeded for {} {}",
                            service.service_type().name().to_lowercase(),
                            service.name()
                        )),
                        self.context.execution_id(),
                    ));
                }
            }
        }

        // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        // clean up all stateless services (router, application...)
        for service in &environment.stateless_services {
            let progress_scope = service.progress_scope();

            listeners_helper.start_in_progress(ProgressInfo::new(
                progress_scope.clone(),
                ProgressLevel::Info,
                Some(format!(
                    "reverting changes for {} {}",
                    service.service_type().name().to_lowercase(),
                    service.name()
                )),
                self.context.execution_id(),
            ));

            match service.on_create_error(&stateless_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateless service {} , id: {} => {:?}",
                        service.name(),
                        service.id(),
                        err
                    );

                    listeners_helper.error(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Error,
                        Some(format!(
                            "error while reverting changes for {} {} : error => {:?}",
                            service.service_type().name().to_lowercase(),
                            service.name(),
                            err
                        )),
                        self.context.execution_id(),
                    ));

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {
                    listeners_helper.start_in_progress(ProgressInfo::new(
                        progress_scope,
                        ProgressLevel::Info,
                        Some(format!(
                            "reverting changes succeeded for {} {}",
                            service.service_type().name().to_lowercase(),
                            service.name()
                        )),
                        self.context.execution_id(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn pause_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.pause_environment() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };

        // create all stateful services (database)
        for stateful_service in &environment.stateful_services {
            match stateful_service.on_pause(&stateful_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateful service {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Pause(err));
                }
                _ => {}
            }
        }

        // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        // create all stateless services (router, application...)
        for stateless_service in &environment.stateless_services {
            match stateless_service.on_pause(&stateless_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateless service {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Pause(err));
                }
                _ => {}
            }
        }

        // check all deployed services
        for stateful_service in &environment.stateful_services {
            match stateful_service.on_pause_check() {
                Err(err) => {
                    error!(
                        "error with stateful service while checking it {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Pause(err));
                }
                _ => {}
            }
        }

        for stateless_service in &environment.stateless_services {
            match stateless_service.on_pause_check() {
                Err(err) => {
                    error!(
                        "error with stateless service while checking it {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Pause(err));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), KubernetesError> {
        warn!("EKS.pause_environment_error() called for {}", self.name());
        Ok(())
    }

    fn delete_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.delete_environment() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);
        // TODO use listeners_helper !!!! Don't be so shy Marc + Pierre

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };

        // delete all stateful services (database)
        for stateful_service in &environment.stateful_services {
            match stateful_service.on_delete(&stateful_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateful service {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Delete(err));
                }
                _ => {}
            }
        }

        // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        // delete all stateless services (router, application...)
        for stateless_service in &environment.stateless_services {
            match stateless_service.on_delete(&stateless_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateless service {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Delete(err));
                }
                _ => {}
            }
        }

        // check all deployed services
        for stateful_service in &environment.stateful_services {
            match stateful_service.on_delete_check() {
                Err(err) => {
                    error!(
                        "error with stateful service while checking it {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Delete(err));
                }
                _ => {}
            }
        }

        for stateless_service in &environment.stateless_services {
            match stateless_service.on_delete_check() {
                Err(err) => {
                    error!(
                        "error with stateless service while checking it {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Delete(err));
                }
                _ => {}
            }
        }

        let aws_credentials_envs = vec![
            (
                AWS_ACCESS_KEY_ID,
                self.cloud_provider.access_key_id.as_str(),
            ),
            (
                AWS_SECRET_ACCESS_KEY,
                &self.cloud_provider.secret_access_key.as_str(),
            ),
        ];

        let kubernetes_config_file_path = common::kubernetes_config_path(
            &self.context.workspace_root_dir(),
            &environment.organization_id.as_str(),
            &self.id,
            &self.cloud_provider.access_key_id.as_str(),
            &self.cloud_provider.secret_access_key.as_str(),
            &self.region.name(),
        )?;

        kubectl_exec_delete_namespace(
            kubernetes_config_file_path,
            &environment.namespace(),
            aws_credentials_envs,
        );

        Ok(())
    }

    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), KubernetesError> {
        warn!("EKS.delete_environment_error() called for {}", self.name());
        Ok(())
    }
}

fn get_s3_kubeconfig_bucket_name(id: String) -> String {
    format!("qovery-kubeconfigs-{}", id)
}

#[derive(Serialize, Deserialize)]
struct WorkerNodeDataTemplate {
    instance_type: String,
    desired_size: String,
    max_size: String,
    min_size: String,
}
