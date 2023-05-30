use crate::cloud_provider::kubernetes::Kind;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::secret_manager::vault::get_vault_mount_name;
use crate::secret_manager::vault::QVaultClient;
use serde::{Deserialize, Serialize};
use vaultrs::api::kv2::responses::SecretVersionMetadata;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum ClusterSecrets {
    Eks(ClusterSecretsAws),
    Ec2(ClusterSecretsAws),
    Scaleway(ClusterSecretsScaleway),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ClusterSecretsAws {
    #[serde(rename = "AWS_ACCESS_KEY_ID")]
    pub aws_access_key_id: String,
    #[serde(rename = "AWS_DEFAULT_REGION")]
    pub aws_default_region: String,
    #[serde(rename = "AWS_SECRET_ACCESS_KEY")]
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

impl ClusterSecretsAws {
    pub fn new(
        aws_access_key_id: String,
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
    ) -> ClusterSecretsAws {
        ClusterSecretsAws {
            aws_access_key_id,
            aws_secret_access_key,
            aws_default_region,
            kubeconfig_b64,
            k8s_cluster_endpoint,
            cloud_provider,
            cluster_name,
            cluster_id,
            grafana_login,
            grafana_password,
            organization_id,
            test_cluster,
            vault_mount_name: get_vault_mount_name(test_cluster),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ClusterSecretsScaleway {
    #[serde(rename = "SCW_ACCESS_KEY")]
    pub scw_access_key: String,
    #[serde(rename = "SCW_SECRET_KEY")]
    pub scw_secret_key: String,
    #[serde(rename = "SCW_DEFAULT_PROJECT_ID")]
    pub scw_default_project_id: String,
    #[serde(rename = "SCW_DEFAULT_REGION")]
    pub scw_default_region: String,
    #[serde(rename = "SCW_DEFAULT_ZONE")]
    pub scw_default_zone: String,
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

impl ClusterSecretsScaleway {
    pub fn new(
        scw_access_key: String,
        scw_secret_key: String,
        scw_default_project_id: String,
        scw_default_region: String,
        scw_default_zone: String,
        kubeconfig_b64: Option<String>,
        k8s_cluster_endpoint: Option<String>,
        cloud_provider: Kind,
        cluster_name: String,
        cluster_id: String,
        grafana_login: String,
        grafana_password: String,
        organization_id: String,
        test_cluster: bool,
    ) -> ClusterSecretsScaleway {
        ClusterSecretsScaleway {
            scw_access_key,
            scw_secret_key,
            scw_default_project_id,
            scw_default_region,
            scw_default_zone,
            kubeconfig_b64,
            k8s_cluster_endpoint,
            cloud_provider,
            cluster_name,
            cluster_id,
            grafana_login,
            grafana_password,
            organization_id,
            test_cluster,
            vault_mount_name: get_vault_mount_name(test_cluster),
        }
    }
}

impl ClusterSecrets {
    pub fn new(cluster_secrets: ClusterSecrets) -> ClusterSecrets {
        cluster_secrets
    }

    pub fn new_aws_eks(cluster_secrets: ClusterSecretsAws) -> ClusterSecrets {
        ClusterSecrets::Eks(cluster_secrets)
    }

    pub fn new_scaleway(cluster_secrets: ClusterSecretsScaleway) -> ClusterSecrets {
        ClusterSecrets::Scaleway(cluster_secrets)
    }

    pub fn get_vault_mount_name(&self) -> String {
        let is_test_cluster = match self {
            ClusterSecrets::Eks(aws) | ClusterSecrets::Ec2(aws) => aws.test_cluster,
            ClusterSecrets::Scaleway(scaleway) => scaleway.test_cluster,
        };
        get_vault_mount_name(is_test_cluster)
    }

    pub fn get_secret(
        qvault_client: &QVaultClient,
        cloud_provider: Kind,
        cluster_id: &str,
        is_test_cluster: bool,
        event_details: EventDetails,
    ) -> Result<ClusterSecrets, Box<EngineError>> {
        let mount = get_vault_mount_name(is_test_cluster);
        let err = |e| {
            Box::new(EngineError::new_vault_secret_could_not_be_retrieved(
                event_details,
                CommandError::new(
                    format!("Vault secret couldn't be retrieved ({cloud_provider}/{cluster_id})"),
                    Some(format!("{e}")),
                    None,
                ),
            ))
        };

        match cloud_provider {
            Kind::Eks => match qvault_client.get_secret(mount.as_str(), cluster_id) {
                Ok(x) => Ok(ClusterSecrets::Eks(x)),
                Err(e) => Err(err(e)),
            },
            Kind::Ec2 => match qvault_client.get_secret(mount.as_str(), cluster_id) {
                Ok(x) => Ok(ClusterSecrets::Ec2(x)),
                Err(e) => Err(err(e)),
            },
            Kind::ScwKapsule => match qvault_client.get_secret(mount.as_str(), cluster_id) {
                Ok(x) => Ok(ClusterSecrets::Scaleway(x)),
                Err(e) => Err(err(e)),
            },
        }
    }

    pub fn get_cloud_provider(&self) -> Kind {
        match self {
            ClusterSecrets::Eks(_) => Kind::Eks,
            ClusterSecrets::Ec2(_) => Kind::Ec2,
            ClusterSecrets::Scaleway(_) => Kind::ScwKapsule,
        }
    }

    pub fn get_cluster_id(&self) -> &str {
        match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => x.cluster_id.as_str(),
            ClusterSecrets::Scaleway(x) => x.cluster_id.as_str(),
        }
    }

    pub fn get_test_cluster(&self) -> bool {
        match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => x.test_cluster,
            ClusterSecrets::Scaleway(x) => x.test_cluster,
        }
    }

    pub fn set_k8s_cluster_endpoint(&mut self, k8s_cluster_endpoint: String) {
        match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => x.k8s_cluster_endpoint = Some(k8s_cluster_endpoint),
            ClusterSecrets::Scaleway(x) => x.k8s_cluster_endpoint = Some(k8s_cluster_endpoint),
        }
    }

    pub fn set_kubeconfig_b64(&mut self, kubeconfig_b64: String) {
        match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => x.kubeconfig_b64 = Some(kubeconfig_b64),
            ClusterSecrets::Scaleway(x) => x.kubeconfig_b64 = Some(kubeconfig_b64),
        }
    }

    pub fn set_organization_id(&mut self, organization_id: String) {
        match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => x.organization_id = organization_id,
            ClusterSecrets::Scaleway(x) => x.organization_id = organization_id,
        }
    }

    /// Create or update a secret in vault
    /// If the secret already exists and has the same content, no update will be made
    /// ignore_kubeconfig_compare is used to avoid to compare kubeconfig_b64. Useful for EC2 when k3s is not ready yet but EC2 instance is
    pub fn create_or_update_secret(
        &self,
        qvault_client: &QVaultClient,
        ignore_kubeconfig_compare: bool,
        event_details: EventDetails,
    ) -> Result<Option<SecretVersionMetadata>, Box<EngineError>> {
        // check if secret already exists and has the same content to avoid to create a new version
        // then update if needed
        match Self::get_secret(
            qvault_client,
            self.get_cloud_provider(),
            self.get_cluster_id(),
            self.get_test_cluster(),
            event_details.clone(),
        ) {
            Ok(mut x) if ignore_kubeconfig_compare => {
                // blank kubeconfig_b64 to avoid to compare it
                match x {
                    ClusterSecrets::Eks(ref mut x) | ClusterSecrets::Ec2(ref mut x) => x.kubeconfig_b64 = None,
                    ClusterSecrets::Scaleway(ref mut x) => x.kubeconfig_b64 = None,
                }
                let mut current_secret = x.clone();
                match current_secret {
                    ClusterSecrets::Eks(ref mut x) | ClusterSecrets::Ec2(ref mut x) => x.kubeconfig_b64 = None,
                    ClusterSecrets::Scaleway(ref mut x) => x.kubeconfig_b64 = None,
                }
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

        // create a new secret
        let (vault_mount_name, secret_name) = match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => (x.vault_mount_name.as_str(), x.cluster_id.as_str()),
            ClusterSecrets::Scaleway(x) => (x.vault_mount_name.as_str(), x.cluster_id.as_str()),
        };

        match qvault_client.crate_update_secret(vault_mount_name, secret_name, self) {
            Ok(x) => Ok(Some(x)),
            Err(e) => Err(Box::new(EngineError::new_vault_secret_could_not_be_created_or_updated(
                event_details,
                CommandError::new(
                    "Vault secret couldn't be created or updated".to_string(),
                    Some(format!("{e:?}")),
                    None,
                ),
            ))),
        }
    }

    // required for tests
    #[allow(dead_code)]
    pub fn delete_secret(
        &self,
        qvault_client: &QVaultClient,
        event_details: EventDetails,
    ) -> Result<(), Box<EngineError>> {
        let (vault_mount_name, secret_name) = match self {
            ClusterSecrets::Eks(x) | ClusterSecrets::Ec2(x) => (x.vault_mount_name.as_str(), x.cluster_id.as_str()),
            ClusterSecrets::Scaleway(x) => (x.vault_mount_name.as_str(), x.cluster_id.as_str()),
        };

        match qvault_client.delete_secret(vault_mount_name, secret_name) {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(EngineError::new_vault_secret_could_not_be_created_or_updated(
                event_details,
                CommandError::new(
                    "Vault secret couldn't be created or updated".to_string(),
                    Some(format!("{e:?}")),
                    None,
                ),
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::kubernetes::Kind;
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use crate::runtime::block_on;
    use crate::secret_manager::vault;
    use crate::secret_manager::vault::QVaultClient;
    use uuid::Uuid;
    use vaultrs::api::kv2::responses::ReadSecretMetadataResponse;
    use vaultrs::error::ClientError;
    use vaultrs::kv2;

    use super::{ClusterSecrets, ClusterSecretsAws};

    fn get_event_details() -> EventDetails {
        EventDetails::new(
            None,
            QoveryIdentifier::new(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap()),
            QoveryIdentifier::new(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap()),
            "123e4567-e89b-12d3-a456-426614174000".to_string(),
            Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::TaskManager(Uuid::new_v4(), "engine".to_string()),
        )
    }

    fn cluster_aws_secret(cluster_id: Uuid, org_id: String) -> ClusterSecrets {
        ClusterSecrets::Eks(ClusterSecretsAws::new(
            "AWSACCESSKEY".to_string(),
            "us-west-42".to_string(),
            "AWSSECRETKEY".to_string(),
            Some("".to_string()),
            Some("http://127.0.0.1:443".to_string()),
            Kind::Eks,
            "cluster_name".to_string(),
            cluster_id.to_string(),
            "admin".to_string(),
            "password".to_string(),
            org_id,
            true,
        ))
    }

    impl QVaultClient {
        pub fn get_secret_metadata(&self, secret_name_path: &str) -> Result<ReadSecretMetadataResponse, ClientError> {
            let mount = vault::get_vault_mount_name(true);
            block_on(kv2::read_metadata(&self.connection, mount.as_str(), secret_name_path))
        }
    }

    #[test]
    fn manage_secret() {
        // todo(pmavro): check both auth (token/app_role), not only the one set
        let event_details = get_event_details();
        let cluster_id = Uuid::new_v4();
        let org_id = "123e4567-e89b-12d3-a456-426614174000".to_string();
        let mut cluster_secret = cluster_aws_secret(cluster_id, org_id);
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
                Kind::Eks,
                cluster_id.to_string().as_str(),
                true, // keep it this way to avoid searching in the wrong path
                event_details.clone()
            ),
            Ok(cluster_secret.clone())
        );

        // update with new content (version should be 2)
        let org_uuid = Uuid::new_v4();
        cluster_secret.set_organization_id(org_uuid.to_string());
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
            ClusterSecrets::get_secret(
                &qvault_client,
                Kind::Eks,
                cluster_id.to_string().as_str(),
                true,
                event_details.clone()
            ),
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
                .get_secret_metadata(cluster_id.to_string().as_str())
                .unwrap()
                .current_version,
            2
        );

        // delete
        assert_eq!(cluster_secret.delete_secret(&qvault_client, event_details), Ok(()))
    }
}
