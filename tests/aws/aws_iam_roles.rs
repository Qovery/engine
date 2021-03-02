use qovery_engine::cloud_provider::aws::kubernetes::roles::get_default_roles_to_create;
use test_utilities::utilities::init;
use tracing::{error, span, Level};

// can't be tested without an AWS access, it can't be enabled inside
// #[test]
// fn create_already_exist_role() {
//     init();
//
//     let span = span!(Level::INFO, "test", name = "create_already_exist_role");
//     let _enter = span.enter();
//     let already_created_roles = get_default_roles_to_create();
//     for role in already_created_roles {
//         match role
//             .create_service_linked_role(aws_access_key_id().as_str(), aws_secret_key().as_str())
//         {
//             Ok(true) => {
//                 assert!(true)
//             }
//             Ok(false) => {
//                 assert!(false)
//             }
//             Err(e) => {
//                 error!("{:?}", e);
//                 assert!(false);
//                 assert!(false);
//             }
//         }
//     }
// }
