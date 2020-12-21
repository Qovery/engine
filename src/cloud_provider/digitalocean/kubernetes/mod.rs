use crate::cloud_provider::common::worker_node_data_template::WorkerNodeDataTemplate;
use crate::cloud_provider::digitalocean::kubernetes::node::Node;
use crate::cloud_provider::digitalocean::DO;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesNode, Resources};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::error::{cast_simple_error_to_engine_error, EngineError};
use crate::fs::workspace_directory;
use crate::models::{
    Context, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressListener,
    ProgressScope,
};
use crate::string::terraform_list_format;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::thread;
use tera::Context as TeraContext;

pub mod cidr;
pub mod node;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Options {
    // Digital Ocean
    pub vpc_cidr_block: String,
    pub vpc_name: String,
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

pub struct DOKS<'a> {
    context: Context,
    id: String,
    name: String,
    version: String,
    region: String,
    cloud_provider: &'a DO,
    nodes: Vec<Node>,
    dns_provider: &'a dyn DnsProvider,
    template_directory: String,
    options: Options,
    listeners: Listeners,
}

impl<'a> DOKS<'a> {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        version: &str,
        region: &str,
        cloud_provider: &'a DO,
        dns_provider: &'a dyn DnsProvider,
        options: Options,
        nodes: Vec<Node>,
    ) -> Self {
        let template_directory = format!("{}/digitalocean/bootstrap", context.lib_root_dir());

        DOKS {
            context,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: region.to_string(),
            cloud_provider,
            dns_provider,
            options,
            nodes,
            template_directory,
            listeners: vec![],
        }
    }

    fn remove_whitespace(s: &mut String) {
        s.retain(|c| !c.is_whitespace());
    }

    // create a context to render tf files (terraform) contained in lib/digitalocan/
    fn tera_context(&self) -> TeraContext {
        let mut context = TeraContext::new();

        // Basics
        let test_cluster = match self.context.metadata() {
            Some(meta) => match meta.test {
                Some(true) => true,
                _ => false,
            },
            _ => false,
        };

        // OKS
        context.insert("oks_cluster_id", &self.id());
        context.insert("oks_master_name", &self.name());
        context.insert("oks_version", &self.version());
        context.insert("oks_master_size", "s-4vcpu-8gb");

        // Network
        let vpc_name = &self.options.vpc_name;
        context.insert("vpc_name", vpc_name);
        let vpc_cidr_block = self.options.vpc_cidr_block.clone();
        context.insert("vpc_cidr_block", &vpc_cidr_block);

        // Qovery
        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert(
            "engine_version_controller_token",
            &self.options.engine_version_controller_token,
        );
        context.insert(
            "agent_version_controller_token",
            &self.options.agent_version_controller_token,
        );
        context.insert("test_cluster", &test_cluster);
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_ssh_key", self.options.qovery_ssh_key.as_str());
        context.insert("discord_api_key", self.options.discord_api_key.as_str());

        // grafana credentials
        context.insert(
            "grafana_admin_user",
            self.options.grafana_admin_user.as_str(),
        );

        context.insert(
            "grafana_admin_password",
            self.options.grafana_admin_password.as_str(),
        );

        // TLS
        let lets_encrypt_url = match &test_cluster {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };
        context.insert("acme_server_url", lets_encrypt_url);
        context.insert("dns_email_report", &self.options.tls_email_report);

        // DNS management
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

        // Digital Ocean
        context.insert("digitalocean_token", &self.cloud_provider.token);
        context.insert("do_region", &self.region);

        // Sapces Credentiales
        context.insert("spaces_access_id", &self.cloud_provider.spaces_access_id);
        context.insert("spaces_secret_key", &self.cloud_provider.spaces_secret_key);
        let space_kubeconfig_bucket = get_space_bucket_kubeconfig_name(self.id.clone());
        context.insert("space_bucket_kubeconfig", &space_kubeconfig_bucket);

        // AWS S3 tfstate storage tfstates
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

        context.insert(
            "aws_terraform_backend_dynamodb_table",
            "qovery-terrafom-tfstates",
        );
        context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");

        // kubernetes workers
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
        context.insert("oks_worker_nodes", &worker_nodes);

        context
    }
}

pub fn get_space_bucket_kubeconfig_name(id: String) -> String {
    format!("qovery-kubeconfigs-{}", id)
}

impl<'a> Kubernetes for DOKS<'a> {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::DOKS
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
        self.region.as_str()
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        self.dns_provider
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        self.listeners.push(listener);
    }

    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn resources(&self, environment: &Environment) -> Result<Resources, EngineError> {
        unimplemented!()
    }

    fn on_create(&self) -> Result<(), EngineError> {
        info!(
            "DigitalOceaan kube cluster.on_create() called for {}",
            self.name()
        );

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Infrastructure {
                execution_id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "start to create Digital Ocean Kubernetes cluster {} with id {}",
                self.name(),
                self.id()
            )),
            self.context.execution_id(),
        ));

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("digitalocean/bootstrap/{}", self.name()),
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
        Ok(())
    }

    fn on_upgrade(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_upgrade_error(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_downgrade(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_downgrade_error(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.deploy_environment() called for {}", self.name());
        let listeners_helper = ListenersHelper::new(&self.listeners);

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };
        //TODO: Do I have enough ressources to run this ?

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

                    return Err(err);
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
        // TODO: create all stateless services (router, application...)
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

                    return Err(err);
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
        //TODO: check stateless services are well deployed
        Ok(())
    }

    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.deploy_environment_error() called for {}", self.name());

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

                    return Err(err);
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

        // Quick fix: adding 100 ms delay to avoid race condition on service status update
        thread::sleep(std::time::Duration::from_millis(100));

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

                    return Err(err);
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

    fn pause_environment(&self, _environment: &Environment) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn delete_environment(&self, _environment: &Environment) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        unimplemented!()
    }
}
