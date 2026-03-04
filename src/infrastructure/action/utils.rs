use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::{InfraLogger, InfraLoggerImpl};
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::kubernetes::Kubernetes;
use serde::de::DeserializeOwned;

pub fn from_terraform_value<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::de::Deserializer<'de>,
    T: DeserializeOwned,
{
    use serde::Deserialize;

    #[derive(serde_derive::Deserialize)]
    struct TerraformJsonValue<T> {
        value: T,
    }

    TerraformJsonValue::deserialize(deserializer).map(|o: TerraformJsonValue<T>| o.value)
}

/// CIDR that allows unrestricted access (all IPs).
const UNRESTRICTED_CIDR: &str = "0.0.0.0/0";

/// Returns `true` when the CIDRs restrict API server access to specific IPs
/// (i.e. not the default unrestricted `0.0.0.0/0`).
pub fn is_api_access_restricted(public_access_cidrs: &[String]) -> bool {
    !(public_access_cidrs.len() == 1 && public_access_cidrs[0] == UNRESTRICTED_CIDR)
}

/// Generates the list of CIDRs allowed to access the Kubernetes API server.
///
/// When static IP mode is enabled and Qovery CIDRs are provided, merges them with any
/// user-specified CIDRs from advanced settings. Otherwise, returns `0.0.0.0/0` (unrestricted).
pub fn generate_public_access_cidrs(
    advanced_settings: &ClusterAdvancedSettings,
    qovery_allowed_public_access_cidrs: Option<&Vec<String>>,
) -> Vec<String> {
    match (
        advanced_settings.qovery_static_ip_mode.unwrap_or(false),
        qovery_allowed_public_access_cidrs,
    ) {
        (true, Some(qovery_allowed_public_access_cidrs)) if !qovery_allowed_public_access_cidrs.is_empty() => {
            match &advanced_settings.k8s_api_allowed_public_access_cidrs {
                Some(k8s_api_allowed_public_access_cidrs) => [
                    qovery_allowed_public_access_cidrs.clone(),
                    k8s_api_allowed_public_access_cidrs.clone(),
                ]
                .concat(),
                None => qovery_allowed_public_access_cidrs.clone(),
            }
        }
        _ => vec![UNRESTRICTED_CIDR.to_string()],
    }
}

pub fn mk_logger(kube: &dyn Kubernetes, step: InfrastructureStep) -> impl InfraLogger {
    let event_details = kube.get_event_details(Infrastructure(step));

    InfraLoggerImpl {
        event_details,
        logger: kube.logger().clone_dyn(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;

    #[test]
    fn test_public_access_cidrs_with_any_parameters_set() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: None,
            k8s_api_allowed_public_access_cidrs: None,
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = None;

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs);

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_disabled() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(false),
            k8s_api_allowed_public_access_cidrs: None,
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = None;

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs);

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_disabled_and_qovey_cidr() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(false),
            k8s_api_allowed_public_access_cidrs: None,
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_enabled_but_without_qovery_cidr() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec!["1.1.1.1/32".to_string()]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec![]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(cidrs, vec!["0.0.0.0/0".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_enabled() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec![]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(cidrs, vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);
    }

    #[test]
    fn test_public_access_cidrs_with_static_ip_mode_enabled_and_custom_cidr() {
        let advanced_settings = ClusterAdvancedSettings {
            qovery_static_ip_mode: Some(true),
            k8s_api_allowed_public_access_cidrs: Some(vec!["1.1.1.4/32".to_string()]),
            ..Default::default()
        };
        let qovery_allowed_public_access_cidrs = Some(vec!["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]);

        let cidrs = generate_public_access_cidrs(&advanced_settings, qovery_allowed_public_access_cidrs.as_ref());

        assert_eq!(
            cidrs,
            vec![
                "1.1.1.2/32".to_string(),
                "1.1.1.3/32".to_string(),
                "1.1.1.4/32".to_string()
            ]
        );
    }

    #[test]
    fn test_is_api_access_restricted_unrestricted() {
        assert!(!is_api_access_restricted(&[UNRESTRICTED_CIDR.to_string()]));
    }

    #[test]
    fn test_is_api_access_restricted_with_specific_cidrs() {
        assert!(is_api_access_restricted(&["1.1.1.2/32".to_string(), "1.1.1.3/32".to_string()]));
    }

    #[test]
    fn test_is_api_access_restricted_with_mixed_cidrs() {
        assert!(is_api_access_restricted(&[
            UNRESTRICTED_CIDR.to_string(),
            "1.1.1.2/32".to_string()
        ]));
    }

    #[test]
    pub fn test_terraform_value_parsing() {
        let json = r#"
{
  "aws_account_id": {
    "sensitive": false,
    "type": "string",
    "value": "843237546537"
  },
  "aws_iam_alb_controller_arn": {
    "sensitive": false,
    "type": "string",
    "value": "arn:aws:iam::843237546537:role/qovery-eks-alb-controller-z00000019"
  },
  "aws_iam_cloudwatch_role_arn": {
    "sensitive": false,
    "type": "string",
    "value": "arn:aws:iam::843237546537:role/qovery-cloudwatch-z00000019"
  },
  "aws_number": {
    "sensitive": false,
    "type": "number",
    "value": 12
  },
  "aws_float": {
    "sensitive": false,
    "type": "number",
    "value": 12.64
  },
  "aws_list": {
    "sensitive": false,
    "type": "list",
    "value": [
      "a",
      "b",
      "c"
    ]
  }
}
        "#;

        #[derive(serde_derive::Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "from_terraform_value")]
            aws_account_id: String,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_iam_alb_controller_arn: String,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_iam_cloudwatch_role_arn: String,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_number: u32,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_float: f32,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_list: Vec<String>,
        }

        let value: TestStruct = serde_json::from_str(json).unwrap();
        assert_eq!(value.aws_account_id, "843237546537");
        assert_eq!(
            value.aws_iam_alb_controller_arn,
            "arn:aws:iam::843237546537:role/qovery-eks-alb-controller-z00000019"
        );
        assert_eq!(
            value.aws_iam_cloudwatch_role_arn,
            "arn:aws:iam::843237546537:role/qovery-cloudwatch-z00000019"
        );
        assert_eq!(value.aws_number, 12);
        assert_eq!(value.aws_float, 12.64);
        assert!(!value.aws_list.is_empty());
    }
}
