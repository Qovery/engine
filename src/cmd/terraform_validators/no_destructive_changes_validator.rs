use crate::cmd::terraform::TerraformOutput;
use crate::cmd::terraform_validators::{TerraformValidationError, TerraformValidator};
use itertools::Itertools;

const TERRAFORM_DESTRUCTIVE_ACTIONS_PATTERNS: [&str; 2] = ["will be destroyed", "must be replaced"];

pub struct NoDestructiveChangesValidator {
    protected_resources_names: Vec<String>,
}

impl NoDestructiveChangesValidator {
    pub fn new(protected_resources_names: &[&str]) -> Self {
        Self {
            protected_resources_names: protected_resources_names.iter().map(|r| r.to_string()).collect_vec(),
        }
    }
}

impl TerraformValidator for NoDestructiveChangesValidator {
    fn name(&self) -> String {
        "No destructive changes".to_string()
    }
    fn description(&self) -> String {
        "Prevent from resource destruction".to_string()
    }
    fn validate(&self, plan_output: &TerraformOutput) -> Result<(), TerraformValidationError> {
        for line in plan_output.raw_std_output.iter() {
            // FIXME: far from optimized code, but since both TERRAFORM_DESTRUCTIVE_ACTIONS_PATTERNS and protected_resources_names shouldn't be more than 10 elements
            // it can be optimized later on.
            for destructive_action_pattern in TERRAFORM_DESTRUCTIVE_ACTIONS_PATTERNS.iter() {
                if line.contains(destructive_action_pattern) {
                    for protected_resource in self.protected_resources_names.iter() {
                        if line.contains(protected_resource) {
                            return Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                                validator_name: self.name(),
                                validator_description: self.description(),
                                resource: protected_resource.to_string(),
                                raw_output: line.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TerraformTestCase<'a> {
        terraform_output: TerraformOutput,
        protected_resources: &'a [&'a str],
        expected: Result<(), TerraformValidationError>,
    }

    #[test]
    fn test_terraform_validator_no_cluster_destructive_changes() {
        // setup:
        let test_cases = vec![
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec!["# aws_eks_cluster.eks_cluster will be destroyed"], vec![]),
                protected_resources: &["aws_eks_cluster"],
                expected: Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "No destructive changes".to_string(),
                    validator_description: "Prevent from resource destruction".to_string(),
                    resource: "aws_eks_cluster".to_string(),
                    raw_output: "# aws_eks_cluster.eks_cluster will be destroyed".to_string(),
                }),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec!["# aws_eks_cluster.eks_cluster must be replaced"], vec![]),
                protected_resources: &["aws_eks_cluster"],
                expected: Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "No destructive changes".to_string(),
                    validator_description: "Prevent from resource destruction".to_string(),
                    resource: "aws_eks_cluster".to_string(),
                    raw_output: "# aws_eks_cluster.eks_cluster must be replaced".to_string(),
                }),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec!["# aws_eks_cluster.eks_cluster must be replaced"], vec![]),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec!["# google_container_cluster.primary will be destroyed"],
                    vec![],
                ),
                protected_resources: &["google_container_cluster"],
                expected: Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "No destructive changes".to_string(),
                    validator_description: "Prevent from resource destruction".to_string(),
                    resource: "google_container_cluster".to_string(),
                    raw_output: "# google_container_cluster.primary will be destroyed".to_string(),
                }),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec!["# google_container_cluster.primary must be replaced"],
                    vec![],
                ),
                protected_resources: &["google_container_cluster"],
                expected: Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "No destructive changes".to_string(),
                    validator_description: "Prevent from resource destruction".to_string(),
                    resource: "google_container_cluster".to_string(),
                    raw_output: "# google_container_cluster.primary must be replaced".to_string(),
                }),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec!["# google_container_cluster.primary must be replaced"],
                    vec![],
                ),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec!["# scaleway_k8s_cluster.kubernetes_cluster will be destroyed"],
                    vec![],
                ),
                protected_resources: &["scaleway_k8s_cluster"],
                expected: Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "No destructive changes".to_string(),
                    validator_description: "Prevent from resource destruction".to_string(),
                    resource: "scaleway_k8s_cluster".to_string(),
                    raw_output: "# scaleway_k8s_cluster.kubernetes_cluster will be destroyed".to_string(),
                }),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec!["# scaleway_k8s_cluster.kubernetes_cluster must be replaced"],
                    vec![],
                ),
                protected_resources: &["scaleway_k8s_cluster"],
                expected: Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "No destructive changes".to_string(),
                    validator_description: "Prevent from resource destruction".to_string(),
                    resource: "scaleway_k8s_cluster".to_string(),
                    raw_output: "# scaleway_k8s_cluster.kubernetes_cluster must be replaced".to_string(),
                }),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec!["# scaleway_k8s_cluster.kubernetes_cluster must be replaced"],
                    vec![],
                ),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(
                    vec![
                        "# aws_eks_cluster.eks_cluster must be updated",
                        "# another.resource will be destroyed",
                    ],
                    vec![],
                ),
                protected_resources: &["aws_eks_cluster"],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec!["# anything will be destroyed"], vec![]),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec!["# anything must be replaced"], vec![]),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec!["nothing really"], vec!["an error here"]),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec![""], vec![""]),
                protected_resources: &[],
                expected: Ok(()),
            },
            TerraformTestCase {
                terraform_output: TerraformOutput::new(vec![], vec![]),
                protected_resources: &[],
                expected: Ok(()),
            },
        ];

        for tc in test_cases.iter() {
            let validator = NoDestructiveChangesValidator::new(tc.protected_resources);

            // execute:
            let result = validator.validate(&tc.terraform_output);

            // verify:
            assert_eq!(result, tc.expected);
        }
    }
}
