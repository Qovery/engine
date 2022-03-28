use crate::errors::CommandError;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_iam::{Iam, IamClient, ListUsersRequest, ListUsersResponse};
use tokio::runtime::Runtime;

async fn get_users(access_key: &str, secret_key: &str) -> Result<ListUsersResponse, CommandError> {
    let credentials = StaticProvider::new(access_key.to_string(), secret_key.to_string(), None, None);
    let client = Client::new_with(credentials, HttpClient::new().unwrap());
    let iam_client = IamClient::new_with_client(client, Region::UsEast1);
    let users = iam_client
        .list_users(ListUsersRequest {
            marker: None,
            max_items: None,
            path_prefix: None,
        })
        .await;

    match users {
        Ok(users) => Ok(users),
        Err(e) => Err(CommandError::new(
            format!("Unable to get users for AWS UI rendering: {:?}", e),
            Some(format!("Unable to get users for AWS UI rendering: {:?}", e)),
        )),
    }
}

pub fn get_cluster_users(access_key: &str, secret_key: &str) -> Result<Vec<String>, CommandError> {
    let future_result = get_users(access_key, secret_key);
    let result = Runtime::new()
        .expect("Failed to create Tokio runtime to check if users exist")
        .block_on(future_result);

    match result {
        Ok(result) => {
            let mut users: Vec<String> = vec![];
            result
                .users
                .into_iter()
                .filter(|user| !user.user_name.contains("qovery-"))
                .for_each(|user| users.push(user.user_name));
            Ok(users)
        }
        Err(e) => Err(CommandError::new(
            format!("Unable to get users for AWS UI rendering: {:?}", e),
            Some(format!("Unable to get users for AWS UI rendering: {:?}", e)),
        )),
    }
}
