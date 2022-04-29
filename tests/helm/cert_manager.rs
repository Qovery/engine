use base64::decode;
use qovery_engine::cloud_provider::helm::{
    deploy_charts_levels, ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces,
};
use qovery_engine::cmd::helm::Helm;
use qovery_engine::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_secrets, kubectl_get_resource_yaml};
use qovery_engine::cmd::structs::SecretItem;
use qovery_engine::fs::list_yaml_backup_files;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::from_utf8;
use std::thread::sleep;
use std::time::Duration;
use tempdir::TempDir;
use test_utilities::utilities::FuncTestsSecrets;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Certificate {
    pub api_version: String,
    pub items: Vec<Item>,
    pub kind: String,
    pub metadata: Metadata2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
    pub status: Status,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub annotations: Annotations,
    pub creation_timestamp: String,
    pub generation: i64,
    pub labels: Labels,
    pub name: String,
    pub namespace: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotations {
    #[serde(rename = "meta.helm.sh/release-name")]
    pub meta_helm_sh_release_name: String,
    #[serde(rename = "meta.helm.sh/release-namespace")]
    pub meta_helm_sh_release_namespace: String,
    #[serde(default, rename = "kubectl.kubernetes.io/last-applied-configuration")]
    pub last_applied_configuration: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    #[serde(rename = "app.kubernetes.io/managed-by")]
    pub app_kubernetes_io_managed_by: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub dns_names: Vec<String>,
    pub issuer_ref: IssuerRef,
    pub secret_name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuerRef {
    pub kind: String,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata2 {
    pub self_link: String,
}

fn cert_manager_conf() -> (Helm, PathBuf, CommonChart, CommonChart) {
    let vault_secrets = FuncTestsSecrets::new();
    let mut kube_config = dirs::home_dir().unwrap();
    kube_config.push(".kube/config");
    let helm = Helm::new(kube_config.to_str().unwrap(), &[]).unwrap();
    let cert_manager = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager".to_string(),
            path: "lib/common/bootstrap/charts/cert-manager".to_string(),
            namespace: HelmChartNamespaces::CertManager,
            values: vec![
                ChartSetValue {
                    key: "installCRDs".to_string(),
                    value: "true".to_string(),
                },
                ChartSetValue {
                    key: "replicaCount".to_string(),
                    value: "1".to_string(),
                },
                // https://cert-manager.io/docs/configuration/acme/dns01/#setting-nameservers-for-dns01-self-check
                ChartSetValue {
                    key: "extraArgs".to_string(),
                    value: "{--dns01-recursive-nameservers-only,--dns01-recursive-nameservers=1.1.1.1:53\\,8.8.8.8:53}"
                        .to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.enabled".to_string(),
                    // Due to cycle, prometheus need tls certificate from cert manager, and enabling this will require
                    // prometheus to be already installed
                    value: "false".to_string(),
                },
                ChartSetValue {
                    key: "prometheus.servicemonitor.prometheusInstance".to_string(),
                    value: "qovery".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    let cert_manager_config = CommonChart {
        chart_info: ChartInfo {
            name: "cert-manager-configs".to_string(),
            path: "lib/common/bootstrap/charts/cert-manager-configs".to_string(),
            namespace: HelmChartNamespaces::CertManager,
            values: vec![
                ChartSetValue {
                    key: "externalDnsProvider".to_string(),
                    value: "cloudflare".to_string(),
                },
                ChartSetValue {
                    key: "provider.cloudflare.apiToken".to_string(),
                    value: vault_secrets.CLOUDFLARE_TOKEN.unwrap().to_string(),
                },
                ChartSetValue {
                    key: "provider.cloudflare.email".to_string(),
                    value: vault_secrets.CLOUDFLARE_ID.as_ref().unwrap().to_string(),
                },
                ChartSetValue {
                    key: "acme.letsEncrypt.emailReport".to_string(),
                    value: vault_secrets.CLOUDFLARE_ID.unwrap().to_string(),
                },
                ChartSetValue {
                    key: "acme.letsEncrypt.acmeUrl".to_string(),
                    value: "https://acme-staging-v02.api.letsencrypt.org/directory".to_string(),
                },
            ],
            ..Default::default()
        },
    };

    (helm, kube_config, cert_manager, cert_manager_config)
}

#[cfg(feature = "test-with-kube")]
#[test]
fn test_create_chart_backup() {
    let (helm, kube_config, cert_manager, cert_manager_config) = cert_manager_conf();

    let lvl_1: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager.clone())];
    let lvl_2: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager_config.clone())];

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_1], false).map_err(|_| assert!(false));

    sleep(Duration::from_secs(30));

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_2], false).map_err(|_| assert!(false));

    let tmp_dir = TempDir::new("workspace_directory").expect("error creating temporary dir");
    let root_dir_path = Path::new(tmp_dir.path());
    let backup_infos = helm
        .prepare_chart_backup(root_dir_path, &cert_manager.chart_info, &vec![], vec!["cert".to_string()])
        .unwrap();
    let secrets = kubectl_exec_get_secrets(
        &helm.kubernetes_config,
        cert_manager.chart_info.namespace.to_string().as_str(),
        "",
        vec![],
    )
    .unwrap();
    assert_eq!(backup_infos.len(), 1);

    for backup_info in backup_infos {
        let backup_name = format!("{}-{}-q-backup", &cert_manager.chart_info.name, backup_info.name.clone());
        assert!(Path::new(backup_info.path.as_str()).exists());
        let secret = secrets
            .items
            .clone()
            .into_iter()
            .filter(|secret| secret.metadata.name == backup_name)
            .collect::<Vec<SecretItem>>();
        let secret_content = decode(secret[0].data[&backup_info.name].clone()).unwrap();
        let content = from_utf8(secret_content.as_slice()).unwrap().to_string();
        let file = OpenOptions::new().read(true).open(backup_info.path.as_str()).unwrap();
        let file_content = BufReader::new(file.try_clone().unwrap())
            .lines()
            .map(|line| line.unwrap())
            .collect::<Vec<String>>()
            .join("\n");
        assert_ne!(content.len(), 0);
        assert_ne!(file_content.len(), 0);
        assert!(content.contains(&file_content));
    }

    let _ = kubectl_exec_delete_namespace(kube_config.as_path(), "cert-manager", vec![]);
}

#[cfg(feature = "test-with-kube")]
#[test]
fn test_apply_chart_backup() {
    let (helm, kube_config, cert_manager, cert_manager_config) = cert_manager_conf();

    let lvl_1: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager.clone())];
    let lvl_2: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager_config.clone())];

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_1], false).map_err(|_| assert!(false));

    sleep(Duration::from_secs(30));

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_2], false).map_err(|_| assert!(false));

    let tmp_dir = TempDir::new("workspace_directory").expect("error creating temporary dir");
    let root_dir_path = Path::new(tmp_dir.path());
    let _ = helm
        .prepare_chart_backup(
            root_dir_path,
            cert_manager_config.get_chart_info(),
            &vec![],
            vec!["cert".to_string()],
        )
        .unwrap();

    match helm.apply_chart_backup(root_dir_path, &vec![], cert_manager_config.get_chart_info()) {
        Err(_) => {
            assert!(false)
        }
        Ok(..) => {
            let string_path = list_yaml_backup_files(root_dir_path).unwrap().first().unwrap().clone();
            let str_path = string_path.as_str();
            let path = Path::new(str_path);
            let backup_string = fs::read_to_string(path).unwrap();
            let cert_string = kubectl_get_resource_yaml(
                kube_config.as_path(),
                vec![],
                "cert",
                Some(cert_manager_config.namespace().as_str()),
            )
            .unwrap();
            let backup_cert = serde_yaml::from_str::<Certificate>(backup_string.as_str()).unwrap();
            let cert = serde_yaml::from_str::<Certificate>(cert_string.as_str()).unwrap();
            assert_eq!(backup_cert.items.first().unwrap().spec, cert.items.first().unwrap().spec)
        }
    };

    let _ = kubectl_exec_delete_namespace(kube_config.as_path(), "cert-manager", vec![]);
}

#[cfg(feature = "test-with-kube")]
#[test]
fn test_should_not_create_chart_backup() {
    let (helm, kube_config, cert_manager, cert_manager_config) = cert_manager_conf();

    let lvl_1: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager.clone())];
    let lvl_2: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager_config.clone())];

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_1], false).map_err(|_| assert!(false));

    sleep(Duration::from_secs(30));

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_2], false).map_err(|_| assert!(false));

    let tmp_dir = TempDir::new("workspace_directory").expect("error creating temporary dir");
    let root_dir_path = Path::new(tmp_dir.path());

    // trying to create a backup from an unknown (toto) resource
    let backup_infos = helm
        .prepare_chart_backup(root_dir_path, &cert_manager.chart_info, &vec![], vec!["toto".to_string()])
        .unwrap();

    assert_eq!(backup_infos.len(), 0);

    let _ = kubectl_exec_delete_namespace(kube_config.as_path(), "cert-manager", vec![]);
}

#[cfg(feature = "test-with-kube")]
#[test]
fn test_should_apply_chart_backup() {
    let (helm, kube_config, cert_manager, mut cert_manager_config) = cert_manager_conf();

    let lvl_1: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager.clone())];
    let lvl_2: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager_config.clone())];

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_1], false).map_err(|_| assert!(false));

    sleep(Duration::from_secs(30));

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_2], false).map_err(|_| assert!(false));

    sleep(Duration::from_secs(30));

    cert_manager_config.chart_info.backup_resources = Some(vec!["cert".to_string()]);

    let lvl_2_bis: Vec<Box<dyn HelmChart>> = vec![Box::new(cert_manager_config.clone())];

    let _ = deploy_charts_levels(kube_config.as_path(), &vec![], vec![lvl_2_bis], false).map_err(|_| assert!(false));

    let secrets = kubectl_exec_get_secrets(
        &helm.kubernetes_config,
        cert_manager.chart_info.namespace.to_string().as_str(),
        "",
        vec![],
    )
    .unwrap();

    let cert_secret = secrets
        .items
        .into_iter()
        .filter(|secret| secret.metadata.name == "cert-manager-configs-cert-q-backup")
        .collect::<Vec<SecretItem>>();

    assert_eq!(cert_secret.len(), 0);

    let cert_string = kubectl_get_resource_yaml(
        kube_config.as_path(),
        vec![],
        "cert",
        Some(cert_manager_config.namespace().as_str()),
    )
    .unwrap();
    let cert = serde_yaml::from_str::<Certificate>(cert_string.as_str()).unwrap();

    assert_ne!(cert.items[0].metadata.annotations.last_applied_configuration, "");

    let _ = kubectl_exec_delete_namespace(kube_config.as_path(), "cert-manager", vec![]);
}
