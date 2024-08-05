use crate::cmd::terraform::TerraformOutput;
use crate::cmd::terraform_validators::no_destructive_changes_validator::NoDestructiveChangesValidator;
use thiserror::Error;

pub mod no_destructive_changes_validator;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum TerraformValidationError {
    #[error("Error, resource `{resource}` has forbidden destructive changes: {raw_output}")]
    HasForbiddenDestructiveChanges {
        validator_name: String,
        validator_description: String,
        resource: String,
        raw_output: String,
    },
}

pub enum TerraformValidators<'a> {
    None,
    Default,
    Custom(Vec<&'a dyn TerraformValidator>),
}

impl TerraformValidators<'_> {
    pub fn validate(&self, plan_output: &TerraformOutput) -> Result<(), TerraformValidationError> {
        match self {
            TerraformValidators::None => {}
            TerraformValidators::Default => {
                // for now there is only one validator per default, once several, activate the code below instead
                NoDestructiveChangesValidator::new(&[
                    "aws_eks_cluster",
                    "google_container_cluster",
                    "scaleway_k8s_cluster",
                ])
                .validate(plan_output)?;

                // -> activate this in case of several default validators
                // for validator in [NoClusterDestructiveChangesValidator::new()].iter() {
                //     validator.validate(plan_output)?;
                // }
            }
            TerraformValidators::Custom(validators) => {
                for validator in validators.iter() {
                    validator.validate(plan_output)?;
                }
            }
        }

        Ok(())
    }
}

pub trait TerraformValidator {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn validate(&self, plan_output: &TerraformOutput) -> Result<(), TerraformValidationError>;
}
