use base64::decode;
use qovery_engine::cloud_provider::helm::{
    deploy_charts_levels, ChartInfo, ChartSetValue, CommonChart, HelmChart, HelmChartNamespaces,
};
use qovery_engine::cmd::helm::Helm;
use qovery_engine::cmd::kubectl::{kubectl_exec_delete_namespace, kubectl_exec_get_secrets};
use qovery_engine::cmd::structs::SecretItem;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::from_utf8;
use std::thread::sleep;
use std::time::Duration;
use tempdir::TempDir;
use test_utilities::utilities::FuncTestsSecrets;

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

    for (name, path) in backup_infos {
        let backup_name = format!("{}-{}-q-backup", &cert_manager.chart_info.name, name.clone());
        assert!(Path::new(path.as_str()).exists());
        let secret = secrets
            .items
            .clone()
            .into_iter()
            .filter(|secret| secret.metadata.name == backup_name)
            .collect::<Vec<SecretItem>>();
        let secret_content = decode(secret[0].data[&name].clone()).unwrap();
        let content = from_utf8(secret_content.as_slice()).unwrap().to_string();
        let file = OpenOptions::new().read(true).open(path.as_str()).unwrap();
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
    let backup_infos = helm
        .prepare_chart_backup(root_dir_path, &cert_manager.chart_info, &vec![], vec!["cert".to_string()])
        .unwrap();

    let _ = kubectl_exec_delete_namespace(kube_config.as_path(), "cert-manager", vec![]);
}
