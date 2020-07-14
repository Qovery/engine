use std::borrow::Borrow;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::str::FromStr;

use dirs::home_dir;
use itertools::Itertools;
use rusoto_core::Region;
use rusoto_s3::CreateBucketConfiguration;
use serde::{Deserialize, Serialize};
use tera::Error as TeraError;
use tera::{Context, Tera};
use walkdir::WalkDir;

use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesError, KubernetesNode};
use crate::cloud_provider::service::Service;
use crate::cloud_provider::{CloudProvider, DeploymentTarget};
use crate::cmd::{exec_with_envs_and_output, exec_with_output, CmdError};
use crate::fs::{
    copy_non_template_files, workspace_directory, write_rendered_templates, RenderedTemplate,
};
use crate::{cmd, dynamo_db, fs, s3};

pub mod node;

pub struct EKS<'a> {
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    nodes: Vec<Node>,
    template_directory: String,
}

impl<'a> EKS<'a> {
    pub fn new(
        id: &str,
        name: &str,
        version: &str,
        region: &str,
        cloud_provider: &'a AWS,
        nodes: Vec<Node>,
    ) -> Self {
        let template_directory = "lib/aws/bootstrap".to_string();

        EKS {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
            nodes,
            template_directory,
        }
    }

    fn bucket_name(&self) -> String {
        format!("{}-{}-qovery-terraform", self.region.name(), self.id())
    }

    fn context(&self) -> Context {
        let region_cluster_id = format!("{}-{}-{}", self.region(), self.name(), self.id());
        let vpc_cidr_block = "10.0.0.0/16";
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
        ];
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
        ];
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
        ];

        let mut context = Context::new();
        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);
        context.insert("aws_region", &self.region.name());
        context.insert("vpc_cidr_block", &vpc_cidr_block);
        context.insert("eks_zone_a_subnet_blocks", &eks_zone_a_subnet_blocks);
        context.insert("eks_zone_b_subnet_blocks", &eks_zone_b_subnet_blocks);
        context.insert("eks_zone_c_subnet_blocks", &eks_zone_c_subnet_blocks);
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cluster_name", &self.name());
        context.insert("region_cluster_id", region_cluster_id.as_str());
        context.insert("aws_terraform_backend_bucket", &self.bucket_name());
        context.insert("aws_terraform_backend_dynamodb_table", &self.bucket_name());

        let worker_nodes = self
            .nodes
            .iter()
            .group_by(|e| e.instance_type())
            .into_iter()
            .map(|(instance_type, group)| (instance_type, group.collect::<Vec<_>>()))
            .map(|(instance_type, nodes)| WorkerNodeData {
                instance_type: instance_type.to_string(),
                desired_size: nodes.len().to_string(),
                max_size: nodes.len().to_string(),
                min_size: nodes.len().to_string(),
            })
            .collect::<Vec<WorkerNodeData>>();

        context.insert("eks_worker_nodes", &worker_nodes);

        context
    }
}

impl<'a> Kubernetes for EKS<'a> {
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

    fn on_create(&self) -> Result<(), KubernetesError> {
        info!("EKS.on_create() called for {}", self.name());
        let temp_dir = workspace_directory(format!("bootstrap/{}", self.name()));
        let temp_dir_path_str = temp_dir.as_str();

        // create S3 bucket
        s3::create_bucket(
            self.cloud_provider.access_key_id.as_str(),
            self.cloud_provider.secret_access_key.as_str(),
            self.region.borrow(),
            self.bucket_name().as_str(),
        )?;

        // create dynamo db table
        dynamo_db::create_terraform_table(
            self.cloud_provider.access_key_id.as_str(),
            self.cloud_provider.secret_access_key.as_str(),
            self.region.borrow(),
            self.bucket_name().as_str(), // bucket name and DynamoDB are the same
        )?;

        // generate terraform files and copy them into temp dir
        let context = self.context();
        let _ = crate::fs::generate_and_copy_j2_files_into_dir(
            self.template_directory.as_str(),
            &temp_dir,
            &context,
        )?;

        let on_error = |err: CmdError| {
            match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(es) => return Err(KubernetesError::Create(es)),
            };
        };

        // terraform init
        info!("terraform init on EKS for {}", self.name());
        match cmd::terraform_exec(
            temp_dir_path_str,
            vec!["init", "-backend-config=backend.tf", "-no-color"],
        ) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform validate config
        info!("terraform validate config on EKS for {}", self.name());
        match cmd::terraform_exec(temp_dir_path_str, vec!["validate"]) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform plan
        info!("terraform plan on EKS for {}", self.name());
        match cmd::terraform_exec(
            temp_dir_path_str,
            vec!["plan", "-out", "tf_plan", "-no-color"],
        ) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform apply
        info!("terraform apply on EKS for {}", self.name());
        match cmd::terraform_exec(
            temp_dir_path_str,
            vec!["apply", "-auto-approve", "-no-color", "tf_plan"],
        ) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // clean temp dir
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
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), KubernetesError> {
        warn!("EKS.on_delete_error() called for {}", self.name());
        unimplemented!()
    }

    fn deploy_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        info!("EKS.deploy_environment() called for {}", self.name());
        // TODO create the namespace

        // TODO install the required services (custom domains, agents..) into the namespace (if necessary)

        let stateful_deployment_target = match environment.kind {
            crate::cloud_provider::environment::Kind::Production => {
                DeploymentTarget::ManagedServices(self, environment)
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self, environment)
            }
        };

        // create all stateful services
        for env in &environment.stateful_services {
            env.on_create(&stateful_deployment_target); // TODO handle err
        }

        // create all stateless services
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self, environment);
        for env in &environment.stateless_services {
            env.on_create(&stateless_deployment_target); // TODO handle err
        }

        // TODO wait for pods
        // TODO check custom domain working
        Ok(())
    }

    fn delete_environment(&self, environment: &Environment) -> Result<(), KubernetesError> {
        warn!("EKS.delete_environment() called for {}", self.name());
        // TODO delete the namespace - do services are all deleted?
        unimplemented!()
    }
}

#[derive(Serialize, Deserialize)]
struct WorkerNodeData {
    instance_type: String,
    desired_size: String,
    max_size: String,
    min_size: String,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cloud_provider::aws::kubernetes::node::Node;
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::CloudProvider;

    fn aws() -> AWS {
        let aws = AWS::new(
            "123-abc",
            "my-default-aws",
            "AKIAZ4KMLSYJLRGNNFNI",
            "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
        );

        match aws.is_valid() {
            Err(err) => panic!("something goes wrong with the connection to AWS"),
            _ => {}
        }

        aws
    }

    fn nodes() -> Vec<Node> {
        vec![
            Node::new(2, 4),
            Node::new(2, 4),
            Node::new(2, 4),
            Node::new(1, 2),
        ]
    }
}
