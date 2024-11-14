use super::InfraLogger;
use crate::cloud_provider::helm::{deploy_charts_levels, HelmChart};
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::EventDetails;
use itertools::Itertools;
use std::path::PathBuf;
use tera::Context as TeraContext;

pub(super) trait HelmInfraResources {
    type ChartPrerequisite;

    fn charts_context(&self) -> &HelmInfraContext;
    fn to_chart_config_prerequisite(&self, infra_ctx: &InfrastructureContext) -> Self::ChartPrerequisite;
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
        logger.info("Preparing helm files on disk");
        self.charts_context().prepare_helm_files_on_disk()?;
        let chart_configs = self.to_chart_config_prerequisite(infra_ctx);
        let charts_to_deploy = self.gen_charts_to_deploy(infra_ctx, chart_configs)?;

        logger.info("Going to deploy helm charts in this sequence:");
        charts_to_deploy.iter().enumerate().for_each(|(ix, charts_lvl)| {
            let chart_names = charts_lvl.iter().map(|c| &c.get_chart_info().name).sorted().join(", ");
            logger.info(format!("Level {}: {}", ix, chart_names));
        });

        deploy_charts_levels(
            infra_ctx.mk_kube_client()?.client(),
            &infra_ctx.kubernetes().kubeconfig_local_file_path(),
            self.charts_context()
                .envs
                .iter()
                .map(|(l, r)| (l.as_str(), r.as_str()))
                .collect_vec()
                .as_slice(),
            charts_to_deploy,
            self.charts_context().is_dry_run,
            Some(&infra_ctx.kubernetes().helm_charts_diffs_directory()),
        )
        .map_err(|e| {
            Box::new(EngineError::new_helm_chart_error(
                self.charts_context().event_details.clone(),
                e,
            ))
        })
    }
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
