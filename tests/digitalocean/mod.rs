use qovery_engine::models::{Context, EnvironmentAction};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use test_utilities::cloudflare::dns_provider_cloudflare;

mod do_databases;
mod do_environment;
pub mod do_kubernetes;

pub fn deploy_environment_on_do(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
    let nodes = test_utilities::digitalocean::do_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = test_utilities::digitalocean::do_kubernetes_ks(&context, &cp, &dns_provider, nodes);

    tx.deploy_environment_with_options(
        &k,
        &environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}
