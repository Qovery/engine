use aws_sdk_ecr::Client;
use aws_sdk_ecr::error::{DisplayErrorContext, SdkError};
use aws_sdk_ecr::operation::create_pull_through_cache_rule::CreatePullThroughCacheRuleError;
use aws_sdk_ecr::operation::create_repository_creation_template::CreateRepositoryCreationTemplateError;
use aws_sdk_ecr::operation::describe_pull_through_cache_rules::DescribePullThroughCacheRulesError;
use aws_sdk_ecr::types::{ImageTagMutability, PullThroughCacheRule, RctAppliedFor, RepositoryCreationTemplate};
use aws_types::SdkConfig;
use serde_json::{Value, json};
use thiserror::Error;

pub(super) const PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX: &str = "qovery-ecr-public";

const PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_TEMPLATE_DESCRIPTION: &str =
    "Qovery public ECR pull through cache repositories";
const PUBLIC_ECR_PULL_THROUGH_CACHE_RETENTION_DAYS: u16 = 365;

const PUBLIC_ECR_PULL_THROUGH_CACHE_RULE: PullThroughCacheRuleSpec = PullThroughCacheRuleSpec {
    ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX,
    upstream_registry_url: "public.ecr.aws",
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PullThroughCacheRuleSpec {
    ecr_repository_prefix: &'static str,
    upstream_registry_url: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EnsurePullThroughCacheRuleOutcome {
    Created,
    AlreadyConfigured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EnsureRepositoryCreationTemplateOutcome {
    Created,
    Updated,
    AlreadyConfigured,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub(super) enum EcrPullThroughCacheError {
    #[error("cannot describe ECR pull through cache rule `{ecr_repository_prefix}`: {raw_error_message}")]
    CannotDescribe {
        ecr_repository_prefix: &'static str,
        raw_error_message: String,
    },
    #[error("cannot create ECR pull through cache rule `{ecr_repository_prefix}`: {raw_error_message}")]
    CannotCreate {
        ecr_repository_prefix: &'static str,
        raw_error_message: String,
    },
    #[error("cannot describe ECR repository creation template `{ecr_repository_prefix}`: {raw_error_message}")]
    CannotDescribeRepositoryCreationTemplate {
        ecr_repository_prefix: &'static str,
        raw_error_message: String,
    },
    #[error("cannot create ECR repository creation template `{ecr_repository_prefix}`: {raw_error_message}")]
    CannotCreateRepositoryCreationTemplate {
        ecr_repository_prefix: &'static str,
        raw_error_message: String,
    },
    #[error("cannot update ECR repository creation template `{ecr_repository_prefix}`: {raw_error_message}")]
    CannotUpdateRepositoryCreationTemplate {
        ecr_repository_prefix: &'static str,
        raw_error_message: String,
    },
    #[error(
        "ECR pull through cache prefix `{ecr_repository_prefix}` is already configured for upstream `{actual_upstream_registry_url}` instead of `{expected_upstream_registry_url}`"
    )]
    ConflictingRule {
        ecr_repository_prefix: &'static str,
        expected_upstream_registry_url: &'static str,
        actual_upstream_registry_url: String,
    },
}

pub(super) async fn ensure_public_ecr_repository_creation_template(
    sdk_config: &SdkConfig,
) -> Result<EnsureRepositoryCreationTemplateOutcome, EcrPullThroughCacheError> {
    let client = Client::new(sdk_config);

    match get_public_ecr_repository_creation_template(&client).await? {
        Some(template) if repository_creation_template_is_configured(&template) => {
            Ok(EnsureRepositoryCreationTemplateOutcome::AlreadyConfigured)
        }
        Some(_) => update_public_ecr_repository_creation_template(&client).await,
        None => create_public_ecr_repository_creation_template(&client).await,
    }
}

async fn get_public_ecr_repository_creation_template(
    client: &Client,
) -> Result<Option<RepositoryCreationTemplate>, EcrPullThroughCacheError> {
    let output = client
        .describe_repository_creation_templates()
        .prefixes(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)
        .send()
        .await
        .map_err(|error| EcrPullThroughCacheError::CannotDescribeRepositoryCreationTemplate {
            ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX,
            raw_error_message: DisplayErrorContext(&error).to_string(),
        })?;

    Ok(output
        .repository_creation_templates
        .unwrap_or_default()
        .into_iter()
        .find(|template| template.prefix() == Some(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)))
}

async fn create_public_ecr_repository_creation_template(
    client: &Client,
) -> Result<EnsureRepositoryCreationTemplateOutcome, EcrPullThroughCacheError> {
    match client
        .create_repository_creation_template()
        .prefix(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)
        .description(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_TEMPLATE_DESCRIPTION)
        .image_tag_mutability(ImageTagMutability::Mutable)
        .lifecycle_policy(public_ecr_pull_through_cache_lifecycle_policy().to_string())
        .applied_for(RctAppliedFor::PullThroughCache)
        .send()
        .await
    {
        Ok(_) => Ok(EnsureRepositoryCreationTemplateOutcome::Created),
        Err(error) if repository_creation_template_already_exists(&error) => {
            match get_public_ecr_repository_creation_template(client).await? {
                Some(template) if repository_creation_template_is_configured(&template) => {
                    Ok(EnsureRepositoryCreationTemplateOutcome::AlreadyConfigured)
                }
                Some(_) => update_public_ecr_repository_creation_template(client).await,
                None => Err(EcrPullThroughCacheError::CannotCreateRepositoryCreationTemplate {
                    ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX,
                    raw_error_message: DisplayErrorContext(&error).to_string(),
                }),
            }
        }
        Err(error) => Err(EcrPullThroughCacheError::CannotCreateRepositoryCreationTemplate {
            ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX,
            raw_error_message: DisplayErrorContext(&error).to_string(),
        }),
    }
}

async fn update_public_ecr_repository_creation_template(
    client: &Client,
) -> Result<EnsureRepositoryCreationTemplateOutcome, EcrPullThroughCacheError> {
    client
        .update_repository_creation_template()
        .prefix(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)
        .description(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_TEMPLATE_DESCRIPTION)
        .image_tag_mutability(ImageTagMutability::Mutable)
        .lifecycle_policy(public_ecr_pull_through_cache_lifecycle_policy().to_string())
        .applied_for(RctAppliedFor::PullThroughCache)
        .send()
        .await
        .map(|_| EnsureRepositoryCreationTemplateOutcome::Updated)
        .map_err(|error| EcrPullThroughCacheError::CannotUpdateRepositoryCreationTemplate {
            ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX,
            raw_error_message: DisplayErrorContext(&error).to_string(),
        })
}

fn repository_creation_template_is_configured(template: &RepositoryCreationTemplate) -> bool {
    template.prefix() == Some(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)
        && template.image_tag_mutability() == Some(&ImageTagMutability::Mutable)
        && template.applied_for() == [RctAppliedFor::PullThroughCache]
        && template
            .lifecycle_policy()
            .and_then(|policy| serde_json::from_str::<Value>(policy).ok())
            .is_some_and(|policy| policy == public_ecr_pull_through_cache_lifecycle_policy())
}

fn public_ecr_pull_through_cache_lifecycle_policy() -> Value {
    json!({
        "rules": [
            {
                "rulePriority": 1,
                "description": "Expire pull-through cache images after 365 days",
                "selection": {
                    "tagStatus": "any",
                    "countType": "sinceImagePushed",
                    "countUnit": "days",
                    "countNumber": PUBLIC_ECR_PULL_THROUGH_CACHE_RETENTION_DAYS
                },
                "action": {
                    "type": "expire"
                }
            }
        ]
    })
}

fn repository_creation_template_already_exists(error: &SdkError<CreateRepositoryCreationTemplateError>) -> bool {
    error
        .as_service_error()
        .is_some_and(repository_creation_template_already_exists_service_error)
}

fn repository_creation_template_already_exists_service_error(error: &CreateRepositoryCreationTemplateError) -> bool {
    error.is_template_already_exists_exception()
}

pub(super) async fn ensure_public_ecr_pull_through_cache_rule(
    sdk_config: &SdkConfig,
) -> Result<EnsurePullThroughCacheRuleOutcome, EcrPullThroughCacheError> {
    let client = Client::new(sdk_config);

    if pull_through_cache_rule_is_configured(&client, PUBLIC_ECR_PULL_THROUGH_CACHE_RULE).await? {
        return Ok(EnsurePullThroughCacheRuleOutcome::AlreadyConfigured);
    }

    match client
        .create_pull_through_cache_rule()
        .ecr_repository_prefix(PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.ecr_repository_prefix)
        .upstream_registry_url(PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.upstream_registry_url)
        .send()
        .await
    {
        Ok(_) => Ok(EnsurePullThroughCacheRuleOutcome::Created),
        Err(error) if pull_through_cache_rule_already_exists(&error) => {
            if pull_through_cache_rule_is_configured(&client, PUBLIC_ECR_PULL_THROUGH_CACHE_RULE).await? {
                Ok(EnsurePullThroughCacheRuleOutcome::AlreadyConfigured)
            } else {
                Err(EcrPullThroughCacheError::CannotCreate {
                    ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.ecr_repository_prefix,
                    raw_error_message: DisplayErrorContext(&error).to_string(),
                })
            }
        }
        Err(error) => Err(EcrPullThroughCacheError::CannotCreate {
            ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.ecr_repository_prefix,
            raw_error_message: DisplayErrorContext(&error).to_string(),
        }),
    }
}

async fn pull_through_cache_rule_is_configured(
    client: &Client,
    expected_rule: PullThroughCacheRuleSpec,
) -> Result<bool, EcrPullThroughCacheError> {
    let output = match client
        .describe_pull_through_cache_rules()
        .ecr_repository_prefixes(expected_rule.ecr_repository_prefix)
        .send()
        .await
    {
        Ok(output) => output,
        Err(error) if pull_through_cache_rule_not_found(&error) => return Ok(false),
        Err(error) => {
            return Err(EcrPullThroughCacheError::CannotDescribe {
                ecr_repository_prefix: expected_rule.ecr_repository_prefix,
                raw_error_message: DisplayErrorContext(&error).to_string(),
            });
        }
    };

    validate_existing_rules(output.pull_through_cache_rules(), expected_rule)
}

fn validate_existing_rules(
    existing_rules: &[PullThroughCacheRule],
    expected_rule: PullThroughCacheRuleSpec,
) -> Result<bool, EcrPullThroughCacheError> {
    let Some(existing_rule) = existing_rules
        .iter()
        .find(|rule| rule.ecr_repository_prefix() == Some(expected_rule.ecr_repository_prefix))
    else {
        return Ok(false);
    };

    if existing_rule.upstream_registry_url() == Some(expected_rule.upstream_registry_url) {
        return Ok(true);
    }

    Err(EcrPullThroughCacheError::ConflictingRule {
        ecr_repository_prefix: expected_rule.ecr_repository_prefix,
        expected_upstream_registry_url: expected_rule.upstream_registry_url,
        actual_upstream_registry_url: existing_rule.upstream_registry_url().unwrap_or("undefined").to_string(),
    })
}

fn pull_through_cache_rule_already_exists(error: &SdkError<CreatePullThroughCacheRuleError>) -> bool {
    error
        .as_service_error()
        .is_some_and(CreatePullThroughCacheRuleError::is_pull_through_cache_rule_already_exists_exception)
}

fn pull_through_cache_rule_not_found(error: &SdkError<DescribePullThroughCacheRulesError>) -> bool {
    error
        .as_service_error()
        .is_some_and(pull_through_cache_rule_not_found_service_error)
}

fn pull_through_cache_rule_not_found_service_error(error: &DescribePullThroughCacheRulesError) -> bool {
    error.is_pull_through_cache_rule_not_found_exception()
}

#[cfg(test)]
mod tests {
    use super::{
        EcrPullThroughCacheError, PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX, PUBLIC_ECR_PULL_THROUGH_CACHE_RULE,
        PullThroughCacheRuleSpec, public_ecr_pull_through_cache_lifecycle_policy,
        pull_through_cache_rule_not_found_service_error, repository_creation_template_already_exists_service_error,
        repository_creation_template_is_configured, validate_existing_rules,
    };
    use aws_sdk_ecr::operation::create_repository_creation_template::CreateRepositoryCreationTemplateError;
    use aws_sdk_ecr::operation::describe_pull_through_cache_rules::DescribePullThroughCacheRulesError;
    use aws_sdk_ecr::types::error::{PullThroughCacheRuleNotFoundException, TemplateAlreadyExistsException};
    use aws_sdk_ecr::types::{ImageTagMutability, PullThroughCacheRule, RctAppliedFor, RepositoryCreationTemplate};

    #[test]
    fn missing_rule_requires_creation() {
        assert_eq!(validate_existing_rules(&[], PUBLIC_ECR_PULL_THROUGH_CACHE_RULE), Ok(false));
    }

    #[test]
    fn matching_rule_is_already_configured() {
        let existing_rule = pull_through_cache_rule(PUBLIC_ECR_PULL_THROUGH_CACHE_RULE);

        assert_eq!(
            validate_existing_rules(&[existing_rule], PUBLIC_ECR_PULL_THROUGH_CACHE_RULE),
            Ok(true)
        );
    }

    #[test]
    fn conflicting_rule_is_rejected() {
        let conflicting_rule = pull_through_cache_rule(PullThroughCacheRuleSpec {
            ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.ecr_repository_prefix,
            upstream_registry_url: "quay.io",
        });

        assert_eq!(
            validate_existing_rules(&[conflicting_rule], PUBLIC_ECR_PULL_THROUGH_CACHE_RULE),
            Err(EcrPullThroughCacheError::ConflictingRule {
                ecr_repository_prefix: PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.ecr_repository_prefix,
                expected_upstream_registry_url: PUBLIC_ECR_PULL_THROUGH_CACHE_RULE.upstream_registry_url,
                actual_upstream_registry_url: "quay.io".to_string(),
            })
        );
    }

    #[test]
    fn missing_rule_service_error_requires_creation() {
        let error = DescribePullThroughCacheRulesError::PullThroughCacheRuleNotFoundException(
            PullThroughCacheRuleNotFoundException::builder()
                .message("The pull through cache rule was not found")
                .build(),
        );

        assert!(pull_through_cache_rule_not_found_service_error(&error));
    }

    #[test]
    fn matching_repository_creation_template_is_already_configured() {
        let template = repository_creation_template(public_ecr_pull_through_cache_lifecycle_policy().to_string());

        assert!(repository_creation_template_is_configured(&template));
    }

    #[test]
    fn semantically_equivalent_lifecycle_policy_is_already_configured() {
        let lifecycle_policy = r#"
        {
          "rules": [{
            "selection": {
              "countNumber": 365,
              "countUnit": "days",
              "countType": "sinceImagePushed",
              "tagStatus": "any"
            },
            "description": "Expire pull-through cache images after 365 days",
            "action": { "type": "expire" },
            "rulePriority": 1
          }]
        }
        "#;
        let template = repository_creation_template(lifecycle_policy.to_string());

        assert!(repository_creation_template_is_configured(&template));
    }

    #[test]
    fn repository_creation_template_with_another_retention_is_updated() {
        let template = repository_creation_template(
            public_ecr_pull_through_cache_lifecycle_policy()
                .to_string()
                .replace("365", "30"),
        );

        assert!(!repository_creation_template_is_configured(&template));
    }

    #[test]
    fn immutable_repository_creation_template_is_updated() {
        let template = RepositoryCreationTemplate::builder()
            .prefix(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)
            .image_tag_mutability(ImageTagMutability::Immutable)
            .lifecycle_policy(public_ecr_pull_through_cache_lifecycle_policy().to_string())
            .applied_for(RctAppliedFor::PullThroughCache)
            .build();

        assert!(!repository_creation_template_is_configured(&template));
    }

    #[test]
    fn repository_creation_template_create_race_is_detected() {
        let error = CreateRepositoryCreationTemplateError::TemplateAlreadyExistsException(
            TemplateAlreadyExistsException::builder()
                .message("The repository creation template already exists")
                .build(),
        );

        assert!(repository_creation_template_already_exists_service_error(&error));
    }

    fn pull_through_cache_rule(spec: PullThroughCacheRuleSpec) -> PullThroughCacheRule {
        PullThroughCacheRule::builder()
            .ecr_repository_prefix(spec.ecr_repository_prefix)
            .upstream_registry_url(spec.upstream_registry_url)
            .build()
    }

    fn repository_creation_template(lifecycle_policy: String) -> RepositoryCreationTemplate {
        RepositoryCreationTemplate::builder()
            .prefix(PUBLIC_ECR_PULL_THROUGH_CACHE_REPOSITORY_PREFIX)
            .image_tag_mutability(ImageTagMutability::Mutable)
            .lifecycle_policy(lifecycle_policy)
            .applied_for(RctAppliedFor::PullThroughCache)
            .build()
    }
}
