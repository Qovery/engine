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

        let mut context = Context::new();
        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);
        context.insert("aws_region", &self.region.name());
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
                DeploymentTarget::ManagedServices(self.cloud_provider())
            }
            crate::cloud_provider::environment::Kind::Development => {
                DeploymentTarget::SelfHosted(self)
            }
        };

        // create all stateful services
        for env in &environment.stateful_services {
            env.on_create(&stateful_deployment_target); // TODO handle err
        }

        // create all stateless services
        let stateless_deployment_target = DeploymentTarget::SelfHosted(self);
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

    #[test]
    fn test_generate_terraform_files() {
        let aws = aws();
        let nodes = nodes();

        let eks = EKS::new("123abc", "test-cluster", "1.14", "eu-west-3", &aws, nodes);
        assert_eq!(eks.context("lib/aws/bootstrap").is_ok(), true);
    }
}
