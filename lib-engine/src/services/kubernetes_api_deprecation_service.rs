use crate::environment::models::types::VersionsNumber;
use crate::io_models::QoveryIdentifierError;
use crate::runtime::block_on;
use crate::{
    cmd::kubent::{Deprecation as CmdDeprecation, Kubent, KubentError},
    io_models::QoveryIdentifier,
};
use kube::api::{ApiResource, DynamicObject};
use kube::core::{GroupVersion, GroupVersionKind};
use kube::{Api, Client, Resource};
use std::str::FromStr;
use std::{
    fmt::{self, Formatter},
    ops::Deref,
    path::Path,
};

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum KubernetesDeprecationServiceError {
    #[error("Client (kubent) error: {client_error}")]
    ClientError { client_error: KubentError },
    #[error(
        "Error while trying to parse kubernetes API version, it seems to be an invalid version: `{invalid_version}`"
    )]
    ApiVersionNumberParsingError { invalid_version: String },
    #[error(
        "Error while trying to parse Qovery identifier, it seems to be an invalid identifier string: `{qovery_identifier_error}`"
    )]
    QoveryIdentifierParsingError {
        qovery_identifier_error: QoveryIdentifierError,
    },
    #[error("Some calls to deprecated APIs have been found: \n{deprecations}")]
    CallsToDeprecatedAPIsFound { deprecations: Deprecations },
}

#[derive(Clone, Debug, PartialEq)]
pub struct QoveryMetadata {
    pub qovery_service_id: Option<QoveryIdentifier>,
    pub qovery_environment_id: Option<QoveryIdentifier>,
    pub qovery_project_id: Option<QoveryIdentifier>,
    pub qovery_service_type: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Deprecation {
    pub name: Option<String>,
    pub namespace: Option<String>,
    pub kind: Option<String>,
    pub api_version: Option<String>,
    pub rule_set: Option<String>,
    pub replace_with: Option<String>,
    pub since: Option<VersionsNumber>,
    pub qovery_metadata: Option<QoveryMetadata>,
}

impl Deprecation {
    pub fn new(
        name: Option<String>,
        namespace: Option<String>,
        kind: Option<String>,
        api_version: Option<String>,
        rule_set: Option<String>,
        replace_with: Option<String>,
        since: Option<VersionsNumber>,
        qovery_metadata: Option<QoveryMetadata>,
    ) -> Self {
        Self {
            name,
            namespace,
            kind,
            api_version,
            rule_set,
            replace_with,
            since,
            qovery_metadata,
        }
    }

    pub fn new_with_qovery_metadata(
        kube_client: &Client,
        name: Option<String>,
        namespace: Option<String>,
        kind: Option<String>,
        api_version: Option<String>,
        rule_set: Option<String>,
        replace_with: Option<String>,
        since: Option<VersionsNumber>,
    ) -> Result<Self, KubernetesDeprecationServiceError> {
        let qovery_metadata = match (&name, &kind) {
            (Some(name), Some(kind)) => {
                let namespace = namespace.clone().unwrap_or_default();
                let api_version = api_version.clone().unwrap_or_default();

                // Parse GVK and create API
                let gvk = match GroupVersion::from_str(&api_version) {
                    Ok(gv) => GroupVersionKind::gvk(&gv.group, &gv.version, kind),
                    Err(e) => {
                        warn!("Error while trying to parse GroupVersion from api_version: {}", e);
                        GroupVersionKind::gvk("", "v1", kind)
                    }
                };

                let api: Api<DynamicObject> = if kind == "Namespace" || namespace.is_empty() {
                    Api::all_with(kube_client.clone(), &ApiResource::from_gvk(&gvk))
                } else {
                    Api::namespaced_with(kube_client.clone(), &namespace, &ApiResource::from_gvk(&gvk))
                };

                match block_on(api.get(name)) {
                    Ok(resource) => resource
                        .meta()
                        .labels
                        .as_ref()
                        .map(|labels| {
                            let mut metadata = QoveryMetadata {
                                qovery_service_id: None,
                                qovery_environment_id: None,
                                qovery_project_id: None,
                                qovery_service_type: None,
                            };

                            if let Some(value) = labels.get("qovery.com/service-id") {
                                metadata.qovery_service_id = Some(QoveryIdentifier::from_str(value).map_err(|e| {
                                    KubernetesDeprecationServiceError::QoveryIdentifierParsingError {
                                        qovery_identifier_error: e,
                                    }
                                })?);
                            }

                            if let Some(value) = labels.get("qovery.com/environment-id") {
                                metadata.qovery_environment_id =
                                    Some(QoveryIdentifier::from_str(value).map_err(|e| {
                                        KubernetesDeprecationServiceError::QoveryIdentifierParsingError {
                                            qovery_identifier_error: e,
                                        }
                                    })?);
                            }

                            if let Some(value) = labels.get("qovery.com/project-id") {
                                metadata.qovery_project_id = Some(QoveryIdentifier::from_str(value).map_err(|e| {
                                    KubernetesDeprecationServiceError::QoveryIdentifierParsingError {
                                        qovery_identifier_error: e,
                                    }
                                })?);
                            }

                            if let Some(value) = labels.get("qovery.com/service-type") {
                                metadata.qovery_service_type = Some(value.clone());
                            }

                            Ok(metadata)
                        })
                        .transpose()?,
                    Err(_) => None,
                }
            }
            _ => None,
        };

        Ok(Self {
            name,
            namespace,
            kind,
            api_version,
            rule_set,
            replace_with,
            since,
            qovery_metadata,
        })
    }

    pub fn try_from_with_qovery_metadata(
        kube_client: &Client,
        deprecation: CmdDeprecation,
    ) -> Result<Self, KubernetesDeprecationServiceError> {
        Self::new_with_qovery_metadata(
            kube_client,
            deprecation.name,
            deprecation.namespace,
            deprecation.kind,
            deprecation.api_version,
            deprecation.rule_set,
            deprecation.replace_with,
            match deprecation.since {
                Some(since) => Some(VersionsNumber::from_str(since.as_str()).map_err(|_e| {
                    KubernetesDeprecationServiceError::ApiVersionNumberParsingError {
                        invalid_version: since.to_string(),
                    }
                })?),
                None => None,
            },
        )
    }
}

impl TryFrom<CmdDeprecation> for Deprecation {
    type Error = KubernetesDeprecationServiceError;

    fn try_from(deprecation: CmdDeprecation) -> Result<Self, Self::Error> {
        Ok(Self {
            name: deprecation.name,
            namespace: deprecation.namespace,
            kind: deprecation.kind,
            api_version: deprecation.api_version,
            rule_set: deprecation.rule_set,
            replace_with: deprecation.replace_with,
            since: match deprecation.since {
                Some(api_version) => Some(VersionsNumber::from_str(api_version.as_str()).map_err(|_e| {
                    KubernetesDeprecationServiceError::ApiVersionNumberParsingError {
                        invalid_version: api_version.to_string(),
                    }
                })?),
                None => None,
            },
            qovery_metadata: None,
        })
    }
}

impl fmt::Display for Deprecation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // This function is not the prettiest one, it handles the display of the deprecation including padding and so
        let mut fields = Vec::new();
        let max_field_length = "Qovery Project ID".len();
        let padding_value = "";

        if let Some(qovery_metadata) = &self.qovery_metadata {
            if let Some(qovery_service_id) = &qovery_metadata.qovery_service_id {
                let field_name = "Qovery ID";
                fields.push(format!(
                    "║\t • {field_name}: {padding_value:<padding$}{qovery_service_id}",
                    padding = max_field_length - field_name.len()
                ));
            }
            if let Some(qovery_environment_id) = &qovery_metadata.qovery_environment_id {
                let field_name = "Qovery Env. ID";
                fields.push(format!(
                    "║\t • {field_name}: {padding_value:<padding$}{qovery_environment_id}",
                    padding = max_field_length - field_name.len()
                ));
            }
            if let Some(qovery_project_id) = &qovery_metadata.qovery_project_id {
                let field_name = "Qovery Project ID";
                fields.push(format!(
                    "║\t • {field_name}: {padding_value:<padding$}{qovery_project_id}",
                    padding = max_field_length - field_name.len()
                ));
            }
            if let Some(qovery_service_type) = &qovery_metadata.qovery_service_type {
                let field_name = "Qovery Type";
                fields.push(format!(
                    "║\t • {field_name}: {padding_value:<padding$}{qovery_service_type}",
                    padding = max_field_length - field_name.len()
                ));
            }
        }

        if let Some(name) = &self.name {
            let field_name = "Name";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{name}",
                padding = max_field_length - field_name.len()
            ));
        }
        if let Some(namespace) = &self.namespace {
            let field_name = "Namespace";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{namespace}",
                padding = max_field_length - field_name.len()
            ));
        }
        if let Some(kind) = &self.kind {
            let field_name = "Kind";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{kind}",
                padding = max_field_length - field_name.len()
            ));
        }
        if let Some(api_version) = &self.api_version {
            let field_name = "Current";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{api_version}",
                padding = max_field_length - field_name.len()
            ));
        }
        if let Some(rule_set) = &self.rule_set {
            let field_name = "Rule set";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{rule_set}",
                padding = max_field_length - field_name.len()
            ));
        }
        if let Some(replace_with) = &self.replace_with {
            let field_name = "Replace with";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{replace_with}",
                padding = max_field_length - field_name.len()
            ));
        }
        if let Some(since) = &self.since {
            let field_name = "Since";
            fields.push(format!(
                "║\t • {field_name}: {padding_value:<padding$}{since}",
                padding = max_field_length - field_name.len()
            ));
        }

        write!(f, "{}", fields.join("\n"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Deprecations(Vec<Deprecation>);

impl Deref for Deprecations {
    type Target = Vec<Deprecation>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Deprecations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const HEADER_LINE: &str = "╔════════════════════════════════════════════════════════════════════════╗";
        const SECTION_SEPARATOR: &str = "╟════════════════════════════════════════════════════════════════════════╗";
        const RESOURCE_SEPARATOR: &str = "╟────────────────────────────────────────────";
        const FOOTER_LINE: &str = "╚════════════════════════════════════════════════════════════════════════╝";

        let mut output = Vec::new();

        fn format_section<'a>(
            output: &mut Vec<String>,
            deprecations: impl Iterator<Item = &'a Deprecation>,
            section_title: &str,
            empty_message: &str,
        ) {
            output.push(format!("║ {section_title}"));
            output.push(RESOURCE_SEPARATOR.to_string());

            let mut has_resources = false;
            for (idx, deprecation) in deprecations.enumerate() {
                if idx > 0 {
                    output.push(RESOURCE_SEPARATOR.to_string());
                }
                output.push(format!("║ {}. Resource", idx + 1));
                output.push(deprecation.to_string());
                has_resources = true;
            }

            if !has_resources {
                output.push(format!("║ {empty_message}"));
            }
        }

        output.push(HEADER_LINE.to_string());

        let user_deployed = self.0.iter().filter(|d| d.qovery_metadata.is_some());
        format_section(
            &mut output,
            user_deployed,
            "User deployed deprecated resources:",
            "No user deployed deprecated resources found.",
        );

        output.push(SECTION_SEPARATOR.to_string());

        let other_resources = self.0.iter().filter(|d| d.qovery_metadata.is_none());
        format_section(
            &mut output,
            other_resources,
            "Other deprecated resources:",
            "No other deployed deprecated resources found.",
        );

        output.push(FOOTER_LINE.to_string());

        write!(f, "{}", output.join("\n"))
    }
}

pub enum KubernetesApiDeprecationServiceGranuality<'a> {
    Default,
    WithQoveryMetadata { kube_client: &'a Client },
}

pub struct KubernetesApiDeprecationService {
    client: Kubent,
}

impl KubernetesApiDeprecationService {
    pub fn new(client: Kubent) -> Self {
        Self { client }
    }

    pub fn get_deprecated_kubernetes_apis(
        &self,
        kubeconfig: &Path,
        target_version: Option<&VersionsNumber>,
        envs: &[(&str, &str)],
        granularity: KubernetesApiDeprecationServiceGranuality,
    ) -> Result<Vec<Deprecation>, KubernetesDeprecationServiceError> {
        self.client
            .get_deprecations(kubeconfig, target_version.map(|v| v.to_string()), envs)
            .map_err(|e| KubernetesDeprecationServiceError::ClientError { client_error: e })?
            .into_iter()
            .map(|d| match granularity {
                KubernetesApiDeprecationServiceGranuality::Default => Deprecation::try_from(d),
                KubernetesApiDeprecationServiceGranuality::WithQoveryMetadata { kube_client } => {
                    Deprecation::try_from_with_qovery_metadata(kube_client, d)
                }
            })
            .collect()
    }

    pub fn is_cluster_fully_compatible_with_kubernetes_version(
        &self,
        kubeconfig: &Path,
        target_kubernetes_version: Option<&VersionsNumber>,
        envs: &[(&str, &str)],
        granularity: KubernetesApiDeprecationServiceGranuality,
    ) -> Result<(), KubernetesDeprecationServiceError> {
        let deprecations = self
            .get_deprecated_kubernetes_apis(kubeconfig, target_kubernetes_version, envs, granularity)?
            .into_iter()
            .filter(|deprecation| match target_kubernetes_version {
                Some(tv) => match deprecation.since {
                    Some(ref version) => version <= tv,
                    None => true, // if deprecation doesn't have any version, we consider it as deprecated
                },
                None => true,
            })
            .collect::<Vec<_>>();
        if !deprecations.is_empty() {
            return Err(KubernetesDeprecationServiceError::CallsToDeprecatedAPIsFound {
                deprecations: Deprecations(deprecations),
            });
        }
        Ok(())
    }
}

impl Default for KubernetesApiDeprecationService {
    fn default() -> Self {
        Self { client: Kubent::new() }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, path::PathBuf};
    use tempfile::tempdir;

    use super::*;
    use crate::environment::models::types::VersionsNumberBuilder;
    use crate::{cmd::kubent, services::kubernetes_api_deprecation_service::Deprecation as ServiceDeprecation};

    #[test]
    fn test_get_deprecated_kubernetes_apis_with_deprecations() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let deprecations = vec![kubent::Deprecation {
            name: Some("name".to_string()),
            namespace: Some("namespace".to_string()),
            kind: Some("kind".to_string()),
            api_version: Some("1.29".to_string()),
            rule_set: Some("rule_set".to_string()),
            replace_with: Some("replace_with".to_string()),
            since: Some("1.28".to_string()),
        }];
        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Ok(deprecations.clone()));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.get_deprecated_kubernetes_apis(
            &kubeconfig,
            None,
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert_eq!(
            deprecations
                .into_iter()
                .flat_map(ServiceDeprecation::try_from)
                .collect::<Vec<ServiceDeprecation>>(),
            result.expect("Should have deprecations")
        );
    }

    #[test]
    fn test_get_deprecated_kubernetes_apis_without_deprecations() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let deprecations = vec![];
        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Ok(deprecations.clone()));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.get_deprecated_kubernetes_apis(
            &kubeconfig,
            None,
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert_eq!(
            deprecations
                .into_iter()
                .flat_map(ServiceDeprecation::try_from)
                .collect::<Vec<ServiceDeprecation>>(),
            result.expect("Should have deprecations")
        );
    }

    #[test]
    fn test_get_deprecated_kubernetes_apis_with_wrong_kubeconfig() {
        // setup:
        let kubeconfig = PathBuf::from("/tmp/kubeconfig-this-one-doesnt-exist");

        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Err(KubentError::InvalidKubeConfig {
            kubeconfig_path: kubeconfig.display().to_string(),
        }));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.get_deprecated_kubernetes_apis(
            &kubeconfig,
            None,
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert_eq!(
            KubernetesDeprecationServiceError::ClientError {
                client_error: KubentError::InvalidKubeConfig {
                    kubeconfig_path: kubeconfig.display().to_string(),
                }
            },
            result.expect_err("Should have error")
        );
    }

    #[test]
    fn test_get_deprecated_kubernetes_apis_with_wrong_kubernetes_api_version() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let invalid_api_version = ""; // TODO(benjaminch): find a better invalid version,
        // VersionsNumber parsing is a bit too permissive and clunky and needs to be improved /
        // swapped with an external lib.
        let deprecations = vec![kubent::Deprecation {
            name: Some("name".to_string()),
            namespace: Some("namespace".to_string()),
            kind: Some("kind".to_string()),
            api_version: Some(invalid_api_version.to_string()),
            rule_set: Some("rule_set".to_string()),
            replace_with: Some("replace_with".to_string()),
            since: Some(invalid_api_version.to_string()),
        }];
        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Ok(deprecations.clone()));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.get_deprecated_kubernetes_apis(
            &kubeconfig,
            None,
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert_eq!(
            KubernetesDeprecationServiceError::ApiVersionNumberParsingError {
                invalid_version: invalid_api_version.to_string()
            },
            result.expect_err("Should have error")
        );
    }

    #[test]
    fn test_is_cluster_fully_compatible_with_kubernetes_version_no_deprecations() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let deprecations = vec![];
        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Ok(deprecations.clone()));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.is_cluster_fully_compatible_with_kubernetes_version(
            &kubeconfig,
            None,
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_cluster_fully_compatible_with_kubernetes_version_no_deprecation_with_target_version() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let deprecations = vec![kubent::Deprecation {
            name: Some("name".to_string()),
            namespace: Some("namespace".to_string()),
            kind: Some("kind".to_string()),
            api_version: Some("1.33".to_string()),
            rule_set: Some("rule_set".to_string()),
            replace_with: Some("replace_with".to_string()),
            since: Some("1.33".to_string()),
        }];
        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Ok(deprecations.clone()));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.is_cluster_fully_compatible_with_kubernetes_version(
            &kubeconfig,
            Some(&VersionsNumberBuilder::new().major(1).minor(32).build()),
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_cluster_fully_compatible_with_kubernetes_version_with_deprecations_on_target_version() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let deprecations = vec![kubent::Deprecation {
            name: Some("name".to_string()),
            namespace: Some("namespace".to_string()),
            kind: Some("kind".to_string()),
            api_version: Some("1.32".to_string()),
            rule_set: Some("rule_set".to_string()),
            replace_with: Some("replace_with".to_string()),
            since: Some("1.32".to_string()),
        }];
        let mut kubent_cmd_mock = Kubent::faux();
        faux::when!(kubent_cmd_mock.get_deprecations(_, _, _)).then_return(Ok(deprecations.clone()));

        let service = KubernetesApiDeprecationService::new(kubent_cmd_mock);

        // execute:
        let result = service.is_cluster_fully_compatible_with_kubernetes_version(
            &kubeconfig,
            Some(&VersionsNumberBuilder::new().major(1).minor(32).build()),
            &[],
            KubernetesApiDeprecationServiceGranuality::Default,
        );

        // verify:
        assert_eq!(
            KubernetesDeprecationServiceError::CallsToDeprecatedAPIsFound {
                deprecations: Deprecations(
                    deprecations
                        .into_iter()
                        .flat_map(ServiceDeprecation::try_from)
                        .collect::<Vec<ServiceDeprecation>>(),
                )
            },
            result.expect_err("Should have error")
        );
    }

    #[test]
    fn test_deprecation_from_cmd_deprecation() {
        // setup:
        struct TestCase {
            cmd_deprecation: kubent::Deprecation,
            expected: Result<Deprecation, KubernetesDeprecationServiceError>,
        }

        let test_cases = vec![
            TestCase {
                cmd_deprecation: kubent::Deprecation {
                    name: Some("name".to_string()),
                    namespace: Some("namespace".to_string()),
                    kind: Some("kind".to_string()),
                    api_version: Some("1.29".to_string()),
                    rule_set: Some("rule_set".to_string()),
                    replace_with: Some("replace_with".to_string()),
                    since: Some("".to_string()), // semver is a bit broken, needs to be improved,
                                                 // but empty string will fail for sure
                },
                expected: Err(KubernetesDeprecationServiceError::ApiVersionNumberParsingError {
                    invalid_version: "".to_string(),
                }),
            },
            TestCase {
                cmd_deprecation: kubent::Deprecation {
                    name: Some("name".to_string()),
                    namespace: Some("namespace".to_string()),
                    kind: Some("kind".to_string()),
                    api_version: Some("1.29".to_string()),
                    rule_set: Some("rule_set".to_string()),
                    replace_with: Some("replace_with".to_string()),
                    since: Some("1.28".to_string()),
                },
                expected: Ok(Deprecation {
                    name: Some("name".to_string()),
                    namespace: Some("namespace".to_string()),
                    kind: Some("kind".to_string()),
                    api_version: Some("1.29".to_string()),
                    rule_set: Some("rule_set".to_string()),
                    replace_with: Some("replace_with".to_string()),
                    since: Some(VersionsNumberBuilder::new().major(1).minor(28).build()),
                    qovery_metadata: None,
                }),
            },
            TestCase {
                cmd_deprecation: kubent::Deprecation {
                    name: None,
                    namespace: None,
                    kind: None,
                    api_version: None,
                    rule_set: None,
                    replace_with: None,
                    since: None,
                },
                expected: Ok(Deprecation {
                    name: None,
                    namespace: None,
                    kind: None,
                    api_version: None,
                    rule_set: None,
                    replace_with: None,
                    since: None,
                    qovery_metadata: None,
                }),
            },
        ];

        for test_case in test_cases {
            // execute:
            let result = Deprecation::try_from(test_case.cmd_deprecation);
            // verify:
            assert_eq!(test_case.expected, result);
        }
    }
}
