use chrono::Utc;

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::aws::kubernetes::{EKS, Options};
use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
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
    );/*

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
    tx.create_kubernetes(&eks);

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
