use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::{InfraLogger, InfraLoggerImpl};
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
