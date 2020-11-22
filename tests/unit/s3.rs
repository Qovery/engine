use qovery_engine::s3;
use qovery_engine::s3::{delete_bucket};
use test_utilities::aws::{aws_access_key_id, aws_secret_access_key};
use test_utilities::utilities::init;

#[test]
fn delete_s3_bucket() {
    init();
    let bucket_name = "my-test-bucket";

    let creation = s3::create_bucket(
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        bucket_name,
    );
    match creation {
        Ok(_) => println!("Yippee Ki Yay"),
        Err(e) => println!("While creating the bucket {}", e.message.unwrap()),
    }

    let delete = delete_bucket(
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        bucket_name,
    );
    match delete {
        Ok(_out) => println!("Yippee Ki Yay"),
        Err(e) => println!("While deleting the bucket {}", e.message.unwrap()),
    }
}
