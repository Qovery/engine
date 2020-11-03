extern crate test_utilities;
use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::init;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::transaction::TransactionResult;
use std::fs::File;
use std::io::Read;
use test_utilities::digitalocean::DO_KUBERNETES_VERSION;

#[test]
fn create_doks_cluster_in_fra_1() {
    init();

    let context = test_utilities::utilities::context();

    let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let digitalo = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
    let nodes = test_utilities::digitalocean::do_kubernetes_nodes();

    let cloudflare = dns_provider_cloudflare(&context);

    let mut file = File::open("tests/assets/do-options.json").unwrap();
    let mut read_buf = String::new();
    file.read_to_string(&mut read_buf).unwrap();

    let options_result = serde_json::from_str::<
        qovery_engine::cloud_provider::digitalocean::kubernetes::Options,
    >(read_buf.as_str());

    let kubernetes = DOKS::new(
        context,
        "my-first-doks",
        "do-kube-cluster-fra1",
        DO_KUBERNETES_VERSION,
        "fra1",
        &digitalo,
        &cloudflare,
        options_result.expect("Oh my satan an error in test... Options options options"),
        nodes,
    );
    match tx.create_kubernetes(&kubernetes) {
        Err(err) => panic!("{:?}", err),
        _ => {}
    }
    tx.commit();

    //TODO: put assert on it
}
