use crate::cloud_provider::kubernetes::Kind;
use crate::errors::{CommandError, EngineError};
use crate::events::{EventDetails, Transmitter};
use crate::runtime::block_on;
use crate::secret_manager::io::ClusterSecretsIo;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::env;
use vaultrs::api::kv2::responses::SecretVersionMetadata;
use vaultrs::client::{VaultClient, VaultClientSettingsBuilder};
use vaultrs::error::ClientError;
use vaultrs::kv2;
use vaultrs_login::engines::approle::AppRoleLogin;
use vaultrs_login::LoginClient;

pub struct QVaultClient {
    connection: VaultClient,
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

    pub fn crate_update_secret(
        &self,
        mount: &str,
        secret_name: &str,
        secret_content: &ClusterSecrets,
    ) -> Result<SecretVersionMetadata, ClientError> {
        block_on(kv2::set(&self.connection, mount, secret_name, secret_content))
    }

    pub fn delete_secret(&self, mount: &str, secret_name: &str) -> Result<(), ClientError> {
        block_on(kv2::delete_metadata(&self.connection, mount, secret_name))
    }

    fn get_env_var(env_var: &str, event_details: EventDetails) -> Result<String, EngineError> {
        match env::var_os(&env_var) {
            Some(x) => Ok(x.into_string().unwrap_or_else(|_| {
                panic!(
                    "environment variable should have been found but not able to get it: {}",
                    &env_var
                )
            })),
            None => Err(EngineError::new_missing_required_env_variable(
                event_details,
                env_var.to_string(),
            )),
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

    pub fn new(event_details: EventDetails) -> Result<QVaultClient, EngineError> {
        let event_details = EventDetails::clone_changing_transmitter(
            event_details,
            Transmitter::SecretManager("QoveryVault".to_string()),
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
                                "error: wasn't able to contact Vault server with the given token. {:?}",
                                e
                            )),
                            None,
                        );
                        return Err(EngineError::new_vault_connection_error(event_details, cmd_error));
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
                                "error: wasn't able to contact Vault server with the given role and secret id. {:?}",
                                e
                            )),
                            None,
                        );
                        return Err(EngineError::new_vault_connection_error(event_details, cmd_error));
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
                return Err(EngineError::new_vault_connection_error(event_details, cmd_error));
            }
        };

        Ok(QVaultClient { connection })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClusterSecrets {
    pub aws_access_key: String,
    pub aws_default_region: String,
    pub aws_secret_access_key: String,
    pub kubeconfig_b64: Option<String>,
    pub k8s_cluster_endpoint: Option<String>,
    pub cloud_provider: Kind,
    pub cluster_name: String,
    pub cluster_id: String,
    pub grafana_login: String,
    pub grafana_password: String,
    pub organization_id: String,
    pub test_cluster: bool,
    pub vault_mount_name: String,
}

impl ClusterSecrets {
    pub fn new(
        aws_access_key: String,
        aws_default_region: String,
        aws_secret_access_key: String,
        kubeconfig_b64: Option<String>,
        k8s_cluster_endpoint: Option<String>,
        cloud_provider: Kind,
        cluster_name: String,
        cluster_id: String,
        grafana_login: String,
        grafana_password: String,
        organization_id: String,
        test_cluster: bool,
    ) -> Result<ClusterSecrets, EngineError> {
        let vault_mount_name = Self::get_vault_mount_name(test_cluster);

        Ok(ClusterSecrets {
            aws_access_key,
            aws_default_region,
            aws_secret_access_key,
            kubeconfig_b64,
            k8s_cluster_endpoint,
            cloud_provider,
            cluster_name,
            cluster_id,
            grafana_login,
            grafana_password,
            organization_id,
            test_cluster,
            vault_mount_name,
        })
    }

    pub fn new_from_cluster_secrets_io(
        cluster_secret: ClusterSecretsIo,
        event_details: EventDetails,
    ) -> Result<ClusterSecrets, EngineError> {
        let test_cluster =
            match cluster_secret.test_cluster.as_str() {
                "true" => true,
                "false" => false,
                _ => return Err(EngineError::new_error_when_create_cluster_secrets(
                    event_details,
                    CommandError::new(
                        "Qovery error when manipulating ClusterSecrets".to_string(),
                        Some(
                            "Qovery error when manipulating ClusterSecrets. Expected true or false for test cluster"
                                .to_string(),
                        ),
                        None,
                    ),
                )),
            };

        Self::new(
            cluster_secret.aws_access_key.clone(),
            cluster_secret.aws_default_region.clone(),
            cluster_secret.aws_secret_access_key.clone(),
            cluster_secret.kubeconfig_b64.clone(),
            cluster_secret.k8s_cluster_endpoint.clone(),
            cluster_secret.cloud_provider.clone(),
            cluster_secret.cluster_name.clone(),
            cluster_secret.cluster_id.clone(),
            cluster_secret.grafana_login.clone(),
            cluster_secret.grafana_password.clone(),
            cluster_secret.organization_id,
            test_cluster,
        )
    }

    pub fn get_vault_mount_name(is_test_cluster: bool) -> String {
        match is_test_cluster {
            false => "official-clusters-access",
            true => "engine-unit-tests",
        }
        .to_string()
    }

    pub fn get_secret(
        qvault_client: &QVaultClient,
        cluster_id: &str,
        is_test_cluster: bool,
        event_details: EventDetails,
    ) -> Result<ClusterSecrets, EngineError> {
        let mount = Self::get_vault_mount_name(is_test_cluster);

        match qvault_client.get_secret(mount.as_str(), cluster_id) {
            Ok(x) => Ok(x),
            Err(e) => Err(EngineError::new_vault_secret_could_not_be_retrieved(
                event_details,
                CommandError::new("Vault secret couldn't be retrieved".to_string(), Some(format!("{}", e)), None),
            )),
        }
    }

    pub fn create_or_update_secret(
        &self,
        qvault_client: &QVaultClient,
        ignore_kubeconfig_compare: bool,
        event_details: EventDetails,
    ) -> Result<Option<SecretVersionMetadata>, EngineError> {
        // check if secret already exists and has the same content to avoid to create a new version
        match Self::get_secret(
            qvault_client,
            self.cluster_id.as_str(),
            self.test_cluster,
            event_details.clone(),
        ) {
            Ok(mut x) if ignore_kubeconfig_compare => {
                x.kubeconfig_b64 = None;
                let mut current_secret = x.clone();
                current_secret.kubeconfig_b64 = None;
                if x == current_secret {
                    return Ok(None);
                }
            }
            Ok(x) => {
                if &x == self {
                    return Ok(None);
                }
            }
            Err(_) => {}
        };

        match qvault_client.crate_update_secret(self.vault_mount_name.as_str(), self.cluster_id.as_str(), self) {
            Ok(x) => Ok(Some(x)),
            Err(e) => Err(EngineError::new_vault_secret_could_not_be_created_or_updated(
                event_details,
                CommandError::new(
                    "Vault secret couldn't be created or updated".to_string(),
                    Some(format!("{:?}", e)),
                    None,
                ),
            )),
        }
    }

    // required for tests
    #[allow(dead_code)]
    pub fn delete_secret(&self, qvault_client: &QVaultClient, event_details: EventDetails) -> Result<(), EngineError> {
        match qvault_client.delete_secret(self.vault_mount_name.as_str(), self.cluster_id.as_str()) {
            Ok(_) => Ok(()),
            Err(e) => Err(EngineError::new_vault_secret_could_not_be_created_or_updated(
                event_details,
                CommandError::new(
                    "Vault secret couldn't be created or updated".to_string(),
                    Some(format!("{:?}", e)),
                    None,
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::kubernetes::Kind;
    use crate::errors::EngineError;
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use crate::runtime::block_on;
    use crate::secret_manager::vault::{ClusterSecrets, QVaultClient};
    use uuid::Uuid;
    use vaultrs::api::kv2::responses::ReadSecretMetadataResponse;
    use vaultrs::error::ClientError;
    use vaultrs::kv2;

    fn get_event_details() -> EventDetails {
        EventDetails::new(
            None,
            QoveryIdentifier::new("123e4567-e89b-12d3-a456-426614174000".to_string(), "z123e456".to_string()),
            QoveryIdentifier::new("123e4567-e89b-12d3-a456-426614174000".to_string(), "z123e456".to_string()),
            QoveryIdentifier::new("123e4567-e89b-12d3-a456-426614174000".to_string(), "z123e456".to_string()),
            None,
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::SecretManager("".to_string()),
        )
    }

    fn cluster_secret() -> Result<ClusterSecrets, EngineError> {
        ClusterSecrets::new(
            "AWSACCESSKEY".to_string(),
            "us-west-42".to_string(),
            "AWSSECRETKEY".to_string(),
            Some("".to_string()),
            Some("http://127.0.0.1:443".to_string()),
            Kind::Ec2,
            "cluster_name".to_string(),
            Uuid::new_v4().to_string(),
            "admin".to_string(),
            "password".to_string(),
            "123e4567-e89b-12d3-a456-426614174000".to_string(),
            true,
        )
    }

    impl QVaultClient {
        pub fn get_secret_metadata(&self, secret_name_path: &str) -> Result<ReadSecretMetadataResponse, ClientError> {
            let mount = ClusterSecrets::get_vault_mount_name(true);
            block_on(kv2::read_metadata(&self.connection, mount.as_str(), secret_name_path))
        }
    }

    #[test]
    fn manage_secret() {
        // todo(pmavro): check both auth (token/app_role), not only the one set
        let event_details = get_event_details();
        let mut cluster_secret = cluster_secret().unwrap();
        let qvault_client =
            QVaultClient::new(event_details.clone()).expect("should have a vault connexion but something is missing");

        // create (none already exists)
        assert_eq!(
            cluster_secret
                .create_or_update_secret(&qvault_client, false, event_details.clone())
                .unwrap()
                .unwrap()
                .version,
            1
        );

        // read created secret to ensure it's present
        assert_eq!(
            ClusterSecrets::get_secret(
                &qvault_client,
                cluster_secret.cluster_id.as_str(),
                true, // keep it this way to avoid searching in the wrong path
                event_details.clone()
            ),
            Ok(cluster_secret.clone())
        );

        // update with new content (version should be 2)
        let org_uuid = Uuid::new_v4();
        cluster_secret.organization_id = org_uuid.to_string();
        assert_eq!(
            cluster_secret
                .create_or_update_secret(&qvault_client, false, event_details.clone())
                .unwrap()
                .unwrap()
                .version,
            2
        );

        // read updated secret
        assert_eq!(
            ClusterSecrets::get_secret(&qvault_client, cluster_secret.cluster_id.as_str(), true, event_details.clone()),
            Ok(cluster_secret.clone())
        );

        // ask to update secret with the same content (no update should be made)
        assert!(cluster_secret
            .create_or_update_secret(&qvault_client, false, event_details.clone())
            .unwrap()
            .is_none());

        // ask to update secret with the same content and ignoring kubeconfig (no update should be made)
        assert!(cluster_secret
            .create_or_update_secret(&qvault_client, true, event_details.clone())
            .unwrap()
            .is_none());

        // ensure we're still on v2 and no update have been made
        assert_eq!(
            qvault_client
                .get_secret_metadata(cluster_secret.cluster_id.as_str())
                .unwrap()
                .current_version,
            2
        );

        // delete
        assert_eq!(cluster_secret.delete_secret(&qvault_client, event_details), Ok(()))
    }
}
