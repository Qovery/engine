use self::rusoto_iam::{
    CreateServiceLinkedRoleRequest, GetRoleError, GetRoleRequest, GetRoleResponse, Iam, IamClient,
};
use crate::error::{EngineError, SimpleError, SimpleErrorKind};
use crate::models::Context;
use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use tokio::macros::support::Future;
use tokio::runtime::Runtime;

extern crate rusoto_iam;

pub struct Role {
    role_name: String,
    service_name: String,
    description: String,
}

pub fn get_default_roles_to_create() -> Vec<Role> {
    let mut defaults_role_to_create: Vec<Role> = Vec::new();
    defaults_role_to_create.push(Role {
        role_name: "create_elasticsearch_role_for_aws_service".to_string(),
        service_name: "AWSServiceRoleForAmazonElasticsearchService".to_string(),
        description: "role permissions policy allows Amazon ES to complete create, delete, describe,  modify on ec2 and elb".to_string(),
    });
    defaults_role_to_create
}

impl Role {
    pub fn new(role_name: String, service_name: String, description: String) -> Self {
        Role {
            role_name,
            service_name,
            description,
        }
    }

    pub async fn is_exist(&self) -> bool {
        let credentials = StaticProvider::new(
            access_key_id.to_string(),
            secret_access_key.to_string(),
            None,
            None,
        );
        let client = Client::new_with(credentials, HttpClient::new().unwrap());
        let iam_client = IamClient::new_with_client(client, Region::default());
        let role = iam_client.get_role(GetRoleRequest { role_name }).await;
        return match role {
            Ok(_) => true,
            Err(_) => false,
        };
    }

    pub fn create_service_linked_role(&self) -> Result<(), SimpleError> {
        let future_is_exist = self.is_exist();
        Runtime::new()
            .expect("Failed to create Tokio runtime to check if role exist")
            .block_on(future_is_exist);
        return match future_is_exist {
            true => {
                info!("Role {} already exist, nothing to do", &self.role_name);
                Ok(())
            }
            false => {
                info!("Role {} doesn't exist, let's create it !", &self.role_name);
                let client = Client::new_with(credentials, HttpClient::new().unwrap());
                let iam_client = IamClient::new_with_client(client, Region::default());
                iam_client.create_service_linked_role(CreateServiceLinkedRoleRequest {
                    aws_service_name: self.service_name.clone(),
                    custom_suffix: None,
                    description: Some(self.description.clone()),
                });
                Ok(())
            }
            _ => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(format!("Unable to check if role {} exist", &self.role_name)),
            )),
        };
    }
}
