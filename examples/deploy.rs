use chrono::Utc;
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::error::CloudProviderError;
use qovery_engine::cloud_provider::gcp::GCP;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::config::Config;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{
    Action, Application, CloudProvider as CP, Deployment, Environment, GitCredentials,
};
use qovery_engine::session::Session;
use qovery_engine::transaction::{ProgressInfo, ProgressListener};
use rusoto_core::Region;

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

fn main() {
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
        cloud_provider: CP {
            name: "aws".to_string(),
            region: "us-east-2".to_string(),
        },
        applications: vec![Application {
            name: "simple-example-node-with-postgresql".to_string(),
            git_url: "https://github.com/Qovery/simple-example-node-with-postgresql.git"
                .to_string(),
            commit_id: "f400e2f199e6a7eb446690b6f2df1017dbbae518".to_string(),
            action: Action::Create,
            git_credentials: GitCredentials {
                login: "x-access-token".to_string(),
                access_token: "v1.d6b3b7db582eab1b85df90df5f558ac5830624f9".to_string(),
                expired_at: Utc::now(),
            },
            storage: vec![],
        }],
        routers: vec![],
        databases: vec![],
    };

    let container_registry = Box::new(DockerHub {
        login: "qoveryrd",
        password: "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24",
    });

    let build_platform = Box::new(LocalDocker {});

    let region = environment.cloud_provider.region.clone();

    let cloud_provider = Box::new(AWS::new(
        "AKIAZ4KMLSYJLRGNNFNI",
        "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
    ));

    let config = Config {
        build_platform,
        container_registry,
        cloud_provider,
    };

    let session = match config.session() {
        Ok(session) => session,
        Err(err) => match err {
            ConfigurationError::BuildPlatform(e) => panic!(e),
            ConfigurationError::ContainerRegistry(e) => panic!(e),
            ConfigurationError::CloudProvider(e) => match e {
                CloudProviderError::Credentials => panic!("bad cloud provider credentials"),
                CloudProviderError::Error(err) => panic!("qerror: err"),
                CloudProviderError::Unknown => panic!("cloud provider unknown error"),
            },
        },
    };

    let mut tx = session.transaction();

    let eks = EKS::new("my-k8s-cluster", "1.16", region.as_str());
    tx.create_kubernetes(&eks);

    match tx.build(&environment) {
        Ok(_) => {}
        Err(err) => panic!("environment error"),
    }

    tx.deploy(&environment);

    tx.add_build_listener(Box::new(QoveryStatusSender {}));

    tx.commit();
}
