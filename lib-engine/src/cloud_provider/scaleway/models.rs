use crate::cloud_provider::models::IngressLoadBalancerType;
use crate::errors::CommandError;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(Clone, Debug, EnumIter, Eq, PartialEq)]
pub enum ScwLoadBalancerType {
    Small,
    GpMedium,
    GpLarge,
    GpXLarge,
}

impl Display for ScwLoadBalancerType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ScwLoadBalancerType::Small => "lb-s",
            ScwLoadBalancerType::GpMedium => "lb-gp-m",
            ScwLoadBalancerType::GpLarge => "lb-gp-l",
            ScwLoadBalancerType::GpXLarge => "lb-gp-xl",
        })
    }
}

impl FromStr for ScwLoadBalancerType {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "lb-s" => Ok(ScwLoadBalancerType::Small),
            "lb-gp-m" => Ok(ScwLoadBalancerType::GpMedium),
            "lb-gp-l" => Ok(ScwLoadBalancerType::GpLarge),
            "lb-gp-xl" => Ok(ScwLoadBalancerType::GpXLarge),
            _ => Err(CommandError::new_from_safe_message(format!(
                "`{}` load balancer type is not supported",
                s
            ))),
        }
    }
}

impl IngressLoadBalancerType for ScwLoadBalancerType {
    fn annotation_key(&self) -> String {
        "service.beta.kubernetes.io/scw-loadbalancer-type".to_string()
    }

    fn annotation_value(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::models::IngressLoadBalancerType;
    use crate::cloud_provider::scaleway::models::ScwLoadBalancerType;
    use crate::errors::CommandError;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_scw_load_balancer_type_to_string() {
        for lb_type in ScwLoadBalancerType::iter() {
            // execute:
            let res = lb_type.to_string();

            // verify:
            assert_eq!(
                match lb_type {
                    ScwLoadBalancerType::Small => "lb-s",
                    ScwLoadBalancerType::GpMedium => "lb-gp-m",
                    ScwLoadBalancerType::GpLarge => "lb-gp-l",
                    ScwLoadBalancerType::GpXLarge => "lb-gp-xl",
                },
                res
            );
        }
    }

    #[test]
    fn test_scw_load_balancer_type_from_str() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            expected: Result<ScwLoadBalancerType, CommandError>,
        }

        let test_cases = vec![
            TestCase {
                input: "wrong",
                expected: Err(CommandError::new_from_safe_message(
                    "`wrong` load balancer type is not supported".to_string(),
                )),
            },
            TestCase {
                input: "lb-s",
                expected: Ok(ScwLoadBalancerType::Small),
            },
            TestCase {
                input: "Lb-s",
                expected: Ok(ScwLoadBalancerType::Small),
            },
            TestCase {
                input: "LB-S",
                expected: Ok(ScwLoadBalancerType::Small),
            },
            TestCase {
                input: "lb-gp-m",
                expected: Ok(ScwLoadBalancerType::GpMedium),
            },
            TestCase {
                input: "Lb-gp-m",
                expected: Ok(ScwLoadBalancerType::GpMedium),
            },
            TestCase {
                input: "LB-GP-M",
                expected: Ok(ScwLoadBalancerType::GpMedium),
            },
            TestCase {
                input: "lb-gp-l",
                expected: Ok(ScwLoadBalancerType::GpLarge),
            },
            TestCase {
                input: "Lb-gp-l",
                expected: Ok(ScwLoadBalancerType::GpLarge),
            },
            TestCase {
                input: "LB-GP-L",
                expected: Ok(ScwLoadBalancerType::GpLarge),
            },
            TestCase {
                input: "lb-gp-xl",
                expected: Ok(ScwLoadBalancerType::GpXLarge),
            },
            TestCase {
                input: "Lb-gp-xl",
                expected: Ok(ScwLoadBalancerType::GpXLarge),
            },
            TestCase {
                input: "LB-GP-XL",
                expected: Ok(ScwLoadBalancerType::GpXLarge),
            },
        ];

        for tc in test_cases {
            // execute:
            let res = ScwLoadBalancerType::from_str(tc.input);

            // verify:
            assert_eq!(tc.expected, res,);
        }
    }

    #[test]
    fn test_scw_load_balancer_type_annotation_key() {
        for lb_type in ScwLoadBalancerType::iter() {
            // execute:
            let res = lb_type.annotation_key();

            // verify:
            assert_eq!("service.beta.kubernetes.io/scw-loadbalancer-type".to_string(), res);
        }
    }

    #[test]
    fn test_scw_load_balancer_type_annotation_value() {
        for lb_type in ScwLoadBalancerType::iter() {
            // execute:
            let res = lb_type.annotation_value();

            // verify:
            assert_eq!(lb_type.to_string(), res);
        }
    }
}
