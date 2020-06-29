use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::error::KubernetesError;
use crate::cloud_provider::{
    CloudProvider, Create, DatabaseType, Kubernetes, Service, ServiceType, StatefulService,
};
use crate::cmd::exec_with_output;
use crate::fs::{copy_terraform_files, write_rendered_templates, RenderedTemplate};
use rusoto_core::Region;
use std::borrow::Borrow;
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::str::FromStr;
use tempdir::TempDir;
use tera::Error as TeraError;
use tera::{Context, Tera};
use walkdir::WalkDir;

pub struct EKS<'a> {
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a AWS,
    tera: Tera,
}

impl<'a> EKS<'a> {
    pub fn new(name: &str, version: &str, region: &str, cloud_provider: &'a AWS) -> Self {
        let tera = match Tera::new("lib/aws/terraform/**/*.j2.tf") {
            Ok(t) => t,
            Err(e) => {
                panic!("lib/aws/terraform/**/*.j2.tf parsing error - does the directory exists?")
            }
        };

        EKS {
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
            tera,
        }
    }

    fn generate_terraform_templates(&self) -> Result<[RenderedTemplate; 2], TeraError> {
        let mut context = Context::new();
        context.insert("aws_access_key", &self.cloud_provider.access_key_id);
        context.insert("aws_secret_key", &self.cloud_provider.secret_access_key);
        context.insert("aws_region", &self.region.name());

        let aws_vars_file_content = self.tera.render("tf-aws-vars.j2.tf", &context)?;

        let mut context = Context::new();
        context.insert("eks_masters_version", &self.version());
        context.insert("eks_workers_version", &self.version());
        context.insert("eks_cluster_name", &self.name());

        let aws_default_vars_file_content =
            self.tera.render("eks/tf-default-vars.j2.tf", &context)?;

        Ok([
            RenderedTemplate::new("tf-aws-vars.tf", aws_vars_file_content),
            RenderedTemplate::new("tf-default-vars.tf", aws_default_vars_file_content),
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
                ))
            }
        };

        // copy all .tf files into our dest directory
        copy_terraform_files(&Path::new("lib/aws/terraform/eks/."), dest_dir.as_ref())?;

        write_rendered_templates(&rendered_templates, dest_dir.as_ref())?;

        Ok(())
    }
}

impl<'a> Kubernetes for EKS<'a> {
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
        Ok(())
    }

    fn on_create(&self) -> Result<(), KubernetesError> {
        let temp_dir = TempDir::new(self.name())?;

        // generate terraform files
        self.generate_and_copy_terraform_files_into_dir(temp_dir)?;

        // terraform init
        exec_with_output(
            "terraform",
            vec!["init", &temp_dir.path().to_str().unwrap()],
            |line| {
                println!("{}", line.unwrap());
            },
        )?;

        // terraform plan
        exec_with_output(
            "terraform",
            vec!["plan", &temp_dir.path().to_str().unwrap()],
            |line| {
                println!("{}", line.unwrap());
            },
        )?;

        // terraform apply
        exec_with_output(
            "terraform",
            vec!["apply", &temp_dir.path().to_str().unwrap()],
            |line| {
                println!("{}", line.unwrap());
            },
        )?;

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

#[cfg(test)]
mod tests {
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

    #[test]
    fn test_generate_terraform_files() {
        let aws = aws();
        let eks = EKS::new("test-cluster", "1.14", "eu-west-3", &aws);
        assert_eq!(eks.generate_terraform_templates().is_ok(), true);
    }

    #[test]
    fn test_write_terraform_files_into_dir() {
        let aws = aws();
        let eks = EKS::new("test-cluster", "1.14", "eu-west-3", &aws);
        assert_eq!(
            eks.generate_and_copy_terraform_files_into_dir("/tmp/toto")
                .is_ok(),
            true
        );
    }
}
