use crate::cloud_provider::helm::ChartInfo;
use crate::cloud_provider::service::{delete_stateless_service, Action, Helm, Service};
use crate::cloud_provider::utilities::{check_cname_for, print_action};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm;
use crate::cmd::helm::to_engine_error;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::router::reporter::RouterDeploymentReporter;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventMessage, Stage};
use crate::io_models::Listen;
use crate::models::router::{Router, RouterService};
use crate::models::types::CloudProvider;
use function_name::named;

impl<T: CloudProvider> DeploymentAction for Router<T>
where
    Router<T>: Service,
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
            let kubernetes = target.kubernetes;
            let environment = target.environment;
            let workspace_dir = self.workspace_directory();
            let helm_release_name = self.helm_release_name();

            let kubernetes_config_file_path = kubernetes.get_kubeconfig_file_path()?;

            // respect order - getting the context here and not before is mandatory
            // the nginx-ingress must be available to get the external dns target if necessary
            let context = self.tera_context(target)?;

            let from_dir = format!(
                "{}/{}/charts/q-ingress-tls",
                self.context.lib_root_dir(),
                T::lib_directory_name()
            );
            if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
                from_dir.as_str(),
                workspace_dir.as_str(),
                context,
            ) {
                return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    event_details.clone(),
                    from_dir,
                    workspace_dir,
                    e,
                ));
            }

            // do exec helm upgrade and return the last deployment status
            let helm = helm::Helm::new(
                &kubernetes_config_file_path,
                &kubernetes.cloud_provider().credentials_environment_variables(),
            )
            .map_err(|e| to_engine_error(&event_details, e))?;

            let chart = ChartInfo::new_from_custom_namespace(
                helm_release_name,
                workspace_dir,
                environment.namespace().to_string(),
                600_i64,
                vec![],
                vec![],
                vec![],
                false,
                self.selector(),
            );

            helm.upgrade(&chart, &[])
                .map_err(|e| EngineError::new_helm_error(event_details.clone(), e))
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
    fn on_pause_check(&self) -> Result<(), EngineError> {
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
            delete_stateless_service(target, self, event_details.clone())
        })
    }

    #[named]
    fn on_delete_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
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
}
