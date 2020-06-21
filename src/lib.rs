use crate::build_platform::BuildPlatform;
use crate::cloud_provider::CloudProvider;

mod build_platform;
mod cloud_provider;
mod cmd;
mod config;
mod git;
mod models;
mod session;
mod transaction;

#[cfg(test)]
mod tests {
    use crate::build_platform::local_docker::LocalDocker;
    use crate::build_platform::registry::docker_hub::DockerHub;
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::do_launch_workflow;
    use crate::cloud_provider::gcp::GCP;
    use crate::config::{Config, ConfigError};
    use crate::models::{
        Action, Application, CloudProvider, Deployment, Environment, GitCredentials,
    };
    use crate::session::Session;
    use crate::transaction::{ProgressInfo, ProgressListener};
    use chrono::Utc;

    struct QoveryStatusSender;

    impl ProgressListener for QoveryStatusSender {
        fn on_progress(&self, info: &ProgressInfo) {
            unimplemented!()
        }

        fn on_complete(&self, info: &ProgressInfo) {
            unimplemented!()
        }

        fn on_error(&self, info: &ProgressInfo) {
            unimplemented!()
        }
    }

    #[test]
    fn test_deploy() {
        //let config = Config::<EKS>::from_json("{}");

        let environment = Environment {
            deployment: Deployment {
                id: "".to_string(),
                created_at: Utc::now(),
            },
            owner_id: "".to_string(),
            project_id: "".to_string(),
            environment_id: "".to_string(),
            environment_type: "".to_string(),
            action: Action::Create,
            cloud_provider: CloudProvider {
                name: "".to_string(),
                region: "".to_string(),
            },
            applications: vec![Application {
                name: "simple-example-node-with-postgresql".to_string(),
                git_url: "https://github.com/Qovery/simple-example-node-with-postgresql.git"
                    .to_string(),
                commit_id: "f400e2f199e6a7eb446690b6f2df1017dbbae518".to_string(),
                action: Action::Create,
                git_credentials: GitCredentials {
                    login: "x-access-token".to_string(),
                    access_token: "".to_string(),
                    expired_at: Utc::now(),
                },
                storage: vec![],
            }],
            routers: vec![],
            databases: vec![],
        };

        let registry = Box::new(DockerHub {
            login: "toto",
            password: "password",
        });

        let build_platform = Box::new(LocalDocker { registry });

        let kubernetes = Box::new(EKS {
            id: "",
            name: "",
            version: "",
        });

        let cloud_provider = Box::new(AWS {
            access_key_id: "",
            secret_access_key: "",
            kubernetes,
        });

        let config = Config {
            environment,
            build_platform,
            cloud_provider,
        };

        let session = match config.session() {
            Ok(session) => session,
            Err(err) => panic!(err),
        };

        let mut tx = session.transaction();

        tx.build();
        tx.deploy();

        tx.add_build_listener(Box::new(QoveryStatusSender {}));

        tx.commit();
    }
}
