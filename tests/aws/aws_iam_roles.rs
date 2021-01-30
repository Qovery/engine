use qovery_engine::cloud_provider::aws::kubernetes::roles::get_default_roles_to_create;
use qovery_engine::cloud_provider::aws::kubernetes::roles::Role;
use qovery_engine::error::SimpleError;
use test_utilities::aws::{aws_access_key_id, aws_secret_key};
use test_utilities::utilities::init;
use tracing::{error, span, Level};

#[test]
fn create_already_exist_role() {
    init();

    let span = span!(Level::INFO, "create_already_exist_role");
    let _enter = span.enter();
    let already_created_roles = get_default_roles_to_create();
    for role in already_created_roles {
        match role
            .create_service_linked_role(aws_access_key_id().as_str(), aws_secret_key().as_str())
        {
            Ok(true) => {
                assert!(true)
            }
            Ok(false) => {
                assert!(false)
            }
            Err(e) => {
                error!("{:?}", e);
                assert!(false);
                assert!(false);
            }
        }
    }
}
