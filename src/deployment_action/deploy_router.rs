use crate::cloud_provider::models::CustomDomain;
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::check_dns::CheckDnsForDomains;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::logger::get_loggers;
use crate::deployment_report::router::reporter::RouterDeploymentReporter;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::models::router::{Router, RouterService};
use crate::models::types::{CloudProvider, ToTeraContext};
use function_name::named;
use std::path::PathBuf;

impl<T: CloudProvider> DeploymentAction for Router<T>
where
    Router<T>: RouterService,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Create), || {
            let helm = HelmDeployment::new(
                self.helm_release_name(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                PathBuf::from(self.workspace_directory()),
                event_details.clone(),
                None,
                None,
            );

            helm.on_create(target)
        })
    }

    #[named]
    fn on_create_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        // check non custom domains
        let logger = get_loggers(self, self.action);
        let custom_domains_to_check: Vec<CustomDomain> = if self.advanced_settings.custom_domain_check_enabled {
            self.custom_domains.clone()
        } else {
            vec![]
        };

        let domain_checker = CheckDnsForDomains {
            resolve_to_ip: vec![self.default_domain.clone()],
            resolve_to_cname: custom_domains_to_check,
            log: logger.send_success,
        };

        let _ = domain_checker.on_create_check();
        Ok(())
    }

    #[named]
    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        Ok(())
    }

    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            T::short_name(),
            "router",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(RouterDeploymentReporter::new(self, target, Action::Delete), || {
            let helm = HelmDeployment::new(
                self.helm_release_name(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                PathBuf::from(self.workspace_directory()),
                event_details.clone(),
                None,
                None,
            );

            helm.on_delete(target)
            // FIXME: Delete also certificates
        })
    }
}
