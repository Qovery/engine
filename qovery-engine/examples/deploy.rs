use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::engine::Engine;
use qovery_engine::error::ConfigurationError;
use qovery_engine::models::EnvironmentAction;
use qovery_engine::transaction::TransactionResult;

fn main() {
    env_logger::init();

    let execution_id = test_utilities::execution_id();

    let engine = test_utilities::docker_ecr_aws_engine(execution_id.as_str());

    let session = match engine.session() {
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

    let environment = test_utilities::working_environment(execution_id.as_str());

    let environment_action = EnvironmentAction::Environment(environment);

    let eks = test_utilities::aws_kubernetes_eks(
        execution_id.as_str(),
        &cloud_provider,
        test_utilities::aws_kubernetes_nodes(),
    );

    tx.deploy_environment(&eks, &environment_action);

    match tx.commit() {
        TransactionResult::Ok => println!("execution: ok"),
        TransactionResult::Rollback(c) => println!("execution: rollback"),
        TransactionResult::UnrecoverableError(c, r) => println!("execution: unrecoverable error"),
    };
}
