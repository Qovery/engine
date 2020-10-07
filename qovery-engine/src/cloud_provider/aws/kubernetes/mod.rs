use std::borrow::Borrow;
use std::env;
use std::rc::Rc;
use std::str::FromStr;

use itertools::Itertools;
use rusoto_core::Region;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tera::Context as TeraContext;

use crate::cloud_provider::aws::common::do_stateless_service_cleanup;
use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::{common, AWS};
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesError, KubernetesNode};
use crate::cloud_provider::service::{Service, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd;
use crate::cmd::kubectl_exec_delete_namespace;
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::dns_provider::cloudflare::Cloudflare;
use crate::dns_provider::DnsProvider;
use crate::dns_provider::Kind::CLOUDFLARE;
use crate::fs::workspace_directory;
use crate::models::{
    Context, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressListener,
    ProgressScope,
};
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
    pub rds_cidr_subnet: String,
    pub documentdb_cidr_subnet: String,
    pub elasticsearch_cidr_subnet: String,
}

pub struct EKS<'a> {
    context: Context,
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    dns_provider: &'a DnsProvider,
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
        dns_provider: &'a DnsProvider,
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
        let eks_zone_a_subnet_blocks = self
            .options
            .eks_zone_a_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let eks_zone_b_subnet_blocks = self
            .options
            .eks_zone_b_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let eks_zone_c_subnet_blocks = self
            .options
            .eks_zone_c_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let rds_zone_a_subnet_blocks = self
            .options
            .rds_zone_a_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let rds_zone_b_subnet_blocks = self
            .options
            .rds_zone_b_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let rds_zone_c_subnet_blocks = self
            .options
            .rds_zone_c_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let documentdb_zone_a_subnet_blocks = self
            .options
            .documentdb_zone_a_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let documentdb_zone_b_subnet_blocks = self
            .options
            .documentdb_zone_b_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let documentdb_zone_c_subnet_blocks = self
            .options
            .documentdb_zone_c_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let elasticsearch_zone_a_subnet_blocks = self
            .options
            .elasticsearch_zone_a_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let elasticsearch_zone_b_subnet_blocks = self
            .options
            .elasticsearch_zone_b_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let elasticsearch_zone_c_subnet_blocks = self
            .options
            .elasticsearch_zone_c_subnet_blocks
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

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
        let s3_kubeconfig_bucket = format!("qovery-kubeconfigs-{}", self.id);
        let engine_version_controller_token = "3b408f660674cac1494869dec61da35982c1e94d";
        let qovery_api_url = self.options.qovery_api_url.clone();
        let rds_cidr_subnet = self.options.rds_cidr_subnet.clone();
        let documentdb_cidr_subnet = self.options.documentdb_cidr_subnet.clone();
        let elasticsearch_cidr_subnet = self.options.elasticsearch_cidr_subnet.clone();

        let managed_dns_list = vec![self.dns_provider.name()]; // FIXME remove the list
        let managed_dns_helm_format = vec![format!("\"{}\"", self.dns_provider.domain())];
        let managed_dns_terraform_format =
            vec![format!("{{{}}}", self.dns_provider.domain())].join(",");

        let mut context = TeraContext::new();
        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("qovery_api_url", &qovery_api_url);
        context.insert(
            "engine_version_controller_token",
            &engine_version_controller_token,
        );

        // DNS configuration
        context.insert("managed_dns", &managed_dns_list);
        context.insert("managed_dns_helm_format", &managed_dns_helm_format);
        context.insert(
            "managed_dns_terraform_format",
            &managed_dns_terraform_format,
        );

        match self.dns_provider.kind() {
            dns_provider::Kind::CLOUDFLARE => {
                context.insert("external_dns_provider", "cloudflare");
                context.insert("cloudflare_api_token", self.dns_provider.token());
                context.insert("cloudflare_email", self.dns_provider.account());
            }
        };

        context.insert("dns_email_report", "dns@qovery.com"); // Pierre suggested renaming to tls_email_report

        // AWS
        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);
        // AWS S3 tfstate storage
        context.insert("aws_access_key_tfstates_account", "AKIAUD622NVNHD6P2S4Z");
        context.insert(
            "aws_secret_key_tfstates_account",
            "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",
        );
        context.insert("aws_region_tfstates_account", "eu-west-3");
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

        let _ = crate::cmd::terraform_exec_with_init_validate_plan_apply(temp_dir.as_str(), true)?;

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

        let _ = crate::cmd::terraform_exec_with_init_validate_plan_destroy(temp_dir.as_str())?;

        // TODO: delete s3 bucket
        // TODO: delete dynamodb table

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

#[derive(Serialize, Deserialize)]
struct WorkerNodeDataTemplate {
    instance_type: String,
    desired_size: String,
    max_size: String,
    min_size: String,
}
