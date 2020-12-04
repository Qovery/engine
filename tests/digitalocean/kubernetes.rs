extern crate test_utilities;
use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::digitalocean::{
    digital_ocean_spaces_access_id, digital_ocean_spaces_secret_key, digital_ocean_token,
    get_kube_cluster_name_from_uuid,
};
use self::test_utilities::utilities::{generate_id, init};
use qovery_engine::cloud_provider::digitalocean::api_structs::clusters::Clusters;
use qovery_engine::cloud_provider::digitalocean::common::{
    get_uuid_of_cluster_from_name, kubernetes_config_path,
};
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cmd::kubectl::{kubectl_exec_create_namespace, kubectl_exec_delete_namespace};
use qovery_engine::constants::DIGITAL_OCEAN_TOKEN;
use qovery_engine::container_registry::docr::{get_current_registry_name, get_header_with_bearer};
use qovery_engine::error::SimpleError;
use qovery_engine::transaction::TransactionResult;
use reqwest::StatusCode;
use std::fs::File;
use std::io::Read;
use test_utilities::digitalocean::DO_KUBERNETES_VERSION;
use tracing::{debug, error, info, span, warn, Level};

#[test]
fn create_doks_cluster_in_fra_1() {
    init();

    let span = span!(Level::INFO, "create_doks_cluster_in_fra_1");
    let _enter = span.enter();

    let cluster_id = "my-first-doks-1";
    let cluster_name = "do-kube-cluster-fra1-1";
    let region = "fra1";

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
        context.clone(),
        cluster_id.clone(),
        cluster_name.clone(),
        DO_KUBERNETES_VERSION,
        region.clone(),
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

    // TESTING: Kube cluster UUID is OK ?
    let res_uuid =
        get_uuid_of_cluster_from_name(digital_ocean_token().as_str(), cluster_name.clone());
    match res_uuid {
        Ok(uuid) => assert_eq!(
            get_kube_cluster_name_from_uuid(uuid.as_str()),
            cluster_name.clone()
        ),
        Err(e) => {
            error!("{:?}", e.message);
            assert!(false);
        }
    }

    /*
    TESTING: Kubeconfig
    match kubernetes_config_path(
        context.lib_root_dir().clone(),
        cluster_id.clone(),
        region.clone(),
        digital_ocean_spaces_secret_key().as_str(),
        digital_ocean_spaces_access_id().as_str(),
    ){
        Ok(file) => {
            let do_credentials_envs = vec![
                (DIGITAL_OCEAN_TOKEN, digitalo.token.as_str()),
            ];
            // testing kubeconfig file
            let namespace_to_test = generate_id();
            match kubectl_exec_create_namespace(file.clone(), namespace_to_test.clone().as_str(), do_credentials_envs.clone()){
                Ok(_) => {
                    // Delete created namespace
                    match kubectl_exec_delete_namespace(file,namespace_to_test.as_str(),do_credentials_envs.clone()){
                        Ok(_) => assert!(true),
                        Err(_) => assert!(false)
                    }
                }
                Err(_) => { assert!(false)}
            }

        },
        Err(_) => assert!(false)
    }
    */
}
