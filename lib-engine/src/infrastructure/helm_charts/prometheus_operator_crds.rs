use super::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::runtime::block_on;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::ListParams;
use kube::{Api, Client, ResourceExt};
use std::collections::HashMap;

pub struct PrometheusOperatorCrdsChart {
    chart_path: HelmChartPath,
    prometheus_namespace: HelmChartNamespaces,
}

impl PrometheusOperatorCrdsChart {
    pub fn new(chart_prefix_path: Option<&str>, prometheus_namespace: HelmChartNamespaces) -> Self {
        Self {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PrometheusOperatorCrdsChart::chart_name(),
            ),
            prometheus_namespace,
        }
    }

    // list of supported CRDs by the chart. If the list change, we'll have to make sure the chart is updated and a migration may be needed
    pub fn expected_list_of_crds<'a>() -> Vec<&'a str> {
        vec![
            "alertmanagerconfigs.monitoring.coreos.com",
            "alertmanagers.monitoring.coreos.com",
            "podmonitors.monitoring.coreos.com",
            "probes.monitoring.coreos.com",
            "prometheusagents.monitoring.coreos.com",
            "prometheuses.monitoring.coreos.com",
            "prometheusrules.monitoring.coreos.com",
            "scrapeconfigs.monitoring.coreos.com",
            "servicemonitors.monitoring.coreos.com",
            "thanosrulers.monitoring.coreos.com",
        ]
    }

    pub fn expected_list_of_crd_annotations<'a, 'b>() -> HashMap<&'a str, &'b str> {
        HashMap::from([
            ("meta.helm.sh/release-name", "prometheus-operator-crds"),
            ("operator.prometheus.io/version", "0.79.2"),
            ("qovery.com/service-type", "crd"),
        ])
    }

    pub fn chart_name() -> String {
        "prometheus-operator-crds".to_string()
    }

    pub fn qovery_annotations() -> Vec<ChartSetValue> {
        vec![ChartSetValue {
            key: r"crds.annotations.qovery\.com/service-type".to_string(),
            value: "crd".to_string(),
        }]
    }
}

impl ToCommonHelmChart for PrometheusOperatorCrdsChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let chart_info: ChartInfo = ChartInfo {
            name: PrometheusOperatorCrdsChart::chart_name(),
            path: self.chart_path.to_string(),
            namespace: self.prometheus_namespace.clone(),
            values: PrometheusOperatorCrdsChart::qovery_annotations(),
            ..Default::default()
        };
        let x = CommonChart::new(chart_info, None, None);
        Ok(x)
    }
}

#[derive(Clone)]

pub struct PrometheusOperatorCrdsChartChecker {}

impl PrometheusOperatorCrdsChartChecker {
    pub fn new() -> PrometheusOperatorCrdsChartChecker {
        PrometheusOperatorCrdsChartChecker {}
    }
}

impl Default for PrometheusOperatorCrdsChartChecker {
    fn default() -> Self {
        PrometheusOperatorCrdsChartChecker::new()
    }
}

impl ChartInstallationChecker for PrometheusOperatorCrdsChartChecker {
    fn verify_installation(&self, kube_client: &Client) -> Result<(), CommandError> {
        let api_crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());
        match block_on(api_crds.list(&ListParams::default())) {
            Ok(crds_result) => {
                let installed_prometheus_operator_crds = crds_result
                    .items
                    .into_iter()
                    .filter(|crd| crd.spec.group == "monitoring.coreos.com")
                    .collect::<Vec<CustomResourceDefinition>>();
                let expected_prometheus_operator_crds = PrometheusOperatorCrdsChart::expected_list_of_crds();
                if installed_prometheus_operator_crds.len() != expected_prometheus_operator_crds.len() {
                    return Err(CommandError::new(
                        format!(
                            "There should be {} CRDs but only {} found",
                            expected_prometheus_operator_crds.len(),
                            installed_prometheus_operator_crds.len()
                        ),
                        None,
                        None,
                    ));
                }

                let prometheus_operator_crds_installed_by_name = installed_prometheus_operator_crds
                    .iter()
                    .map(|crd| (crd.name_any(), crd))
                    .collect::<HashMap<String, &CustomResourceDefinition>>();

                let expected_crd_annotations = PrometheusOperatorCrdsChart::expected_list_of_crd_annotations();

                for expected_crd in expected_prometheus_operator_crds {
                    let crd_installed = match prometheus_operator_crds_installed_by_name.get(expected_crd) {
                        None => return Err(CommandError::new(format!("Cannot find CRD '{expected_crd}'"), None, None)),
                        Some(crd_installed) => crd_installed,
                    };

                    let crd_installed_annotations = crd_installed.annotations();
                    for (expected_crd_annotation, expected_crd_annotation_value) in expected_crd_annotations.iter() {
                        match crd_installed_annotations.get(*expected_crd_annotation) {
                            None => {
                                return Err(CommandError::new(
                                    format!(
                                        "Cannot find annotation '{expected_crd_annotation}' for CRD '{expected_crd}'"
                                    ),
                                    None,
                                    None,
                                ));
                            }
                            Some(annotation_value) => {
                                if annotation_value != expected_crd_annotation_value {
                                    return Err(CommandError::new(
                                        format!(
                                            "Annotation '{expected_crd_annotation}' for CRD '{expected_crd}' is '{annotation_value}' instead of '{expected_crd_annotation_value}'"
                                        ),
                                        None,
                                        None,
                                    ));
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            Err(err) => Err(CommandError::new(
                "Cannot retrieve CRDs".to_string(),
                Some(err.to_string()),
                None,
            )),
        }
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, env, fs, path::Path, process::Command};

    use crate::{
        environment::models::kubernetes::K8sCrd,
        infrastructure::helm_charts::{HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name},
    };

    use super::*;

    // ensure all crds are supported by Qovery to avoid any unexpected issue
    // if we detect a new CRD or a missing one we previously supported, we'll have to update the chart and maybe do a migration
    #[test]
    fn test_prometheus_operator_supported_crds_list() {
        let mut resource_names = Vec::new();
        let chart = PrometheusOperatorCrdsChart::new(None, HelmChartNamespaces::Prometheus);
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let crds_path_string = format!(
            "{}/lib/{}/bootstrap/charts/{}",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            PrometheusOperatorCrdsChart::chart_name(),
        );
        let crds_path = Path::new(&crds_path_string);

        if !&crds_path.is_dir() {
            panic!("The path doesn't exist: {}", crds_path.to_string_lossy());
        }

        // render files to avoid remaining go template syntax, leading to serde yaml parsing errors
        let utests_dir = format!("{}/.qovery-workspace/utests", current_directory.display());
        let crds_rendered = format!(
            "{}/{}/charts/crds/templates",
            utests_dir,
            PrometheusOperatorCrdsChart::chart_name()
        );
        let _rm = fs::remove_dir_all(&utests_dir);
        let _mkdir =
            fs::create_dir(Path::new(&utests_dir)).map_err(|e| format!("Error creating folder {}: {}", &utests_dir, e));
        let output = match Command::new("helm")
            .args([
                "template".to_string(),
                PrometheusOperatorCrdsChart::chart_name(),
                crds_path.display().to_string(),
                "--hide-notes".to_string(),
                "--output-dir".to_string(),
                utests_dir.clone(),
            ])
            .output()
        {
            Ok(x) => format!("Helm rendering output: {x:?}"),
            Err(e) => panic!("Error while trying to render crds from prometheus operator: {e}"),
        };

        // list all crd rendered files
        for entry in fs::read_dir(&crds_rendered)
            .map_err(|e| format!("Error reading folder {}: {}\n{}", &crds_rendered, e, output))
            .unwrap()
        {
            let path = entry.unwrap().path();

            // only read yaml files
            if let Some(extension) = path.extension() {
                if extension == "yaml" {
                    // remove --- in the file because it's seen as multiple files and not supported by serde
                    let content = fs::read_to_string(&path).unwrap_or_else(|_| {
                        panic!(
                            "error while trying to read crd file {} for prometheus operator crd",
                            path.to_string_lossy()
                        )
                    });
                    let content = content.replace("---", "");

                    // parse crd file
                    let file = serde_yaml::from_str::<K8sCrd>(&content).unwrap_or_else(|_| {
                        panic!(
                            "error while trying to parse crd file {} for prometheus operator crd",
                            path.to_string_lossy()
                        )
                    });
                    resource_names.push(file.metadata.name);
                }
            }
        }

        let set1: HashSet<String> = PrometheusOperatorCrdsChart::expected_list_of_crds()
            .iter()
            .map(|it| it.to_string())
            .collect();
        let set2: HashSet<String> = resource_names.iter().cloned().collect();
        let diff: Vec<String> = set1.symmetric_difference(&set2).cloned().collect();

        // show the diff between our known CRDs base and the one in the current chart to avoid any unexpected issue
        assert!(
            diff.is_empty(),
            "The following CRDs are not supported by Qovery or not present anymore in the list of CRDs: {diff:?}"
        );
    }
}
