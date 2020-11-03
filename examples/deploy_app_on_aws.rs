use chrono::Utc;

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::aws::kubernetes::{EKS, Options};
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
use qovery_engine::cloud_provider::aws::router::Router;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::engine::Engine;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::{
    Action, Application, Context, Environment, EnvironmentAction, GitCredentials, Kind,
};
use qovery_engine::session::Session;
use qovery_engine::transaction::TransactionResult;

fn main() {
    let context = Context::new("unique-id", "/tmp/qovery-workspace", "lib", None, None);

    // build image with Docker
    let local_docker = LocalDocker::new(context.clone(), "local-docker-id", "local-docker-name");

    // use ECR as Container Registry
    let ecr = ECR::new(
        context.clone(),
        "ecr-id",
        "ecr-name",
        "YOUR AWS ACCESS KEY",
        "YOUR AWS SECRET ACCESS KEY",
        "us-east-1",
    );

    // use cloudflare as DNS provider
    let cloudflare = Cloudflare::new(
        context.clone(),
        "cloudflare-id",
        "cloudflare-name",
        "tld.io",
        "YOUR CLOUDFLARE TOKEN",
        "YOUR CLOUDFLARE EMAIL",
    );

    // use AWS
    let aws = AWS::new(
        context.clone(),
        "aws-id",
        "organization-id",
        "eks-name",
        "YOUR AWS ACCESS KEY",
        "YOUR AWS SECRET ACCESS KEY",
        TerraformStateCredentials::new(
            "YOUR AWS ACCESS KEY",
            "YOUR AWS SECRET ACCESS KEY",
            "us-east-1",
        ),
    );
/*
    let nodes = vec![Node::new(2, 4), Node::new(2, 4), Node::new(2, 4)];

    // use Kubernetes
    let eks = EKS::new(
        context.clone(),
        "eks-id",
        "eks-name",
        "1.16",
        "us-east-1",
        &aws,
        &cloudflare,
        Options::default(),
        nodes,
    );

    let engine = Engine::new(
        context,
        Box::new(local_docker),
        Box::new(ecr),
        Box::new(aws),
        Box::new(cloudflare),
    );

    let session = match engine.session() {
        Ok(session) => session,
        Err(config_error) => match config_error {
            ConfigurationError::BuildPlatform(_) => panic!("build platform config error"),
            ConfigurationError::ContainerRegistry(_) => panic!("container registry config error"),
            ConfigurationError::CloudProvider(_) => panic!("cloud provider config error"),
            ConfigurationError::DnsProvider(_) => panic!("dns provider config error"),
        },
    };

    let mut tx = session.transaction();

    let environment = Environment {
        execution_id: "unique-id".to_string(),
        id: "environment-id".to_string(),
        kind: Kind::Production,
        owner_id: "owner-id".to_string(),
        project_id: "project-id".to_string(),
        organization_id: "organization-id".to_string(),
        action: Action::Create,
        applications: vec![Application {
            id: "app-id-1".to_string(),
            name: "app-name-1".to_string(),
            action: Action::Create,
            git_url: "https://github.com/Qovery/node-simple-example.git".to_string(),
            git_credentials: GitCredentials {
                login: "github-login".to_string(),
                access_token: "github-access-token".to_string(),
                expired_at: Utc::now(),
            },
            branch: "main".to_string(),
            commit_id: "238f7f0454783defa4946613bc17ebbf4ccc514a".to_string(),
            dockerfile_path: "Dockerfile".to_string(),
            private_port: Some(3000),
            total_cpus: "256m".to_string(),
            cpu_burst: "500m".to_string(),
            total_ram_in_mib: 256,
            total_instances: 1,
            storage: vec![],
            environment_variables: vec![],
        }],
        routers: vec![],
        databases: vec![],
        external_services: vec![],
        clone_from_environment_id: None,
    };

    tx.deploy_environment(&eks, &EnvironmentAction::Environment(environment));

    match tx.commit() {
        TransactionResult::Ok => println!("infrastructure initialization OK"),
        TransactionResult::Rollback(commit_err) => {
            println!("infrastructure initialization ERROR and rollback OK")
        }
        TransactionResult::UnrecoverableError(commit_err, rollback_err) => {
            println!("infrastructure initialization ERROR and rollback FAILED")
        }
    };*/
}
