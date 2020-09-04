use qovery_engine::container_registry::DOCR;
extern crate test_utilities;

#[test]
fn create_do_container_registry() {
    test_utilities::init();
    let context = context();
    test_utilities::docker_cr_do_engine(&context);
}

#[test]
fn create_do_repository_on_container_registry() {}

#[test]
fn delete_do_repository_on_container_registry() {}

#[test]
fn push_sample_image_on_container_registry() {}

//
// test --package qovery-engine --test container_registry create_do_container_registry -- --exact
