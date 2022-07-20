use crate::cloud_provider::service::{
    delete_managed_stateful_service, delete_pending_service, deploy_managed_database_service, Action, Helm, Service,
};
use crate::cloud_provider::utilities::{check_domain_for, print_action};
use crate::cloud_provider::Kind::Aws;
use crate::cloud_provider::{service, DeploymentTarget};
use crate::cmd::command::QoveryCommand;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::pause_service::PauseServiceAction;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::database::reporter::DatabaseDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage};
use crate::io_models::ListenersHelper;
use crate::models::database::{Container, Database, DatabaseService, DatabaseType, Managed};
use crate::models::types::{CloudProvider, ToTeraContext};
use function_name::named;
use std::path::PathBuf;
use std::time::Duration;

// For Managed database
impl<C: CloudProvider, T: DatabaseType<C, Managed>> DeploymentAction for Database<C, Managed, T>
where
    Database<C, Managed, T>: ToTeraContext,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Create), || {
            deploy_managed_database_service(target, self, event_details.clone())
        })
    }

    #[named]
    fn on_create_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        if self.publicly_accessible() {
            check_domain_for(
                ListenersHelper::new(&self.listeners),
                vec![&self.fqdn],
                self.context.execution_id(),
                self.context.execution_id(),
                event_details,
                self.logger(),
            )?;
        }

        Ok(())
    }

    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Pause), || {
            // We don't manage PAUSE for managed database elsewhere than for AWS
            if target.kubernetes.cloud_provider().kind() != Aws {
                return Ok(());
            }

            let mut output_stdout: Vec<String> = vec![];
            let mut output_stderr: Vec<String> = vec![];
            let ret = match self.db_type() {
                service::DatabaseType::PostgreSQL | service::DatabaseType::MySQL => {
                    // We use the fqdn_id as db identifier, why not id or name like everything else ¯\_(ツ)_/¯
                    let mut cmd = QoveryCommand::new(
                        "aws",
                        &["rds", "stop-db-instance", "--db-instance-identifier", &self.fqdn_id],
                        &target.kubernetes.cloud_provider().credentials_environment_variables(),
                    );
                    cmd.exec_with_output(&mut |line| output_stdout.push(line), &mut |line| output_stderr.push(line))
                }
                service::DatabaseType::MongoDB => {
                    // We use the fqdn_id as db identifier, why not id or name like everything else ¯\_(ツ)_/¯
                    let mut cmd = QoveryCommand::new(
                        "aws",
                        &["docdb", "stop-db-cluster", "--db-cluster-identifier", &self.fqdn_id],
                        &target.kubernetes.cloud_provider().credentials_environment_variables(),
                    );
                    cmd.exec_with_output(&mut |line| output_stdout.push(line), &mut |line| output_stderr.push(line))
                }
                service::DatabaseType::Redis => {
                    // can't pause elasticache
                    Ok(())
                }
            };

            output_stdout.extend(output_stderr);
            if let Err(cmd_error) = ret {
                Err(EngineError::new_cannot_pause_managed_database(
                    event_details.clone(),
                    CommandError::new_from_legacy_command_error(
                        cmd_error,
                        Some(output_stdout.join("\n").trim().to_string()),
                    ),
                ))
            } else {
                Ok(())
            }
        })
    }

    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Delete), || {
            delete_managed_stateful_service(target, self, event_details.clone(), self.logger())
        })
    }
}

// For Container database
impl<C: CloudProvider, T: DatabaseType<C, Container>> DeploymentAction for Database<C, Container, T>
where
    Database<C, Container, T>: ToTeraContext,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Create), || {
            let helm = HelmDeployment::new_with_values_override(
                self.helm_release_name(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                PathBuf::from(self.workspace_directory()),
                PathBuf::from(self.helm_chart_values_dir()),
                event_details.clone(),
                Some(self.selector()),
            );

            helm.on_create(target)?;

            delete_pending_service(
                target.kubernetes.get_kubeconfig_file_path()?.as_str(),
                target.environment.namespace(),
                self.selector().as_str(),
                target.kubernetes.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
            )?;

            Ok(())
        })
    }

    #[named]
    fn on_create_check(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        if self.publicly_accessible() {
            check_domain_for(
                ListenersHelper::new(&self.listeners),
                vec![&self.fqdn],
                self.context.execution_id(),
                self.context.execution_id(),
                event_details,
                self.logger(),
            )?;
        }

        Ok(())
    }

    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Pause), || {
            let pause_service = PauseServiceAction::new(
                self.selector(),
                true,
                Duration::from_secs(5 * 60),
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
            );
            pause_service.on_pause(target)
        })
    }

    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            C::short_name(),
            T::db_type().to_string().as_str(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(DatabaseDeploymentReporter::new(self, target, Action::Delete), || {
            let helm = HelmDeployment::new_with_values_override(
                self.helm_release_name(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                PathBuf::from(self.workspace_directory()),
                PathBuf::from(self.helm_chart_values_dir()),
                event_details.clone(),
                Some(self.selector()),
            );

            helm.on_delete(target)
            // FIXME delete pvc
        })
    }
}
