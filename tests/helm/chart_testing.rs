use crate::helm::{
    TestInfo, application_context, chart_path, container_context, container_database_context, job_context,
    kubeconfig_path, lib_dir, managed_database_context,
};
use kube::core::DynamicObject;
use qovery_engine::cmd::helm::Helm;
use qovery_engine::environment::action::deploy_helm::HelmDeployment;
use qovery_engine::helm::CommonChart;
use qovery_engine::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use std::collections::HashMap;
use std::fs;
use std::fs::{File, read_dir};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::router_context;

fn to_kube_kind(file_path: &str) -> DynamicObject {
    let file = File::open(file_path).unwrap_or_else(|_| panic!("Unable to open file {}", &file_path));
    let obj: DynamicObject =
        serde_yaml::from_reader(file).unwrap_or_else(|_| panic!("Unable to parse file {}", &file_path));
    obj
}

fn generate_template(chart_info: &ChartInfo, temp_dir: &str, service_type_folder: &str, chart_id: &Uuid) -> String {
    let template_dir = format!("{}/{}/{}/{}-rendered", temp_dir, service_type_folder, chart_id, chart_info.name);
    if !Path::new(&template_dir).exists() {
        let _ = fs::create_dir_all(template_dir.clone());
    }

    let helm = Helm::new(Some(kubeconfig_path()), &[]).unwrap_or_else(|_| panic!("Unable to generate Helm struct"));
    helm.template_validate(chart_info, &[], Some(template_dir.as_str()))
        .expect("Unable to generate Helm template");
    template_dir
}

fn get_kube_resources(
    chart_original_path: &str,
    chart_info: ChartInfo,
    render_custom_values_file: Option<PathBuf>,
    test_info: &TestInfo,
    chart_id: &Uuid,
) -> HashMap<String, DynamicObject> {
    let helm_deployment = HelmDeployment::new(
        test_info.event_details.clone(),
        test_info.context.clone(),
        chart_original_path.parse().unwrap(),
        render_custom_values_file,
        chart_info.clone(),
    );
    let _ = helm_deployment.prepare_helm_chart();

    let template_dir = generate_template(&chart_info, &test_info.temp_dir, &test_info.service_folder_type, chart_id);

    let templates_path = format!("{}/{}/templates", template_dir, &chart_info.name);
    let files =
        read_dir(&templates_path).unwrap_or_else(|e| panic!("Unable to read files in {} : {:?}", &templates_path, e));
    let mut kube_resources: HashMap<String, DynamicObject> = HashMap::new();
    for file in files {
        let file_path = file
            .as_ref()
            .unwrap_or_else(|_| panic!("Unable to get file {:?}", &file))
            .path();
        let file_path_str = file_path
            .to_str()
            .unwrap_or_else(|| panic!("Unable to get file path for {:?}", &file_path));
        if file_path_str.ends_with(".yaml") {
            let kube_kind = to_kube_kind(file_path_str);
            kube_resources.insert(
                file.as_ref()
                    .unwrap_or_else(|_| panic!("Unable to get file {:?}", &file))
                    .file_name()
                    .to_str()
                    .unwrap_or_else(|| panic!("Unable to get file name for {:?}", &file))
                    .to_string(),
                kube_kind,
            );
        }
    }

    kube_resources
}

#[cfg(feature = "test-local-kube")]
#[test]
#[ignore]
fn q_ingress_test() {
    let test_info = router_context();
    let chart_name = "q-ingress-tls";
    let uuid = test_info.service_id;
    let chart = CommonChart {
        chart_info: ChartInfo {
            name: chart_name.to_string(),
            path: chart_path(&test_info.temp_dir, &test_info.service_folder_type, &uuid, chart_name),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: HelmAction::Deploy,
            atomic: false,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: 0,
            dry_run: false,
            wait: false,
            values: vec![],
            values_string: vec![],
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: false,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
    };

    let resources = get_kube_resources(
        format!("{}/common/charts/{}", lib_dir(), chart_name).as_str(),
        chart.chart_info,
        None,
        &test_info,
        &uuid,
    );
    assert!(!resources.is_empty());
}

#[cfg(feature = "test-local-kube")]
#[test]
#[ignore]
fn q_container_test() {
    let test_info = container_context();
    let chart_name = "q-container";
    let uuid = test_info.service_id;
    let chart = CommonChart {
        chart_info: ChartInfo {
            name: chart_name.to_string(),
            path: chart_path(&test_info.temp_dir, &test_info.service_folder_type, &uuid, chart_name),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: HelmAction::Deploy,
            atomic: false,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: 0,
            dry_run: false,
            wait: false,
            values: vec![],
            values_string: vec![],
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: false,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
    };
    let resources = get_kube_resources(
        format!("{}/common/charts/{}", lib_dir(), chart_name).as_str(),
        chart.chart_info,
        None,
        &test_info,
        &uuid,
    );
    for resource in resources.values() {
        assert!(resource.metadata.annotations.is_some());
        let annotations = resource.clone().metadata.annotations.unwrap();
        assert_eq!(annotations.get("annotation_key"), Some(&"annotation_value".to_string()));
    }

    assert!(!resources.is_empty());
}

#[cfg(feature = "test-local-kube")]
#[test]
#[ignore]
fn q_application_test() {
    dotenv::dotenv().ok();
    let test_info = application_context();
    let chart_name = "q-container";
    let uuid = test_info.service_id;
    let chart = CommonChart {
        chart_info: ChartInfo {
            name: chart_name.to_string(),
            path: chart_path(&test_info.temp_dir, &test_info.service_folder_type, &uuid, chart_name),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: HelmAction::Deploy,
            atomic: false,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: 0,
            dry_run: false,
            wait: false,
            values: vec![],
            values_string: vec![],
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: false,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
    };
    let resources = get_kube_resources(
        format!("{}/common/charts/{}", lib_dir(), chart_name).as_str(),
        chart.chart_info,
        None,
        &test_info,
        &uuid,
    );
    for resource in resources.values() {
        assert!(resource.metadata.annotations.is_some());
        let annotations = resource.clone().metadata.annotations.unwrap();
        assert_eq!(annotations.get("annotation_key"), Some(&"annotation_value".to_string()));
    }

    assert!(!resources.is_empty());
}

#[cfg(feature = "test-local-kube")]
#[test]
#[ignore]
fn q_container_psql_test() {
    let test_info = container_database_context();
    let chart_name = "postgresql";
    let uuid = test_info.service_id;
    let chart = CommonChart {
        chart_info: ChartInfo {
            name: chart_name.to_string(),
            path: chart_path(&test_info.temp_dir, &test_info.service_folder_type, &uuid, chart_name),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: HelmAction::Deploy,
            atomic: false,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: 0,
            dry_run: false,
            wait: false,
            values: vec![],
            values_string: vec![],
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: false,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
    };
    let resources = get_kube_resources(
        format!("{}/common/services/{}", lib_dir(), chart_name).as_str(),
        chart.chart_info,
        None,
        &test_info,
        &uuid,
    );
    assert!(!resources.is_empty());
}

#[cfg(feature = "test-local-kube")]
#[test]
#[ignore]
fn q_managed_psql_test() {
    let test_info = managed_database_context();
    let chart_name = "external-name-svc";
    let uuid = test_info.service_id;
    let chart = CommonChart {
        chart_info: ChartInfo {
            name: chart_name.to_string(),
            path: chart_path(&test_info.temp_dir, &test_info.service_folder_type, &uuid, chart_name),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: HelmAction::Deploy,
            atomic: false,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: 0,
            dry_run: false,
            wait: false,
            values: vec![],
            values_string: vec![],
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: false,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
    };
    let resources = get_kube_resources(
        format!("{}/common/charts/{}", lib_dir(), chart_name).as_str(),
        chart.chart_info,
        None,
        &test_info,
        &uuid,
    );
    assert!(!resources.is_empty());
}

#[cfg(feature = "test-local-kube")]
#[test]
#[ignore]
fn q_job_test() {
    let test_info = job_context();
    let chart_name = "q-job";
    let uuid = test_info.service_id;
    let chart = CommonChart {
        chart_info: ChartInfo {
            name: chart_name.to_string(),
            path: chart_path(&test_info.temp_dir, &test_info.service_folder_type, &uuid, chart_name),
            namespace: HelmChartNamespaces::KubeSystem,
            custom_namespace: None,
            action: HelmAction::Deploy,
            atomic: false,
            force_upgrade: false,
            recreate_pods: false,
            reinstall_chart_if_installed_version_is_below_than: None,
            timeout_in_seconds: 0,
            dry_run: false,
            wait: false,
            values: vec![],
            values_string: vec![],
            values_files: vec![],
            yaml_files_content: vec![],
            parse_stderr_for_error: false,
            k8s_selector: None,
            backup_resources: None,
            crds_update: None,
            skip_if_already_installed: false,
            upgrade_retry: None,
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
    };
    let resources = get_kube_resources(
        format!("{}/common/charts/{}", lib_dir(), chart_name).as_str(),
        chart.chart_info,
        None,
        &test_info,
        &uuid,
    );
    for resource in resources.values() {
        assert!(resource.metadata.annotations.is_some());
        let annotations = resource.clone().metadata.annotations.unwrap();
        assert_eq!(annotations.get("annotation_key"), Some(&"annotation_value".to_string()));
    }
    assert!(!resources.is_empty());
}
