use crate::cloud_provider::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::cloud_provider::models::CustomDomain;
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::check_dns::CheckDnsForDomains;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::router::reporter::RouterDeploymentReporter;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::models::router::Router;
use crate::models::types::{CloudProvider, ToTeraContext};

use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for Router<T>
where
    Router<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Create), || {
            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            helm.on_create(target)
        })
    }

    fn on_create_check(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        // check non custom domains
        let custom_domains_to_check: Vec<CustomDomain> = if self.advanced_settings.custom_domain_check_enabled {
            self.custom_domains.clone()
        } else {
            vec![]
        };

        let logger = target.env_logger(self, EnvironmentStep::Deploy);
        let domain_checker = CheckDnsForDomains {
            resolve_to_ip: vec![self.default_domain.clone()],
            resolve_to_cname: custom_domains_to_check,
            log: Box::new(move |msg| logger.send_success(msg)),
        };

        let _ = domain_checker.on_create_check(target);
        if (target.should_abort)() {
            Err(EngineError::new_task_cancellation_requested(
                self.get_event_details(Stage::Environment(EnvironmentStep::Cancelled)),
            ))
        } else {
            Ok(())
        }
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Pause), || Ok(()))
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Delete), || {
            let chart = ChartInfo {
                name: self.helm_release_name(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
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
        })
    }
}
