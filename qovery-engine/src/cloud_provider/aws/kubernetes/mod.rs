use std::borrow::Borrow;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::str::FromStr;

use itertools::Itertools;
use rusoto_core::Region;
use rusoto_s3::CreateBucketConfiguration;
use serde::{Deserialize, Serialize};
use tera::Error as TeraError;
use tera::{Context, Tera};
use walkdir::WalkDir;

use crate::cloud_provider::aws::kubernetes::node::Node;
use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesError, KubernetesNode};
use crate::cloud_provider::service::Service;
use crate::cloud_provider::CloudProvider;
use crate::cmd::{exec_with_envs_and_output, exec_with_output, CmdError};
use crate::fs::{
    copy_bootstrap_files, workspace_directory, write_rendered_templates, RenderedTemplate,
};
use crate::{dynamo_db, s3};
use dirs::home_dir;

pub mod node;

pub struct EKS<'a> {
    id: String,
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    nodes: Vec<Node>,
    tera: Tera,
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
        let tera_template_string = format!("{}/**/*.j2.*", template_directory);

        let tera = match Tera::new(tera_template_string.as_str()) {
            Ok(t) => t,
            Err(e) => panic!(
                "{} parsing error - does the directory exists?",
                template_directory
            ),
        };

        EKS {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
            nodes,
            tera,
            template_directory,
        }
    }

    fn bucket_name(&self) -> String {
        format!("{}-{}-qovery-terraform", self.region.name(), self.id())
    }

    fn generate_j2_templates(&self) -> Result<Vec<RenderedTemplate>, TeraError> {
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

        let files = WalkDir::new(self.template_directory.as_str())
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.contains(".j2."))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        let mut results: Vec<RenderedTemplate> = vec![];
        for file in files.into_iter() {
            let path_str = file.path().to_str().unwrap();
            let j2_path = path_str.replace(self.template_directory.as_str(), "");

            let j2_file_name = file.file_name().to_str().unwrap();
            let file_name = j2_file_name.replace(".j2", "");

            let content = self.tera.render(&j2_path[1..], &context)?;
            results.push(RenderedTemplate::new(file_name, content));
        }

        Ok(results)
    }

    fn generate_and_copy_bootstrap_files_into_dir<P: AsRef<Path>>(
        &self,
        dest_dir: P,
    ) -> Result<(), Error> {
        // generate j2 templates
        let rendered_templates = match self.generate_j2_templates() {
            Ok(rt) => rt,
            Err(err) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "something goes wrong while generating j2 templates {}",
                ));
            }
        };

        // copy all .tf and .yaml files into our dest directory
        copy_bootstrap_files(
            &Path::new(self.template_directory.as_str()),
            dest_dir.as_ref(),
        )?;

        write_rendered_templates(&rendered_templates, dest_dir.as_ref())?;

        Ok(())
    }

    fn terraform_exec(&self, root_dir: &str, args: Vec<&str>) -> Result<(), CmdError> {
        let home_dir = home_dir().unwrap();
        let tf_plugin_cache_dir =
            format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

        match exec_with_envs_and_output(
            format!("{} terraform", root_dir).as_str(),
            args,
            vec![("TF_PLUGIN_CACHE_DIR", tf_plugin_cache_dir.as_str())],
            |line| {
                info!("{}", line.unwrap());
            },
        ) {
            Err(err) => return Err(err),
            _ => {}
        };

        Ok(())
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

        // generate terraform files
        self.generate_and_copy_bootstrap_files_into_dir(&temp_dir)?;

        let on_error = |err: CmdError| {
            match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(es) => return Err(KubernetesError::Create(es)),
            };
        };

        // terraform init
        info!("terraform init on EKS for {}", self.name());
        match self.terraform_exec(
            temp_dir_path_str,
            vec!["init", "-backend-config=backend.tf", "-no-color"],
        ) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform validate config
        info!("terraform validate config on EKS for {}", self.name());
        match self.terraform_exec(temp_dir_path_str, vec!["validate"]) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform plan
        info!("terraform plan on EKS for {}", self.name());
        match self.terraform_exec(
            temp_dir_path_str,
            vec!["plan", "-out", "tf_plan", "-no-color"],
        ) {
            Err(err) => return on_error(err),
            _ => {}
        };

        // terraform apply
        info!("terraform apply on EKS for {}", self.name());
        match self.terraform_exec(
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

    fn services(&self) -> Result<Vec<Box<dyn Service>>, KubernetesError> {
        unimplemented!()
    }

    fn create_service(&self, service: &dyn Service) -> Result<(), KubernetesError> {
        info!("EKS.create_service() called for {}", self.name());
        Err(KubernetesError::Error)
    }

    fn delete_service(&self, service: &dyn Service) -> Result<(), KubernetesError> {
        info!("EKS.delete_service() called for {}", self.name());
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
        assert_eq!(eks.generate_j2_templates().is_ok(), true);
    }

    #[test]
    fn test_write_terraform_files_into_dir() {
        let aws = aws();
        let nodes = nodes();

        let eks = EKS::new("123abc", "test-cluster", "1.14", "eu-west-3", &aws, nodes);
        assert_eq!(
            eks.generate_and_copy_bootstrap_files_into_dir("/tmp/xxx")
                .is_ok(),
            true
        );
    }
}
