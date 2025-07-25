use crate::environment::action::DeploymentAction;
use crate::environment::action::check_dns::CheckDnsForDomains;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::models::router::Router;
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::router::reporter::RouterDeploymentReporter;
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::io_models::models::CustomDomain;

use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for Router<T>
where
    Router<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let pre_run = |_: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };
        let run = |logger: &EnvProgressLogger, _: ()| -> Result<(), Box<EngineError>> {
            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            helm.on_create(target)?;

            // check non custom domains
            let custom_domains_to_check = self
                .custom_domains
                .clone()
                .into_iter()
                .filter(|it| !it.use_cdn)
                .collect::<Vec<CustomDomain>>();

            let domain_checker = CheckDnsForDomains {
                resolve_to_ip: vec![self.default_domain.clone()],
                resolve_to_cname: custom_domains_to_check,
                log: Box::new(move |msg| logger.info(msg)),
            };
            let _ = domain_checker.on_create(target);

            Ok(())
        };

        let empty_post_run = |_logger: &EnvSuccessLogger, _: ()| {};

        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Create),
            DeploymentTaskImpl {
                pre_run: &pre_run,
                run: &run,
                post_run_success: &empty_post_run,
            },
        )
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Pause),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) },
        )
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Delete),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                let chart = ChartInfo {
                    name: self.helm_release_name(),
                    namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
                    action: HelmAction::Destroy,
                    ..Default::default()
                };
                let helm = HelmDeployment::new(
                    self.get_event_details(Stage::Environment(EnvironmentStep::Delete)),
                    self.to_tera_context(target)?,
                    PathBuf::from(self.helm_chart_dir().as_str()),
                    None,
                    chart,
                );

                helm.on_delete(target)
                // FIXME: Delete also certificates
            },
        )
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            RouterDeploymentReporter::new(self, target, Action::Restart),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) },
        )
    }
}
