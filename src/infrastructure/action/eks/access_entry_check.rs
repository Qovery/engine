use crate::errors::{EngineError, Tag};
use crate::events::EventDetails;
use crate::infrastructure::action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::aws::AWS;
use crate::infrastructure::models::kubernetes::Kind;
use crate::runtime::block_on_with_timeout;
use rusoto_core::Region;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

/// IAM principal extracted from an STS caller identity ARN
#[derive(Debug, PartialEq, Eq)]
enum Principal {
    /// Assumed role session: `arn:aws:sts::<account>:assumed-role/<role_name>/<session>`.
    /// The role path is not part of this ARN, so roles are matched on account and name.
    Role {
        partition: String,
        account_id: String,
        role_name: String,
    },
    /// Any other principal (IAM user, root), matched on its exact ARN
    Other { arn: String },
}

impl Principal {
    fn from_caller_arn(caller_arn: &str) -> Option<Principal> {
        let (partition, service, account_id, resource) = split_arn(caller_arn)?;
        if service != "sts" {
            return Some(Principal::Other {
                arn: caller_arn.to_string(),
            });
        }

        let mut parts = resource.split('/');
        match (parts.next(), parts.next()) {
            (Some("assumed-role"), Some(role_name)) if !role_name.is_empty() => Some(Principal::Role {
                partition: partition.to_string(),
                account_id: account_id.to_string(),
                role_name: role_name.to_string(),
            }),
            _ => None,
        }
    }

    fn arn(&self) -> String {
        match self {
            Principal::Role {
                partition,
                account_id,
                role_name,
            } => format!("arn:{partition}:iam::{account_id}:role/{role_name}"),
            Principal::Other { arn } => arn.clone(),
        }
    }

    fn matches_access_entry(&self, access_entry_arn: &str) -> bool {
        match self {
            Principal::Role {
                partition,
                account_id,
                role_name,
            } => match split_arn(access_entry_arn) {
                // resource is `role/<optional path>/<role_name>`
                Some((entry_partition, "iam", entry_account_id, entry_resource)) => {
                    entry_partition == partition
                        && entry_account_id == account_id
                        && entry_resource.starts_with("role/")
                        && entry_resource.rsplit('/').next() == Some(role_name.as_str())
                }
                _ => false,
            },
            Principal::Other { arn } => arn == access_entry_arn,
        }
    }
}

/// Splits `arn:<partition>:<service>:<region>:<account>:<resource>` into (partition, service, account, resource)
fn split_arn(arn: &str) -> Option<(&str, &str, &str, &str)> {
    match arn.splitn(6, ':').collect::<Vec<_>>()[..] {
        ["arn", partition, service, _region, account_id, resource] => Some((partition, service, account_id, resource)),
        _ => None,
    }
}

/// Returns the principal ARN when it is confirmed that it has no access entry on the cluster.
/// Returns None when the principal has an access entry, or when it cannot be checked.
fn find_principal_without_access_entry(aws: &AWS, cluster_name: &str) -> Option<String> {
    let sts_client = StsClient::new_with_client(aws.client(), Region::default());
    let caller_arn = match block_on_with_timeout(sts_client.get_caller_identity(GetCallerIdentityRequest::default())) {
        Ok(Ok(identity)) => identity.arn?,
        Ok(Err(err)) => {
            warn!("Cannot check EKS access entries, unable to get caller identity: {err}");
            return None;
        }
        Err(err) => {
            warn!("Cannot check EKS access entries, unable to get caller identity: {err}");
            return None;
        }
    };
    let principal = Principal::from_caller_arn(&caller_arn)?;

    let access_entries = match block_on_with_timeout(aws.aws_sdk_client().list_access_entries(cluster_name)) {
        Ok(Ok(access_entries)) => access_entries,
        Ok(Err(err)) => {
            warn!("Cannot check EKS access entries of cluster {cluster_name}: {err}");
            return None;
        }
        Err(err) => {
            warn!("Cannot check EKS access entries of cluster {cluster_name}: {err}");
            return None;
        }
    };

    if access_entries.iter().any(|entry| principal.matches_access_entry(entry)) {
        None
    } else {
        Some(principal.arn())
    }
}

/// When the kube client cannot be created on an EKS cluster, checks whether the cloud provider
/// credentials have an access entry on it. This happens when the cluster credentials have been changed
/// to another IAM role or user: the cluster only grants access to the principal of its last deployment.
/// Returns the original error when it is not the case, or when it cannot be checked.
pub(crate) fn explain_kube_client_error(
    infra_ctx: &InfrastructureContext,
    event_details: &EventDetails,
    error: Box<EngineError>,
) -> Box<EngineError> {
    if error.tag() != &Tag::CannotConnectK8sCluster || infra_ctx.kubernetes().kind() != Kind::Eks {
        return error;
    }
    let cloud_provider = infra_ctx.cloud_provider().downcast_ref();
    let Some(aws) = cloud_provider.as_aws() else {
        return error;
    };

    let cluster_name = infra_ctx.kubernetes().cluster_name();
    match find_principal_without_access_entry(aws, &cluster_name) {
        Some(principal_arn) => Box::new(EngineError::new_eks_access_entry_missing(
            event_details.clone(),
            &principal_arn,
            &cluster_name,
        )),
        None => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_principal_from_caller_arn() {
        assert_eq!(
            Principal::from_caller_arn("arn:aws:sts::880317640327:assumed-role/mzo-update-cluster-role/session-name"),
            Some(Principal::Role {
                partition: "aws".to_string(),
                account_id: "880317640327".to_string(),
                role_name: "mzo-update-cluster-role".to_string(),
            })
        );
        assert_eq!(
            Principal::from_caller_arn("arn:aws:iam::880317640327:user/path/bob"),
            Some(Principal::Other {
                arn: "arn:aws:iam::880317640327:user/path/bob".to_string()
            })
        );
        assert_eq!(Principal::from_caller_arn("arn:aws:sts::880317640327:federated-user/bob"), None);
        assert_eq!(Principal::from_caller_arn("not-an-arn"), None);
    }

    #[test]
    fn test_role_arn() {
        let principal =
            Principal::from_caller_arn("arn:aws-cn:sts::880317640327:assumed-role/my-role/session").unwrap();
        assert_eq!(principal.arn(), "arn:aws-cn:iam::880317640327:role/my-role");
    }

    #[test]
    fn test_role_matches_access_entry() {
        let principal =
            Principal::from_caller_arn("arn:aws:sts::880317640327:assumed-role/mzo-update-cluster-role/session")
                .unwrap();

        assert!(principal.matches_access_entry("arn:aws:iam::880317640327:role/mzo-update-cluster-role"));
        assert!(principal.matches_access_entry("arn:aws:iam::880317640327:role/some/path/mzo-update-cluster-role"));
        assert!(!principal.matches_access_entry("arn:aws:iam::880317640327:role/mzo-create-cluster-role"));
        assert!(!principal.matches_access_entry("arn:aws:iam::880317640327:role/mzo-update-cluster-role-2"));
        assert!(!principal.matches_access_entry("arn:aws:iam::111111111111:role/mzo-update-cluster-role"));
        assert!(!principal.matches_access_entry("arn:aws:iam::880317640327:user/mzo-update-cluster-role"));
        assert!(!principal.matches_access_entry("arn:aws-cn:iam::880317640327:role/mzo-update-cluster-role"));
    }

    #[test]
    fn test_user_matches_access_entry() {
        let principal = Principal::from_caller_arn("arn:aws:iam::880317640327:user/bob").unwrap();

        assert!(principal.matches_access_entry("arn:aws:iam::880317640327:user/bob"));
        assert!(!principal.matches_access_entry("arn:aws:iam::880317640327:user/alice"));
        assert!(!principal.matches_access_entry("arn:aws:iam::880317640327:role/bob"));
    }
}
