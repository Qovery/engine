use crate::environment::models::domain::ToTerraformString;
use serde_derive::{Deserialize, Serialize};
use std::fmt::Display;
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(EnumIter, Eq, PartialEq, Serialize, Deserialize, Debug, Clone)]
pub enum ScalewayPublicGatewayType {
    #[serde(alias = "VPC-GW-S")]
    Small,
    #[serde(alias = "VPC-GW-M")]
    Medium,
    #[serde(alias = "VPC-GW-L")]
    Large,
    #[serde(alias = "VPC-GW-XL")]
    XLarge,
}

impl FromStr for ScalewayPublicGatewayType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "vpc-gw-s" => Ok(ScalewayPublicGatewayType::Small),
            "vpc-gw-m" => Ok(ScalewayPublicGatewayType::Medium),
            "vpc-gw-l" => Ok(ScalewayPublicGatewayType::Large),
            "vpc-gw-xl" => Ok(ScalewayPublicGatewayType::XLarge),
            _ => Err(format!("Unknown PublicGatewayType: {s}")),
        }
    }
}

impl Display for ScalewayPublicGatewayType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            ScalewayPublicGatewayType::Small => "VPC-GW-S".to_string(),
            ScalewayPublicGatewayType::Medium => "VPC-GW-M".to_string(),
            ScalewayPublicGatewayType::Large => "VPC-GW-L".to_string(),
            ScalewayPublicGatewayType::XLarge => "VPC-GW-XL".to_string(),
        };
        write!(f, "{str}")
    }
}

impl ToTerraformString for ScalewayPublicGatewayType {
    fn to_terraform_format_string(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn test_public_gateway_type_from_str_valid_inputs() {
        for gateway_type in ScalewayPublicGatewayType::iter() {
            let result_lower_case =
                ScalewayPublicGatewayType::from_str(&gateway_type.to_string().to_lowercase()).unwrap();
            let result_upper_case =
                ScalewayPublicGatewayType::from_str(&gateway_type.to_string().to_uppercase()).unwrap();
            let result_empty_start_trailing_spaces =
                ScalewayPublicGatewayType::from_str(format!("  {}  ", &gateway_type.to_string()).as_str()).unwrap();
            assert!(
                gateway_type == result_lower_case
                    && gateway_type == result_upper_case
                    && gateway_type == result_empty_start_trailing_spaces
            );
        }
    }

    #[test]
    fn test_public_gateway_type_from_str_invalid_inputs() {
        let invalid_inputs = vec!["invalid", "vpc-gw-xxl", ""];

        for input in invalid_inputs {
            let result = ScalewayPublicGatewayType::from_str(input);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_public_gateway_type_to_string() {
        let cases = vec![
            (ScalewayPublicGatewayType::Small, "VPC-GW-S"),
            (ScalewayPublicGatewayType::Medium, "VPC-GW-M"),
            (ScalewayPublicGatewayType::Large, "VPC-GW-L"),
            (ScalewayPublicGatewayType::XLarge, "VPC-GW-XL"),
        ];

        for (input, expected) in cases {
            let result = input.to_string();
            assert_eq!(expected, result);
        }

        for gateway_type in ScalewayPublicGatewayType::iter() {
            let result = gateway_type.to_string();
            assert_eq!(
                match gateway_type {
                    ScalewayPublicGatewayType::Small => "VPC-GW-S",
                    ScalewayPublicGatewayType::Medium => "VPC-GW-M",
                    ScalewayPublicGatewayType::Large => "VPC-GW-L",
                    ScalewayPublicGatewayType::XLarge => "VPC-GW-XL",
                },
                result
            );
        }
    }

    #[test]
    fn test_public_gateway_type_to_terraform_string() {
        let cases = vec![
            (ScalewayPublicGatewayType::Small, "VPC-GW-S"),
            (ScalewayPublicGatewayType::Medium, "VPC-GW-M"),
            (ScalewayPublicGatewayType::Large, "VPC-GW-L"),
            (ScalewayPublicGatewayType::XLarge, "VPC-GW-XL"),
        ];

        for (input, expected) in cases {
            let result = input.to_string();
            assert_eq!(expected, result);
        }

        for gateway_type in ScalewayPublicGatewayType::iter() {
            let result = gateway_type.to_terraform_format_string();
            assert_eq!(
                match gateway_type {
                    ScalewayPublicGatewayType::Small => "VPC-GW-S",
                    ScalewayPublicGatewayType::Medium => "VPC-GW-M",
                    ScalewayPublicGatewayType::Large => "VPC-GW-L",
                    ScalewayPublicGatewayType::XLarge => "VPC-GW-XL",
                },
                result
            );
        }
    }
}
