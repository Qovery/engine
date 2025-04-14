use super::InfraLogger;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::Helm;
use crate::errors::{CommandError, EngineError};
use crate::events::{EventDetails, InfrastructureDiffType};
use crate::helm::{HelmAction, HelmChart, HelmChartError};
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::models::CustomerHelmChartsOverride;
use itertools::Itertools;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tera::Context as TeraContext;

pub(super) trait HelmInfraResources {
    type ChartPrerequisite;

    fn charts_context(&self) -> &HelmInfraContext;
    fn new_chart_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite;
    fn gen_charts_to_deploy(
        &self,
        infra_ctx: &InfrastructureContext,
        config: Self::ChartPrerequisite,
    ) -> Result<Vec<Vec<Box<dyn HelmChart>>>, Box<EngineError>>;

    fn deploy_charts(
        &self,
        infra_ctx: &InfrastructureContext,
        logger: &impl InfraLogger,
    ) -> Result<(), Box<EngineError>> {
        logger.info("⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓");
        logger.info("⚓ Preparing Helm files on disk");
        logger.info("⚓ 📥 chart is going to be updated");
        logger.info("⚓ 📤 chart is going to be uninstalled");
        logger.info("⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓");

        self.charts_context().prepare_helm_files_on_disk()?;
        let chart_configs = self.new_chart_prerequisite(infra_ctx);
        let ev_details = &self.charts_context().event_details;
        let charts_to_deploy = self.gen_charts_to_deploy(infra_ctx, chart_configs)?;

        logger.info("🛳️ Going to deploy Helm charts in this sequence:");
        charts_to_deploy.iter().enumerate().for_each(|(ix, charts_lvl)| {
            logger.info(format!("Level {}: {}", ix, charts_names_user_str(charts_lvl)));
        });

        let envs = self
            .charts_context()
            .envs
            .iter()
            .map(|(l, r)| (l.as_str(), r.as_str()))
            .collect_vec();
        let helm = Helm::new(Some(infra_ctx.kubernetes().kubeconfig_local_file_path()), &envs)
            .map_err(|e| Box::new(EngineError::new_helm_chart_error(ev_details.clone(), e.into())))?;

        for (ix, charts_level) in charts_to_deploy.into_iter().enumerate() {
            logger.info("");
            logger.info(format!("🏁 Starting level {}", ix));
            // Show diff for all chart we want to deploy
            charts_level
                .iter()
                .filter(|c| c.get_chart_info().action == HelmAction::Deploy)
                .for_each(|chart| {
                    let mut buf_writer = match create_helm_diff_file(
                        &self.charts_context().destination_folder,
                        &chart.get_chart_info().name,
                    ) {
                        Ok(buf_writer) => buf_writer,
                        Err(err) => {
                            logger.warn(format!(
                                "Unable to create diff file for chart {}: {}",
                                chart.get_chart_info().name,
                                err
                            ));
                            return;
                        }
                    };
                    logger.info(format!("🔍 Showing diff for chart: {}", chart.get_chart_info().name));
                    let _ = helm.upgrade_diff(chart.get_chart_info(), &envs, &mut |line| {
                        let _ = buf_writer.write_all(line.as_bytes());
                        logger.diff(InfrastructureDiffType::Helm, line);
                    });
                });

            // Skip actual deployment if dry run
            if self.charts_context().is_dry_run {
                logger.warn("👻 Dry run mode enabled, skipping actual deployment");
                continue;
            }

            // We do the actual deployment in parallel
            let chart_names = charts_names_user_str(&charts_level);
            logger.info(format!("🛳️ Deploying in parallel charts of level {}: {}", ix, chart_names));
            deploy_parallel_charts(
                infra_ctx.mk_kube_client()?.as_ref(),
                &infra_ctx.kubernetes().kubeconfig_local_file_path(),
                &envs,
                charts_level,
            )
            .map_err(|e| Box::new(EngineError::new_helm_chart_error(ev_details.clone(), e)))?;
            logger.info(format!("✅ Charts of level {} deployed", ix));
        }

        logger.info("⚓ Helm charts deployed successfully");
        logger.info("⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓");

        Ok(())
    }
}

fn charts_names_user_str(charts: &[Box<dyn HelmChart>]) -> String {
    charts
        .iter()
        .map(|c| match c.get_chart_info().action {
            HelmAction::Deploy => format!("📥 {}", c.get_chart_info().name),
            HelmAction::Destroy => format!("📤 {}", c.get_chart_info().name),
        })
        .sorted()
        .join(", ")
}

pub struct HelmInfraContext {
    pub tera_context: TeraContext,
    pub lib_root_dir: PathBuf,
    pub charts_template_dir: PathBuf,
    pub destination_folder: PathBuf,
    pub event_details: EventDetails,
    pub envs: Vec<(String, String)>,
    pub is_dry_run: bool,
}

impl HelmInfraContext {
    pub fn new(
        tera_context: TeraContext,
        lib_root_dir: PathBuf,
        charts_template_dir: PathBuf,
        destination_folder: PathBuf,
        event_details: EventDetails,
        envs: Vec<(String, String)>,
        is_dry_run: bool,
    ) -> Self {
        Self {
            tera_context,
            lib_root_dir,
            charts_template_dir,
            destination_folder,
            event_details,
            envs,
            is_dry_run,
        }
    }

    fn prepare_helm_files_on_disk(&self) -> Result<(), Box<EngineError>> {
        crate::template::generate_and_copy_all_files_into_dir(
            &self.charts_template_dir,
            &self.destination_folder,
            &self.tera_context,
        )
        .map_err(|e| {
            Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                self.event_details.clone(),
                self.charts_template_dir.to_string_lossy().to_string(),
                self.destination_folder.to_string_lossy().to_string(),
                e,
            ))
        })?;
        let dirs_to_be_copied_to = vec![
            // copy lib/common/bootstrap/charts directory (and subdirectory) into the lib/scaleway/bootstrap/common/charts directory.
            // this is due to the required dependencies of lib/scaleway/bootstrap/*.tf files
            (
                self.lib_root_dir
                    .join("common/bootstrap/charts")
                    .to_string_lossy()
                    .to_string(),
                self.destination_folder
                    .join("common/charts")
                    .to_string_lossy()
                    .to_string(),
            ),
            // copy lib/common/bootstrap/chart_values directory (and subdirectory) into the lib/scaleway/bootstrap/common/chart_values directory.
            (
                self.lib_root_dir
                    .join("common/bootstrap/chart_values")
                    .to_string_lossy()
                    .to_string(),
                self.destination_folder
                    .join("common/chart_values")
                    .to_string_lossy()
                    .to_string(),
            ),
        ];
        for (source_dir, target_dir) in dirs_to_be_copied_to {
            if let Err(e) = crate::template::copy_non_template_files(&source_dir, target_dir.as_str()) {
                return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    self.event_details.clone(),
                    source_dir,
                    target_dir,
                    e,
                )));
            }
        }

        Ok(())
    }
}

pub(super) fn mk_customer_chart_override_fn(
    chart_overrides: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
) -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
    match chart_overrides {
        None => Arc::new(|_| None),
        Some(charts_override) => Arc::new(move |chart_name: String| -> Option<CustomerHelmChartsOverride> {
            charts_override
                .get(&chart_name)
                .map(|content| CustomerHelmChartsOverride {
                    chart_name: chart_name.to_string(),
                    chart_values: content.clone(),
                })
        }),
    }
}

fn deploy_parallel_charts(
    kube_client: &kube::Client,
    kubernetes_config: &Path,
    envs: &[(&str, &str)],
    charts: Vec<Box<dyn HelmChart>>,
) -> Result<(), HelmChartError> {
    thread::scope(|s| {
        let mut handles = vec![];

        for chart in charts.into_iter() {
            let path = kubernetes_config.to_path_buf();
            let current_span = tracing::Span::current();
            let handle = s.spawn(move || {
                // making sure to pass the current span to the new thread not to lose any tracing info
                let _span = current_span.enter();
                chart.run(kube_client, path.as_path(), envs, &CommandKiller::never())
            });

            handles.push(handle);
        }

        let mut errors: Vec<Result<(), HelmChartError>> = vec![];
        for handle in handles {
            match handle.join() {
                Ok(helm_run_ret) => {
                    if let Err(e) = helm_run_ret {
                        errors.push(Err(e));
                    }
                }
                Err(e) => {
                    let err = match e.downcast_ref::<&'static str>() {
                        None => match e.downcast_ref::<String>() {
                            None => "Unable to get error.",
                            Some(s) => s.as_str(),
                        },
                        Some(s) => *s,
                    };
                    let error = Err(HelmChartError::CommandError(CommandError::new(
                        "Thread panicked during parallel charts deployments.".to_string(),
                        Some(err.to_string()),
                        None,
                    )));
                    errors.push(error);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            error!("Deployments of charts failed with: {:?}", errors);
            errors.remove(0)
        }
    })
}

fn create_helm_diff_file(dir_path: &Path, chart_name: &str) -> anyhow::Result<BufWriter<File>> {
    use std::fs::{self, OpenOptions};

    let filepath = {
        let filepath = dir_path.join("helm-diffs");
        if !filepath.exists() {
            fs::create_dir_all(&filepath)?;
        }
        filepath.join(format!("{}.diff", chart_name))
    };

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true) // This will ensure the content is overridden
        .open(filepath)?;

    Ok(BufWriter::new(file))
}
