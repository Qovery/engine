use std::io::{Error, ErrorKind};

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_dynamodb::{
    AttributeDefinition, CreateTableError, CreateTableInput, DynamoDb, DynamoDbClient,
    KeySchemaElement,
};

use crate::runtime::async_run;

pub fn create_terraform_table(
    access_key_id: &str,
    secret_access_key: &str,
    region: &Region,
    table_name: &str,
) -> Result<(), Error> {
    let access_key_id = access_key_id.to_string();
    let secret_access_key = secret_access_key.to_string();
    let table_name = table_name.to_string();

    let credentials = StaticProvider::new(access_key_id, secret_access_key, None, None);
    let client = Client::new_with(credentials, HttpClient::new().unwrap());
    let ddb_client = DynamoDbClient::new_with_client(client, region.clone());

    let mut cti = CreateTableInput::default();
    cti.table_name = table_name;
    cti.billing_mode = Some("PAY_PER_REQUEST".to_string());

    cti.key_schema = vec![KeySchemaElement {
        attribute_name: "LockID".to_string(),
        key_type: "HASH".to_string(),
    }];

    cti.attribute_definitions = vec![AttributeDefinition {
        attribute_name: "LockID".to_string(),
        attribute_type: "S".to_string(),
    }];

    let r = async_run(ddb_client.create_table(cti));

    // FIXME: return a custom DynamoDBError?
    match r {
        Err(err) => match err {
            RusotoError::Unknown(r) => {
                error!("{}", r.body_as_str());
                Err(Error::new(ErrorKind::Other, r.body_as_str()))
            }
            RusotoError::Service(r) => match r {
                CreateTableError::ResourceInUse(_) => Ok(()), // table already exists
                _ => {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "something goes wrong while creating terraform DynamoDB table",
                    ));
                }
            },
            _ => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "something goes wrong while creating terraform DynamoDB table",
                ));
            }
        },
        Ok(_x) => Ok(()),
    }
}
