use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::error::KubernetesError;
use crate::cloud_provider::{CloudProvider, Kubernetes, KubernetesNode, Service};
use crate::cmd::{exec_with_output, CmdError};
use crate::fs::{copy_terraform_files, write_rendered_templates, RenderedTemplate};
use crate::s3::create_bucket;
use itertools::Itertools;
use rusoto_core::Region;
use rusoto_s3::CreateBucketConfiguration;
use serde::{Deserialize, Serialize};
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::str::FromStr;
use tempdir::TempDir;
use tera::Error as TeraError;
use tera::{Context, Tera};

pub mod node;

pub struct EKS<'a> {
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    nodes: &'a Vec<Node>,
    tera: Tera,
}

impl<'a> EKS<'a> {
    pub fn new(
        id: &str,
        name: &str,
        version: &str,
        region: &str,
        cloud_provider: &'a AWS,
        nodes: &'a Vec<Node>,
    ) -> Self {
        let tera = match Tera::new("lib/aws/bootstrap/**/*.j2.tf") {
            Ok(t) => t,
            Err(e) => {
                panic!("lib/aws/bootstrap/**/*.j2.tf parsing error - does the directory exists?")
            }
        };

        EKS {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
            nodes,
            tera,
        }
    }

    fn generate_terraform_templates(&self) -> Result<[RenderedTemplate; 3], TeraError> {
        let mut context = Context::new();
        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);
        context.insert("aws_region", &self.region.name());

        let aws_vars_file_content = self.tera.render("tf-aws-vars.j2.tf", &context)?;

        let mut context = Context::new();
        context.insert("aws_region", &self.region.name());
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cluster_name", &self.name());
        context.insert(
            "eks_region_cluster_name",
            format!("{}-{}", self.name(), self.region()).as_str(),
        );

        let aws_default_vars_file_content = self.tera.render("tf-default-vars.j2.tf", &context)?;

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

        let mut context = Context::new();
        context.insert("eks_worker_nodes", &worker_nodes);

        let eks_workers_nodes_content = self.tera.render("eks-workers-nodes.j2.tf", &context)?;

        // TODO generate eks-workers-nodes.j2.tf

        Ok([
            RenderedTemplate::new("tf-aws-vars.tf", aws_vars_file_content),
            RenderedTemplate::new("tf-default-vars.tf", aws_default_vars_file_content),
            RenderedTemplate::new("eks-workers-nodes.tf", eks_workers_nodes_content),
        ])
    }

    fn generate_and_copy_terraform_files_into_dir<P: AsRef<Path>>(
        &self,
        dest_dir: P,
    ) -> Result<(), Error> {
        // generate terraform templates
        let rendered_templates = match self.generate_terraform_templates() {
            Ok(rt) => rt,
            Err(err) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "something goes wrong while generating terraform templates",
                ));
            }
        };

        // copy all .tf files into our dest directory
        copy_terraform_files("eks-", &Path::new("lib/aws/bootstrap/."), dest_dir.as_ref())?;

        write_rendered_templates(&rendered_templates, dest_dir.as_ref())?;

        Ok(())
    }

    fn terraform_exec(&self, args: Vec<&str>) -> Result<(), CmdError> {
        match exec_with_output("terraform", args, |line| {
            println!("{}", line.unwrap());
        }) {
            Err(err) => return Err(err),
            _ => {}
        };

        Ok(())
    }
}

impl<'a> Kubernetes for EKS<'a> {
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
        let temp_dir = TempDir::new(self.name())?;
        let temp_dir_path_str = temp_dir.path().to_str().unwrap();

        // create S3 bucket
        create_bucket(
            self.cloud_provider.access_key_id.clone(),
            self.cloud_provider.secret_access_key.clone(),
            self.region.clone(),
            format!("{}-{}-qovery-terraform", self.region.name(), self.id()),
        )?;

        // generate terraform files
        self.generate_and_copy_terraform_files_into_dir(&temp_dir)?;

        let on_error = |err: CmdError| {
            match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(es) => return Err(KubernetesError::Create(es)),
            };
        };

        // terraform init
        info!("terraform init on EKS for {}", self.name());
        match self.terraform_exec(vec!["init", "-no-color", temp_dir_path_str]) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform plan
        info!("terraform plan on EKS for {}", self.name());
        match self.terraform_exec(vec!["plan", "-no-color", temp_dir_path_str]) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform apply
        info!("terraform apply on EKS for {}", self.name());
        match self.terraform_exec(vec![
            "apply",
            "-auto-approve",
            "-no-color",
            temp_dir_path_str,
        ]) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // clean temp dir
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_upgrade(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_upgrade_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_downgrade(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_downgrade_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn create_namespace(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn services(&self) -> Result<Vec<Box<dyn Service>>, KubernetesError> {
        unimplemented!()
    }

    fn create_service(&self, service: &dyn Service) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn delete_service(&self, service: &dyn Service) -> Result<(), KubernetesError> {
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
    use crate::cloud_provider::aws::kubernetes::node::Node;
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::CloudProvider;
    use std::path::Path;

    fn aws() -> AWS {
        let aws = AWS::new(
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

        let eks = EKS::new("123abc", "test-cluster", "1.14", "eu-west-3", &aws, &nodes);
        assert_eq!(eks.generate_terraform_templates().is_ok(), true);
    }

    #[test]
    fn test_write_terraform_files_into_dir() {
        let aws = aws();
        let nodes = nodes();

        let eks = EKS::new("123abc", "test-cluster", "1.14", "eu-west-3", &aws, &nodes);
        assert_eq!(
            eks.generate_and_copy_terraform_files_into_dir("/tmp/coco")
                .is_ok(),
            true
        );
    }
}
