use crate::errors::{CommandError, EngineError};
use crate::events::{EventDetails, Transmitter};
use crate::runtime::block_on;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::env;
use uuid::Uuid;
use vaultrs::api::kv2::responses::SecretVersionMetadata;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};
use vaultrs::error::ClientError;
use vaultrs::kv2;
use vaultrs_login::engines::approle::AppRoleLogin;
use vaultrs_login::LoginClient;

pub struct QVaultClient {
    pub(crate) connection: VaultClient,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum VaultAuthType {
    Token,
    AppRole,
    Invalid,
}

impl QVaultClient {
    pub fn get_secret<D: DeserializeOwned>(&self, mount: &str, secret_name_path: &str) -> Result<D, ClientError> {
        block_on(kv2::read(&self.connection, mount, secret_name_path))
    }

    pub fn crate_update_secret<T: Serialize>(
        &self,
        mount: &str,
        secret_name: &str,
        secret_content: &T,
    ) -> Result<SecretVersionMetadata, ClientError> {
        block_on(kv2::set(&self.connection, mount, secret_name, secret_content))
    }

    pub fn delete_secret(&self, mount: &str, secret_name: &str) -> Result<(), ClientError> {
        block_on(kv2::delete_metadata(&self.connection, mount, secret_name))
    }

    fn get_env_var(env_var: &str, event_details: EventDetails) -> Result<String, Box<EngineError>> {
        match env::var_os(env_var) {
            Some(x) => Ok(x.into_string().unwrap_or_else(|_| {
                panic!(
                    "environment variable should have been found but not able to get it: {}",
                    &env_var
                )
            })),
            None => Err(Box::new(EngineError::new_missing_required_env_variable(
                event_details,
                env_var.to_string(),
            ))),
        }
    }

    pub fn detect_auth_type(event_details: EventDetails) -> VaultAuthType {
        let mut auth_type = VaultAuthType::Invalid;

        if Self::get_env_var("VAULT_TOKEN", event_details.clone()).is_ok() {
            auth_type = VaultAuthType::Token;
        } else if Self::get_env_var("VAULT_ROLE_ID", event_details.clone()).is_ok()
            && Self::get_env_var("VAULT_SECRET_ID", event_details).is_ok()
        {
            auth_type = VaultAuthType::AppRole;
        };

        auth_type
    }

    pub fn new(event_details: EventDetails) -> Result<QVaultClient, Box<EngineError>> {
        let event_details = EventDetails::clone_changing_transmitter(
            event_details,
            Transmitter::TaskManager(Uuid::new_v4(), "vault".to_string()),
        );

        let vault_addr = Self::get_env_var("VAULT_ADDR", event_details.clone())?;

        let connection = match Self::detect_auth_type(event_details.clone()) {
            VaultAuthType::Token => {
                let token = Self::get_env_var("VAULT_TOKEN", event_details.clone())?;

                match VaultClient::new(
                    VaultClientSettingsBuilder::default()
                        .address(vault_addr)
                        .token(token.as_str())
                        .build()
                        .expect("errors while using VaultClientSettingsBuilder"),
                ) {
                    Ok(x) => x,
                    Err(e) => {
                        let cmd_error = CommandError::new(
                            "error: wasn't able to contact Vault server".to_string(),
                            Some(format!(
                                "error: wasn't able to contact Vault server with the given token. {e:?}"
                            )),
                            None,
                        );
                        return Err(Box::new(EngineError::new_vault_connection_error(event_details, cmd_error)));
                    }
                }
            }
            VaultAuthType::AppRole => {
                let role_id = Self::get_env_var("VAULT_ROLE_ID", event_details.clone())?;
                let secret_id = Self::get_env_var("VAULT_SECRET_ID", event_details.clone())?;

                let mut client = match VaultClient::new(
                    VaultClientSettingsBuilder::default()
                        .address(vault_addr)
                        .build()
                        .unwrap(),
                ) {
                    Ok(x) => x,
                    Err(e) => {
                        let cmd_error = CommandError::new(
                            "error: wasn't able to contact Vault server".to_string(),
                            Some(format!(
                                "error: wasn't able to contact Vault server with the given role and secret id. {e:?}"
                            )),
                            None,
                        );
                        return Err(Box::new(EngineError::new_vault_connection_error(event_details, cmd_error)));
                    }
                };

                let login = AppRoleLogin { role_id, secret_id };
                let _ = block_on(client.login("approle", &login));

                client
            }
            VaultAuthType::Invalid => {
                let cmd_error = CommandError::new(
                    "error: can't contact Vault server".to_string(),
                    Some("can't contact Vault server with the given connections details".to_string()),
                    None,
                );
                return Err(Box::new(EngineError::new_vault_connection_error(event_details, cmd_error)));
            }
        };

        Ok(QVaultClient { connection })
    }
}

pub fn get_vault_mount_name(is_test_cluster: bool) -> String {
    match is_test_cluster {
        false => "official-clusters-access",
        true => "engine-unit-test",
    }
    .to_string()
}
