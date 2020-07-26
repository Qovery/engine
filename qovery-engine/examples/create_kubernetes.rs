use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::config::Config;
use qovery_engine::error::ConfigurationError;
use qovery_engine::transaction::TransactionResult;

fn main() {
    env_logger::init();

    let execution_id = test_utilities::execution_id();

    // use ECR
    let container_registry = test_utilities::container_registry_ecr(execution_id.as_str());

    // use LocalDocker
    let build_platform = test_utilities::build_platform_local_docker(execution_id.as_str());

    // use AWS
    let cloud_provider = test_utilities::cloud_provider_aws(execution_id.as_str());

    let config = Config::new(&build_platform, &container_registry, &cloud_provider);

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

    let eks_eu_west_3 = test_utilities::aws_kubernetes_eks(
        execution_id.as_str(),
        &cloud_provider,
        test_utilities::aws_kubernetes_nodes(),
    );

    tx.create_kubernetes(&eks_eu_west_3);

    match tx.commit() {
        TransactionResult::Ok => println!("execution: ok"),
        TransactionResult::Rollback(c) => println!("execution: rollback"),
        TransactionResult::UnrecoverableError(c, r) => println!("execution: unrecoverable error"),
    };
}
