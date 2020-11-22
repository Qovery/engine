// extern crate test_utilities;
//
// use qovery_engine::build_platform::Image;
// use qovery_engine::container_registry::docr::DOCR;
//
// use self::test_utilities::aws::context;
// use self::test_utilities::digitalocean::docker_cr_do_engine;
// use self::test_utilities::utilities::init;

/*#[test]
#[ignore]
fn create_do_container_registry() {
    init();
    let context = context();
    docker_cr_do_engine(&context);
    let docr = DOCR {
        context,
        registry_name: "qoverytest".to_string(),
        api_key: DIGITAL_OCEAN_TOKEN.to_string(),
    };
    let image_test = Image {
        application_id: "to change".to_string(),
        name: "imageName".to_string(),
        tag: "v666".to_string(),
        commit_id: "sha256".to_string(),
        registry_url: None,
    };
    let repository = DOCR::create_repository(&docr, &image_test);
}
*/
/*#[test]
fn create_do_repository_on_container_registry() {}

#[test]
fn delete_do_repository_on_container_registry() {}

#[test]
fn push_sample_image_on_container_registry() {}*/

//
// test --package qovery-engine --test container_registry create_do_container_registry -- --exact
