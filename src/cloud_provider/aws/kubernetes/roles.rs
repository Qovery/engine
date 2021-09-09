use self::rusoto_iam::{CreateServiceLinkedRoleRequest, GetRoleRequest, Iam, IamClient};
use crate::error::{SimpleError, SimpleErrorKind};
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use tokio::runtime::Runtime;

extern crate rusoto_iam;

pub struct Role {
    pub role_name: String,
    pub service_name: String,
    pub description: String,
}

pub fn get_default_roles_to_create() -> Vec<Role> {
    vec![Role {
        role_name: "AWSServiceRoleForAmazonElasticsearchService".to_string(),
        service_name: "es.amazonaws.com".to_string(),
        description:
            "role permissions policy allows Amazon ES to complete create, delete, describe,  modify on ec2 and elb"
                .to_string(),
    }]
}

impl Role {
    pub fn new(role_name: String, service_name: String, description: String) -> Self {
        Role {
            role_name,
            service_name,
            description,
        }
    }

    pub async fn is_exist(&self, access_key: &str, secret_key: &str) -> Result<bool, SimpleError> {
        let credentials = StaticProvider::new(access_key.to_string(), secret_key.to_string(), None, None);
        let client = Client::new_with(credentials, HttpClient::new().unwrap());
        let iam_client = IamClient::new_with_client(client, Region::UsEast1);
        let role = iam_client
            .get_role(GetRoleRequest {
                role_name: self.role_name.clone(),
            })
            .await;
        return match role {
            Ok(_) => Ok(true),
            Err(e) => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(format!(
                    "Unable to know if {} exist on AWS Account: {:?}",
                    &self.role_name, e
                )),
            )),
        };
    }

    pub fn create_service_linked_role(&self, access_key: &str, secret_key: &str) -> Result<bool, SimpleError> {
        let future_is_exist = self.is_exist(access_key, secret_key);
        let exist = Runtime::new()
            .expect("Failed to create Tokio runtime to check if role exist")
            .block_on(future_is_exist);
        return match exist {
            Ok(true) => {
                info!("Role {} already exist, nothing to do", &self.role_name);
                Ok(true)
            }
            _ => {
                info!("Role {} doesn't exist, let's create it !", &self.role_name);
                let credentials = StaticProvider::new(access_key.to_string(), secret_key.to_string(), None, None);
                let client = Client::new_with(credentials, HttpClient::new().unwrap());
                let iam_client = IamClient::new_with_client(client, Region::UsEast1);
                let future_create = iam_client.create_service_linked_role(CreateServiceLinkedRoleRequest {
                    aws_service_name: self.service_name.clone(),
                    custom_suffix: None,
                    description: Some(self.description.clone()),
                });
                let created = Runtime::new()
                    .expect("Failed to create Tokio runtime to check if role exist")
                    .block_on(future_create);
                return match created {
                    Ok(_) => Ok(true),
                    Err(e) => Err(SimpleError::new(
                        SimpleErrorKind::Other,
                        Some(format!(
                            "Unable to know if {} exist on AWS Account: {:?}",
                            &self.role_name, e
                        )),
                    )),
                };
            }
        };
    }
}
