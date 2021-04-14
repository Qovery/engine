use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::digitalocean::kubernetes::node::Node;
use crate::cloud_provider::digitalocean::DO;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesNode};
use crate::cloud_provider::models::WorkerNodeDataTemplate;
use crate::cloud_provider::{kubernetes, CloudProvider};
use crate::cmd::terraform::terraform_init_validate_plan_apply;
use crate::dns_provider;
use crate::dns_provider::DnsProvider;
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, EngineErrorScope};
use crate::fs::workspace_directory;
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::object_storage::spaces::Spaces;
use crate::object_storage::ObjectStorage;
use crate::string::terraform_list_format;

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
    pub qovery_nats_user: String,
    pub qovery_nats_password: String,
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
    spaces: Spaces,
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

        let spaces = Spaces::new(
            context.clone(),
            "spaces-temp-id".to_string(),
            "my-spaces-object-storage".to_string(),
            cloud_provider.spaces_access_id.clone(),
            cloud_provider.spaces_secret_key.clone(),
            region.to_string(),
        );

        DOKS {
            context,
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: region.to_string(),
            cloud_provider,
            dns_provider,
            spaces,
            options,
            nodes,
            template_directory,
            listeners: cloud_provider.listeners.clone(), // copy listeners from CloudProvider
        }
    }

    // create a context to render tf files (terraform) contained in lib/digitalocean/
    fn tera_context(&self) -> TeraContext {
        let mut context = TeraContext::new();

        // OKS
        context.insert("doks_cluster_id", &self.id());
        context.insert("doks_master_name", &self.name());
        context.insert("doks_version", &self.version());

        // Network
        context.insert("vpc_name", self.options.vpc_name.as_str());
        context.insert("vpc_cidr_block", self.options.vpc_cidr_block.as_str());

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

        context.insert("test_cluster", &self.context.is_test_cluster());
        context.insert("qovery_api_url", self.options.qovery_api_url.as_str());
        context.insert("qovery_nats_url", self.options.qovery_nats_url.as_str());
        context.insert("qovery_nats_user", self.options.qovery_nats_user.as_str());
        context.insert("qovery_nats_password", self.options.qovery_nats_password.as_str());
        context.insert("qovery_ssh_key", self.options.qovery_ssh_key.as_str());
        context.insert("discord_api_key", self.options.discord_api_key.as_str());

        // grafana credentials
        context.insert("grafana_admin_user", self.options.grafana_admin_user.as_str());

        context.insert("grafana_admin_password", self.options.grafana_admin_password.as_str());

        // TLS
        let lets_encrypt_url = match self.context.is_test_cluster() {
            true => "https://acme-staging-v02.api.letsencrypt.org/directory",
            false => "https://acme-v02.api.letsencrypt.org/directory",
        };

        context.insert("acme_server_url", lets_encrypt_url);
        context.insert("dns_email_report", &self.options.tls_email_report);

        // DNS management
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

        // Digital Ocean
        context.insert("digitalocean_token", &self.cloud_provider.token);
        context.insert("do_region", &self.region);

        // Spaces Credentials
        context.insert("spaces_access_id", &self.cloud_provider.spaces_access_id);
        context.insert("spaces_secret_key", &self.cloud_provider.spaces_secret_key);

        let space_kubeconfig_bucket = format!("qovery-kubeconfigs-{}", self.id.as_str());
        context.insert("space_bucket_kubeconfig", &space_kubeconfig_bucket);

        // AWS S3 tfstates storage tfstates
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

        context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");

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
                desired_size: "2".to_string(),
                max_size: nodes.len().to_string(),
                min_size: "2".to_string(),
            })
            .collect::<Vec<WorkerNodeDataTemplate>>();

        context.insert("doks_worker_nodes", &worker_nodes);

        context
    }
}

impl<'a> Kubernetes for DOKS<'a> {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Doks
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

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.spaces
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create(&self) -> Result<(), EngineError> {
        info!("DOKS.on_create() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.deployment_in_progress(ProgressInfo::new(
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

        match terraform_init_validate_plan_apply(temp_dir.as_str(), self.context.is_dry_run_deploy()) {
            Ok(_) => Ok(()),
            Err(e) => {
                // print Terraform error logs to end user
                if e.logs.is_some() {
                    let error_lined_joined = e.logs.unwrap().join("\n");
                    listeners_helper.deployment_error(ProgressInfo::new(
                        ProgressScope::Infrastructure {
                            execution_id: self.context.execution_id().to_string(),
                        },
                        ProgressLevel::Error,
                        Some(format!(
                            "Failed to deploy EKS {} cluster with id {}. \n {}",
                            self.name(),
                            self.id(),
                            error_lined_joined
                        )),
                        self.context.execution_id(),
                    ));
                }

                error!("Error while deploying cluster {} with id {}.", self.name(), self.id());
                Err(EngineError::new(
                    EngineErrorCause::Internal,
                    EngineErrorScope::Kubernetes(self.id.clone(), self.name.clone()),
                    self.context.execution_id(),
                    e.message,
                ))
            }
        }
    }

    fn on_create_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_upgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_upgrade_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_downgrade(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_downgrade_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_pause(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn on_pause_error(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn on_delete(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.deploy_environment() called for {}", self.name());
        kubernetes::deploy_environment(self, environment)
    }

    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.deploy_environment_error() called for {}", self.name());
        kubernetes::deploy_environment_error(self, environment)
    }

    fn pause_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.pause_environment() called for {}", self.name());
        kubernetes::pause_environment(self, environment)
    }

    fn pause_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.pause_environment_error() called for {}", self.name());
        Ok(())
    }

    fn delete_environment(&self, environment: &Environment) -> Result<(), EngineError> {
        info!("DOKS.delete_environment() called for {}", self.name());
        kubernetes::delete_environment(self, environment)
    }

    fn delete_environment_error(&self, _environment: &Environment) -> Result<(), EngineError> {
        warn!("DOKS.delete_environment_error() called for {}", self.name());
        Ok(())
    }
}

impl<'a> Listen for DOKS<'a> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}
