use crate::cloud_provider::helm::HelmAction::Deploy;
use crate::cloud_provider::helm::HelmChartNamespaces::KubeSystem;
use crate::cmd::helm::{
    helm_exec_uninstall_with_chart_info, helm_exec_upgrade_with_chart_info, helm_upgrade_diff_with_chart_info,
};
use crate::cmd::kubectl::{
    kubectl_exec_get_configmap, kubectl_exec_get_events, kubectl_exec_rollout_restart_deployment,
    kubectl_exec_with_output,
};
use crate::cmd::structs::HelmHistoryRow;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::utilities::calculate_hash;
use std::collections::HashMap;
use std::path::Path;
use std::{fs, thread};
use thread::spawn;
use tracing::{span, Level};

#[derive(Clone)]
pub enum HelmAction {
    Deploy,
    Destroy,
    Skip,
}

#[derive(Copy, Clone)]
pub enum HelmChartNamespaces {
    KubeSystem,
    Prometheus,
    Logging,
    CertManager,
    NginxIngress,
    Qovery,
}

#[derive(Clone)]
pub struct ChartSetValue {
    pub key: String,
    pub value: String,
}

#[derive(Clone)]
pub struct ChartValuesGenerated {
    pub filename: String,
    pub yaml_content: String,
}

#[derive(Clone)]
pub struct ChartInfo {
    pub name: String,
    pub path: String,
    pub namespace: HelmChartNamespaces,
    pub action: HelmAction,
    pub atomic: bool,
    pub force_upgrade: bool,
    pub timeout: String,
    pub dry_run: bool,
    pub wait: bool,
    pub values: Vec<ChartSetValue>,
    pub values_files: Vec<String>,
    pub yaml_files_content: Vec<ChartValuesGenerated>,
}

impl Default for ChartInfo {
    fn default() -> ChartInfo {
        ChartInfo {
            name: "undefined".to_string(),
            path: "undefined".to_string(),
            namespace: KubeSystem,
            action: Deploy,
            atomic: true,
            force_upgrade: false,
            timeout: "180s".to_string(),
            dry_run: false,
            wait: true,
            values: Vec::new(),
            values_files: Vec::new(),
            yaml_files_content: vec![],
        }
    }
}

pub fn get_chart_namespace(namespace: HelmChartNamespaces) -> String {
    match namespace {
        HelmChartNamespaces::KubeSystem => "kube-system",
        HelmChartNamespaces::Prometheus => "prometheus",
        HelmChartNamespaces::Logging => "logging",
        HelmChartNamespaces::CertManager => "cert-manager",
        HelmChartNamespaces::NginxIngress => "nginx-ingress",
        HelmChartNamespaces::Qovery => "qovery",
    }
    .to_string()
}

pub trait HelmChart: Send {
    fn check_prerequisites(&self) -> Result<Option<ChartPayload>, SimpleError> {
        let chart = self.get_chart_info();
        for file in chart.values_files.iter() {
            match fs::metadata(file) {
                Ok(_) => {}
                Err(e) => {
                    return Err(SimpleError {
                        kind: SimpleErrorKind::Other,
                        message: Some(format!(
                            "Can't access helm chart override file {} for chart {}. {:?}",
                            file, chart.name, e
                        )),
                    })
                }
            }
        }
        Ok(None)
    }

    fn get_chart_info(&self) -> &ChartInfo;

    fn namespace(&self) -> String {
        get_chart_namespace(self.get_chart_info().namespace)
    }

    fn pre_exec(
        &self,
        _kubernetes_config: &Path,
        _envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, SimpleError> {
        Ok(payload)
    }

    fn run(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> Result<Option<ChartPayload>, SimpleError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        let payload = self.check_prerequisites()?;
        let payload = self.pre_exec(&kubernetes_config, &envs, payload)?;
        let payload = match self.exec(&kubernetes_config, &envs, payload.clone()) {
            Ok(payload) => payload,
            Err(e) => {
                error!(
                    "Error while deploying chart: {:?}",
                    e.message.clone().expect("no error message provided")
                );
                self.on_deploy_failure(&kubernetes_config, &envs, payload)?;
                return Err(e);
            }
        };
        let payload = self.post_exec(&kubernetes_config, &envs, payload)?;
        Ok(payload)
    }

    fn exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, SimpleError> {
        let environment_variables = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        match self.get_chart_info().action {
            HelmAction::Deploy => {
                helm_exec_upgrade_with_chart_info(kubernetes_config, &environment_variables, self.get_chart_info())?
            }
            HelmAction::Destroy => {
                helm_exec_uninstall_with_chart_info(kubernetes_config, &environment_variables, self.get_chart_info())?
            }
            HelmAction::Skip => {}
        }
        Ok(payload)
    }

    fn post_exec(
        &self,
        _kubernetes_config: &Path,
        _envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, SimpleError> {
        Ok(payload)
    }

    fn on_deploy_failure(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, SimpleError> {
        // print events for future investigation
        let environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        kubectl_exec_get_events(
            kubernetes_config,
            get_chart_namespace(self.get_chart_info().namespace).as_str(),
            environment_variables,
        )?;
        Ok(payload)
    }
}

fn deploy_parallel_charts(
    kubernetes_config: &Path,
    envs: &[(String, String)],
    charts: Vec<Box<dyn HelmChart>>,
) -> Result<(), SimpleError> {
    let mut handles = vec![];

    for chart in charts.into_iter() {
        let environment_variables = envs.to_owned();
        let path = kubernetes_config.to_path_buf();
        let current_span = tracing::Span::current();
        let handle = spawn(move || {
            // making sure to pass the current span to the new thread not to lose any tracing info
            span!(parent: &current_span, Level::INFO, "") // empty span name to reduce logs length
                .in_scope(|| chart.run(path.as_path(), &environment_variables))
        });
        handles.push(handle);
    }

    for handle in handles {
        match handle.join() {
            Ok(helm_run_ret) => match helm_run_ret {
                Ok(_) => {}
                Err(e) => return Err(e),
            },
            Err(e) => {
                error!("{:?}", e);
                return Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some("thread panicked during parallel charts deployments".to_string()),
                });
            }
        }
    }

    Ok(())
}

pub fn deploy_charts_levels(
    kubernetes_config: &Path,
    envs: &Vec<(String, String)>,
    charts: Vec<Vec<Box<dyn HelmChart>>>,
    dry_run: bool,
) -> Result<(), SimpleError> {
    // first show diff
    for level in &charts {
        for chart in level {
            let _ = helm_upgrade_diff_with_chart_info(&kubernetes_config, envs, chart.get_chart_info());
        }
    }

    // then apply
    if dry_run {
        return Ok(());
    }
    for level in charts.into_iter() {
        match deploy_parallel_charts(&kubernetes_config, &envs, level) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

//
// Common charts
//

#[derive(Default)]
pub struct CommonChart {
    pub chart_info: ChartInfo,
}

/// using ChartPayload to pass random kind of data between each deployment steps against a chart deployment
#[derive(Clone)]
pub struct ChartPayload {
    data: HashMap<String, String>,
}

impl CommonChart {}

impl HelmChart for CommonChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }
}

// CoreDNS config

#[derive(Default)]
pub struct CoreDNSConfigChart {
    pub chart_info: ChartInfo,
}

impl HelmChart for CoreDNSConfigChart {
    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }

    fn pre_exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        _payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, SimpleError> {
        let kind = "configmap";
        let mut environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
        environment_variables.push(("KUBECONFIG", kubernetes_config.to_str().unwrap()));

        // calculate current configmap checksum
        let current_configmap_hash = match kubectl_exec_get_configmap(
            &kubernetes_config,
            &get_chart_namespace(self.chart_info.namespace),
            &self.chart_info.name,
            environment_variables.clone(),
        ) {
            Ok(cm) => {
                if cm.data.corefile.is_none() {
                    return Err(SimpleError {
                        kind: SimpleErrorKind::Other,
                        message: Some("Corefile data structure is not found in CoreDNS configmap".to_string()),
                    });
                };
                calculate_hash(&cm.data.corefile.unwrap())
            }
            Err(e) => return Err(e),
        };
        let mut configmap_hash = HashMap::new();
        configmap_hash.insert("checksum".to_string(), current_configmap_hash.to_string());
        let payload = ChartPayload { data: configmap_hash };

        // set labels and annotations to give helm ownership
        info!("setting annotations and labels on {}/{}", &kind, &self.chart_info.name);
        let steps = || -> Result<(), SimpleError> {
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    &kind,
                    &self.chart_info.name,
                    format!("meta.helm.sh/release-name={}", self.chart_info.name).as_str(),
                ],
                environment_variables.clone(),
                |_| {},
                |_| {},
            )?;
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "annotate",
                    "--overwrite",
                    &kind,
                    &self.chart_info.name,
                    "meta.helm.sh/release-namespace=kube-system",
                ],
                environment_variables.clone(),
                |_| {},
                |_| {},
            )?;
            kubectl_exec_with_output(
                vec![
                    "-n",
                    "kube-system",
                    "label",
                    "--overwrite",
                    &kind,
                    &self.chart_info.name,
                    "app.kubernetes.io/managed-by=Helm",
                ],
                environment_variables.clone(),
                |_| {},
                |_| {},
            )?;
            Ok(())
        };
        if let Err(e) = steps() {
            return Err(e);
        };

        Ok(Some(payload))
    }

    fn run(&self, kubernetes_config: &Path, envs: &[(String, String)]) -> Result<Option<ChartPayload>, SimpleError> {
        info!("prepare and deploy chart {}", &self.get_chart_info().name);
        self.check_prerequisites()?;
        let payload = match self.pre_exec(&kubernetes_config, &envs, None) {
            Ok(p) => match p {
                None => {
                    return Err(SimpleError {
                        kind: SimpleErrorKind::Other,
                        message: Some(
                            "CoreDNS configmap checksum couldn't be get, can't deploy CoreDNS chart".to_string(),
                        ),
                    })
                }
                Some(p) => p,
            },
            Err(e) => return Err(e),
        };
        if let Err(e) = self.exec(&kubernetes_config, &envs, None) {
            error!(
                "Error while deploying chart: {:?}",
                e.message.clone().expect("no message provided")
            );
            self.on_deploy_failure(&kubernetes_config, &envs, None)?;
            return Err(e);
        };
        self.post_exec(&kubernetes_config, &envs, Some(payload))?;
        Ok(None)
    }

    fn post_exec(
        &self,
        kubernetes_config: &Path,
        envs: &[(String, String)],
        payload: Option<ChartPayload>,
    ) -> Result<Option<ChartPayload>, SimpleError> {
        let mut environment_variables = Vec::new();
        for env in envs {
            environment_variables.push((env.0.as_str(), env.1.as_str()));
        }

        // detect configmap data change
        let previous_configmap_checksum = match &payload {
            None => {
                return Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some("missing payload, can't check coredns update".to_string()),
                })
            }
            Some(x) => match x.data.get("checksum") {
                None => {
                    return Err(SimpleError {
                        kind: SimpleErrorKind::Other,
                        message: Some("missing configmap checksum, can't check coredns diff".to_string()),
                    })
                }
                Some(c) => c.clone(),
            },
        };
        let current_configmap_checksum = match kubectl_exec_get_configmap(
            &kubernetes_config,
            &get_chart_namespace(self.chart_info.namespace),
            &self.chart_info.name,
            environment_variables.clone(),
        ) {
            Ok(cm) => {
                if cm.data.corefile.is_none() {
                    return Err(SimpleError {
                        kind: SimpleErrorKind::Other,
                        message: Some("Corefile data structure is not found in CoreDNS configmap".to_string()),
                    });
                };
                calculate_hash(&cm.data.corefile.unwrap()).to_string()
            }
            Err(e) => return Err(e),
        };
        if previous_configmap_checksum == current_configmap_checksum {
            info!("no coredns config change detected, nothing to restart");
            return Ok(None);
        }

        // avoid rebooting coredns on every run
        info!("coredns config change detected, proceed to config reload");
        kubectl_exec_rollout_restart_deployment(
            kubernetes_config,
            &self.chart_info.name,
            self.namespace().as_str(),
            &environment_variables,
        )?;
        Ok(None)
    }
}

// Qovery Portal

// #[derive(Default)]
// pub struct QoveryPortalChart {
//     pub chart_info: ChartInfo,
// }
//
// impl HelmChart for QoveryPortalChart {
//     fn get_chart_info(&self) -> &ChartInfo {
//         &self.chart_info
//     }
//
//     fn pre_exec(
//         &self,
//         kubernetes_config: &Path,
//         envs: &[(String, String)],
//         _payload: Option<ChartPayload>,
//     ) -> Result<Option<ChartPayload>, SimpleError> {
//         let mut environment_variables: Vec<(&str, &str)> = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
//         let cluster_default_ingress_loadbalancer_address = match kubectl_exec_get_external_ingress_hostname(
//             &kubernetes_config,
//             &get_chart_namespace(HelmChartNamespaces::NginxIngress), // todo: would be better to get it directly from the chart itself
//             "app=nginx-ingress,component=controller",
//             environment_variables,
//         ) {
//             Ok(x) => {
//                 if x.is_some() {
//                     x.unwrap()
//                 } else {
//                     return Err(SimpleError {
//                         kind: SimpleErrorKind::Other,
//                         message: Some(format!(
//                             "No default Nginx ingress was found, can't deploy Qovery portal. {:?}",
//                             e.message
//                         )),
//                     });
//                 }
//             }
//             Err(e) => {
//                 return Err(SimpleError {
//                     kind: SimpleErrorKind::Other,
//                     message: Some(format!(
//                         "Error while trying to get default Nginx ingress to deploy Qovery portal. {:?}",
//                         e.message
//                     )),
//                 })
//             }
//         };
//         let mut configmap_hash = HashMap::new();
//         configmap_hash.insert(
//             "loadbalancer_address".to_string(),
//             cluster_default_ingress_loadbalancer_address,
//         );
//         let payload = ChartPayload { data: configmap_hash };
//
//         Ok(Some(payload))
//     }
//
//     fn exec(
//         &self,
//         kubernetes_config: &Path,
//         envs: &[(String, String)],
//         payload: Option<ChartPayload>,
//     ) -> Result<Option<ChartPayload>, SimpleError> {
//         if payload.is_none() {
//             return Err(SimpleError {
//                 kind: SimpleErrorKind::Other,
//                 message: Some("payload is missing for qovery-portal chart".to_string()),
//             });
//         }
//         let external_dns_target = match payload.unwrap().data.get("loadbalancer_address") {
//             None => {
//                 return Err(SimpleError {
//                     kind: SimpleErrorKind::Other,
//                     message: Some("loadbalancer_address payload is missing, can't deploy qovery portal".to_string()),
//                 })
//             }
//             Some(x) => x.into_string(),
//         };
//
//         let environment_variables = envs.iter().map(|x| (x.0.as_str(), x.1.as_str())).collect();
//         let mut chart = self.chart_info.clone();
//         chart.values.push(ChartSetValue {
//             key: "externalDnsTarget".to_string(),
//             value: external_dns_target,
//         });
//
//         match self.get_chart_info().action {
//             HelmAction::Deploy => helm_exec_upgrade_with_chart_info(kubernetes_config, &environment_variables, &chart)?,
//             HelmAction::Destroy => {
//                 helm_exec_uninstall_with_chart_info(kubernetes_config, &environment_variables, &chart)?
//             }
//             HelmAction::Skip => {}
//         }
//         Ok(payload)
//     }
// }

pub fn get_latest_successful_deployment(helm_history_list: &[HelmHistoryRow]) -> Result<HelmHistoryRow, SimpleError> {
    let mut helm_history_reversed = helm_history_list.to_owned();
    helm_history_reversed.reverse();

    for revision in helm_history_reversed.clone() {
        if revision.status == "deployed" {
            return Ok(revision);
        }
    }

    Err(SimpleError {
        kind: SimpleErrorKind::Other,
        message: Some(format!(
            "no succeed revision found for chart {}",
            helm_history_reversed[0].chart
        )),
    })
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::get_latest_successful_deployment;
    use crate::cmd::structs::HelmHistoryRow;

    #[test]
    fn test_last_succeeded_deployment() {
        let payload = r#"
        [
            {
                "revision": 1,
                "updated": "2021-06-17T08:37:37.687890192+02:00",
                "status": "superseded",
                "chart": "coredns-config-0.1.0",
                "app_version": "0.1",
                "description": "Install complete"
            },
            {
                "revision": 2,
                "updated": "2021-06-17T12:34:08.958006444+02:00",
                "status": "deployed",
                "chart": "coredns-config-0.1.0",
                "app_version": "0.1",
                "description": "Upgrade complete"
            },
            {
                "revision": 3,
                "updated": "2021-06-17T12:36:08.958006444+02:00",
                "status": "failed",
                "chart": "coredns-config-0.1.0",
                "app_version": "0.1",
                "description": "Failed complete"
            }
        ]
        "#;

        let results = serde_json::from_str::<Vec<HelmHistoryRow>>(payload).unwrap();
        let final_succeed = get_latest_successful_deployment(&results).unwrap();
        assert_eq!(results[1].updated, final_succeed.updated);
    }
}
