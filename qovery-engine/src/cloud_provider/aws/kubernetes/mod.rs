use dirs::home_dir;
use itertools::Itertools;
use rusoto_core::Region;
use rusoto_s3::CreateBucketConfiguration;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::str::FromStr;
use tera::Error as TeraError;
use tera::{Context as TeraContext, Tera};
use walkdir::WalkDir;

use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesError, KubernetesNode};
use crate::cloud_provider::service::{Service, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::{exec_with_envs_and_output, exec_with_output, CmdError};
use crate::fs::workspace_directory;
use crate::models::{Context, Listeners, ListenersHelper, ProgressInfo, ProgressListener};
use crate::{cmd, dynamo_db, fs, s3};
use std::rc::Rc;

pub mod node;

pub struct EKS<'a> {
    context: Context,
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    nodes: Vec<Node>,
    template_directory: String,
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
            nodes,
            template_directory,
            listeners: vec![],
        }
    }

    fn bucket_name(&self) -> String {
        format!("{}-{}-qovery-terraform", self.region.name(), self.id())
    }

    fn tera_context(&self) -> TeraContext {
        let eks_zone_a_subnet_blocks = [
            "10.0.0.0/23",
            "10.0.2.0/23",
            "10.0.4.0/23",
            "10.0.6.0/23",
            "10.0.8.0/23",
            "10.0.10.0/23",
            "10.0.12.0/23",
            "10.0.14.0/23",
            "10.0.16.0/23",
            "10.0.18.0/23",
            "10.0.20.0/23",
            "10.0.22.0/23",
            "10.0.24.0/23",
            "10.0.26.0/23",
            "10.0.28.0/23",
            "10.0.30.0/23",
            "10.0.32.0/23",
            "10.0.34.0/23",
            "10.0.36.0/23",
            "10.0.38.0/23",
            "10.0.40.0/23",
        ]
        .iter()
        .map(|ip| format!("\"{}\"", ip))
        .collect::<Vec<_>>();

        let eks_zone_b_subnet_blocks = [
            "10.0.42.0/23",
            "10.0.44.0/23",
            "10.0.46.0/23",
            "10.0.48.0/23",
            "10.0.50.0/23",
            "10.0.52.0/23",
            "10.0.54.0/23",
            "10.0.56.0/23",
            "10.0.58.0/23",
            "10.0.60.0/23",
            "10.0.62.0/23",
            "10.0.64.0/23",
            "10.0.66.0/23",
            "10.0.68.0/23",
            "10.0.70.0/23",
            "10.0.72.0/23",
            "10.0.74.0/23",
            "10.0.78.0/23",
            "10.0.80.0/23",
            "10.0.82.0/23",
            "10.0.84.0/23",
        ]
        .iter()
        .map(|ip| format!("\"{}\"", ip))
        .collect::<Vec<_>>();

        let eks_zone_c_subnet_blocks = [
            "10.0.86.0/23",
            "10.0.88.0/23",
            "10.0.90.0/23",
            "10.0.92.0/23",
            "10.0.94.0/23",
            "10.0.96.0/23",
            "10.0.98.0/23",
            "10.0.100.0/23",
            "10.0.102.0/23",
            "10.0.104.0/23",
            "10.0.106.0/23",
            "10.0.108.0/23",
            "10.0.110.0/23",
            "10.0.112.0/23",
            "10.0.114.0/23",
            "10.0.116.0/23",
            "10.0.118.0/23",
            "10.0.120.0/23",
            "10.0.122.0/23",
            "10.0.124.0/23",
            "10.0.126.0/23",
        ]
        .iter()
        .map(|ip| format!("\"{}\"", ip))
        .collect::<Vec<_>>();

        let rds_zone_a_subnet_blocks = [
            "10.0.214.0/23",
            "10.0.216.0/23",
            "10.0.218.0/23",
            "10.0.220.0/23",
            "10.0.222.0/23",
            "10.0.224.0/23",
        ]
        .iter()
        .map(|ip| format!("\"{}\"", ip))
        .collect::<Vec<_>>();

        let rds_zone_b_subnet_blocks = [
            "10.0.226.0/23",
            "10.0.228.0/23",
            "10.0.230.0/23",
            "10.0.232.0/23",
            "10.0.234.0/23",
            "10.0.236.0/23",
        ]
        .iter()
        .map(|ip| format!("\"{}\"", ip))
        .collect::<Vec<_>>();

        let rds_zone_c_subnet_blocks = [
            "10.0.238.0/23",
            "10.0.240.0/23",
            "10.0.242.0/23",
            "10.0.244.0/23",
            "10.0.246.0/23",
            "10.0.248.0/23",
        ]
        .iter()
        .map(|ip| format!("\"{}\"", ip))
        .collect::<Vec<_>>();

        let documentdb_zone_a_subnet_blocks = ["10.0.196.0/23", "10.0.198.0/23", "10.0.200.0/23"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let documentdb_zone_b_subnet_blocks = ["10.0.202.0/23", "10.0.204.0/23", "10.0.206.0/23"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let documentdb_zone_c_subnet_blocks = ["10.0.208.0/23", "10.0.210.0/23", "10.0.212.0/23"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let elasticsearch_zone_a_subnet_blocks = ["10.0.184.0/23", "10.0.186.0/23"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let elasticsearch_zone_b_subnet_blocks = ["10.0.188.0/23", "10.0.190.0/23"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let elasticsearch_zone_c_subnet_blocks = ["10.0.192.0/23", "10.0.194.0/23"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>();

        let region_cluster_id = format!("{}-{}", self.region(), self.id());
        let vpc_cidr_block = "10.0.0.0/16";
        let eks_cloudwatch_log_group = format!("/aws/eks/{}/cluster", self.id());
        let eks_cidr_subnet = "23";
        let s3_kubeconfig_bucket = format!("kubeconfigs-{}", self.cloud_provider.organization_id());
        let rds_cidr_subnet = "23";
        let documentdb_cidr_subnet = "23";
        let elasticsearch_cidr_subnet = "23";
        let managed_dns = ["oom.sh"]
            .iter()
            .map(|ip| format!("\"{}\"", ip))
            .collect::<Vec<_>>(); // Todo: make it customizable

        let mut context = TeraContext::new();

        context.insert("organization_id", self.cloud_provider.organization_id());
        context.insert("managed_dns", &managed_dns);

        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);
        context.insert("aws_region", &self.region.name());
        context.insert("aws_terraform_backend_bucket", &self.bucket_name());
        context.insert("aws_terraform_backend_dynamodb_table", &self.bucket_name());
        context.insert("vpc_cidr_block", &vpc_cidr_block);
        context.insert("s3_kubeconfig_bucket", &s3_kubeconfig_bucket);

        context.insert("eks_cidr_subnet", &eks_cidr_subnet);
        context.insert("eks_cluster_name", &self.name());
        context.insert("eks_cluster_id", self.id());
        context.insert("eks_region_cluster_id", region_cluster_id.as_str());
        context.insert("eks_zone_a_subnet_blocks", &eks_zone_a_subnet_blocks);
        context.insert("eks_zone_b_subnet_blocks", &eks_zone_b_subnet_blocks);
        context.insert("eks_zone_c_subnet_blocks", &eks_zone_c_subnet_blocks);
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cloudwatch_log_group", &eks_cloudwatch_log_group);

        context.insert("rds_cidr_subnet", &rds_cidr_subnet);
        context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
        context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
        context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);

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

        context.insert("elasticsearch_cidr_subnet", &elasticsearch_cidr_subnet);
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

        context.insert("eks_worker_nodes", &worker_nodes);

        // Todo: export this, do not let it this way
        // DNS configuration
        context.insert("external_dns_provider", "cloudflare");
        context.insert(
            "cloudflare_api_token",
            "9XhHmPprCG2OgLGhGEFEy7PxzOO_eydnxvtbRLn7",
        );
        context.insert("cloudflare_email", "dns@qovery.com");

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

    fn is_valid(&self) -> Result<(), KubernetesError> {
        // TODO check that terraform binary is available
        Ok(())
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        self.listeners.push(listener);
    }

    fn on_create(&self) -> Result<(), KubernetesError> {
        info!("EKS.on_create() called for {}", self.name());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        listeners_helper.on_progress(ProgressInfo::new(
            "kubernetes",
            0,
            "start to create EKS cluster",
        ));

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("bootstrap/{}", self.name()),
        );

        // create S3 bucket
        let _ = s3::create_bucket(
            self.cloud_provider.access_key_id.as_str(),
            self.cloud_provider.secret_access_key.as_str(),
            self.region.borrow(),
            self.bucket_name().as_str(),
        )?;

        // create dynamo db table
        let _ = dynamo_db::create_terraform_table(
            self.cloud_provider.access_key_id.as_str(),
            self.cloud_provider.secret_access_key.as_str(),
            self.region.borrow(),
            self.bucket_name().as_str(), // bucket name and DynamoDB are the same
        )?;

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

        let temp_dir = workspace_directory(
            self.context.workspace_root_dir(),
            self.context.execution_id(),
            format!("bootstrap/{}", self.name()),
        );

        // TODO delete s3 bucket?
        // TODO delete dynamodb table?

        // generate terraform files and copy them into temp dir
        let context = self.tera_context();
        let _ = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            &context,
        )?;

        let _ = crate::cmd::terraform_exec_destroy(temp_dir.as_str())?;

        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), KubernetesError> {
        warn!("EKS.on_delete_error() called for {}", self.name());

        // FIXME What should we do if something goes wrong while deleting the cluster?

        Ok(())
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.deploy_environment() called for {}", self.name());

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
            match stateful_service.exec_action(&stateful_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateful service {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {}
            }
        }

        // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        // create all stateless services (router, application...)
        for stateless_service in &environment.stateless_services {
            match stateless_service.exec_action(&stateless_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateless service {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {}
            }
        }

        // check all deployed services
        for stateful_service in &environment.stateful_services {
            match stateful_service.on_create_check() {
                Err(err) => {
                    error!(
                        "error with stateful service while checking it {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {}
            }
        }

        for stateless_service in &environment.stateless_services {
            match stateless_service.on_create_check() {
                Err(err) => {
                    error!(
                        "error with stateless service while checking it {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn deploy_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError> {
        warn!("EKS.deploy_environment_error() called for {}", self.name());

        // TODO get output of all pods and send it back through the listener
        // TODO helm uninstall for each stateless service

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };

        // clean up all stateful services (database)
        for stateful_service in &environment.stateful_services {
            // TODO add multi threading to improve deployment performance - but consider to respect the deployment order
            match stateful_service.on_create_error(&stateful_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateful service {} , id: {} => {:?}",
                        stateful_service.name(),
                        stateful_service.id(),
                        err
                    );

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {}
            }
        }

        // stateless services are deployed on kubernetes, that's why we choose the deployment target SelfHosted.
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        // clean up all stateless services (router, application...)
        for stateless_service in &environment.stateless_services {
            // TODO add multi threading to improve deployment performance - but consider to respect the deployment order
            match stateless_service.on_create_error(&stateless_deployment_target) {
                Err(err) => {
                    error!(
                        "error with stateless service {} , id: {} => {:?}",
                        stateless_service.name(),
                        stateless_service.id(),
                        err
                    );

                    return Err(KubernetesError::Deploy(err));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn pause_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.pause_environment() called for {}", self.name());

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

    fn pause_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError> {
        warn!("EKS.pause_environment_error() called for {}", self.name());
        Ok(())
    }

    fn delete_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.delete_environment() called for {}", self.name());

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
        // create all stateless services (router, application...)
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

        Ok(())
    }

    fn delete_environment_error(&self, environment: &Environment) -> Result<(), KubernetesError> {
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
