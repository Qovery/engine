use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::utilities::{check_cname_for, print_action};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::router::reporter::RouterDeploymentReporter;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventMessage, Stage};
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
            event_details.clone(),
            self.logger(),
        );

        // check non custom domains
        self.check_domains(vec![self.default_domain.as_str()], event_details.clone(), self.logger())?;

        let custom_domains_to_check = if self.advanced_settings.custom_domain_check_enabled {
            self.custom_domains.iter().collect::<Vec<_>>()
        } else {
            self.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new("Custom domain check is disabled.".to_string(), None),
            ));

            vec![]
        };

        // Wait/Check that custom domain is a CNAME targeting qovery
        for domain_to_check in custom_domains_to_check {
            match check_cname_for(
                self.progress_scope(),
                self.listeners(),
                &domain_to_check.domain,
                self.context.execution_id(),
            ) {
                Ok(cname) if cname.trim_end_matches('.') == domain_to_check.target_domain.trim_end_matches('.') => {
                    continue;
                }
                Ok(err) | Err(err) => {
                    // TODO(benjaminch): Handle better this one via a proper error eventually
                    self.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            format!(
                                "Invalid CNAME for {}. It might not be an issue if user is using a CDN.",
                                domain_to_check.domain,
                            ),
                            Some(err.to_string()),
                        ),
                    ));
                }
            }
        }

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
            );

            helm.on_delete(target)
            // FIXME: Delete also certificates
        })
    }
}
