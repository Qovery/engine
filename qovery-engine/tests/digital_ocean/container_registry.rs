extern crate test_utilities;

use self::test_utilities::aws::context;
use self::test_utilities::digitalocean::docker_cr_do_engine;
use self::test_utilities::utilities::init;
use qovery_engine::container_registry::docr::DOCR;

#[test]
fn create_do_container_registry() {
    init();
    let context = context();
    docker_cr_do_engine(&context);
}

#[test]
fn create_do_repository_on_container_registry() {}

#[test]
fn delete_do_repository_on_container_registry() {}

#[test]
fn push_sample_image_on_container_registry() {}

//
// test --package qovery-engine --test container_registry create_do_container_registry -- --exact
