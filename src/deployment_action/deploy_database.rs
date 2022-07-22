use crate::cloud_provider::service::{
    delete_managed_stateful_service, delete_pending_service, deploy_managed_database_service, Action, Helm, Service,
};
use crate::cloud_provider::utilities::{check_domain_for, print_action};
use crate::cloud_provider::Kind::Aws;
use crate::cloud_provider::{service, DeploymentTarget};
use crate::cmd;
use crate::cmd::command::QoveryCommand;
use crate::constants::AWS_DEFAULT_REGION;
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
use serde::Deserialize;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const DB_READY_STATE: &str = "available";
const DB_STOPPED_STATE: &str = "stopped";

#[derive(Deserialize, Default)]
struct CacheCluster {
    #[serde(alias = "CacheClusterStatus")]
    pub cache_cluster_status: String,
}

#[derive(Deserialize, Default)]
struct CacheClustersResponse {
    #[serde(alias = "CacheClusters")]
    pub cache_clusters: Vec<CacheCluster>,
}

#[derive(Deserialize, Default)]
struct DbInstance {
    #[serde(alias = "DBInstanceStatus")]
    pub db_instance_status: String,
}
#[derive(Deserialize, Default)]
struct DbInstancesResponse {
    #[serde(alias = "DBInstances")]
    pub db_instances: Vec<DbInstance>,
}

#[derive(Deserialize, Default)]
struct DocDbCluster {
    #[serde(alias = "Status")]
    pub status: String,
}

#[derive(Deserialize, Default)]
struct DocDbClustersResponse {
    #[serde(alias = "DBClusters")]
    pub db_cluster: Vec<DocDbCluster>,
}

fn get_managed_database_status(
    db_type: service::DatabaseType,
    db_id: &str,
    credentials: &[(&str, &str)],
) -> Result<String, (cmd::command::CommandError, String)> {
    let mut cmd = match db_type {
        service::DatabaseType::PostgreSQL | service::DatabaseType::MySQL => QoveryCommand::new(
            "aws",
            &["rds", "describe-db-instances", "--db-instance-identifier", db_id],
            credentials,
        ),
        service::DatabaseType::MongoDB => QoveryCommand::new(
            "aws",
            &["docdb", "describe-db-clusters", "--db-cluster-identifier", db_id],
            credentials,
        ),
        service::DatabaseType::Redis => QoveryCommand::new(
            "aws",
            &["elasticache", "describe-cache-clusters", "--cache-cluster-id", db_id],
            credentials,
        ),
    };

    let mut output_stdout: Vec<String> = vec![];
    let mut output_stderr: Vec<String> = vec![];
    let cmd_ret = cmd.exec_with_output(&mut |line| output_stdout.push(line), &mut |line| output_stderr.push(line));

    if let Err(cmd_error) = cmd_ret {
        output_stdout.extend(output_stderr);
        return Err((cmd_error, output_stdout.join("\n").trim().to_string()));
    }

    match db_type {
        service::DatabaseType::PostgreSQL | service::DatabaseType::MySQL => {
            let payload: DbInstancesResponse =
                serde_json::from_str(output_stdout.join("").as_str()).unwrap_or_default();
            Ok(payload
                .db_instances
                .first()
                .map(|c| c.db_instance_status.clone())
                .unwrap_or_default())
        }
        service::DatabaseType::MongoDB => {
            let payload: DocDbClustersResponse =
                serde_json::from_str(output_stdout.join("").as_str()).unwrap_or_default();
            Ok(payload.db_cluster.first().map(|c| c.status.clone()).unwrap_or_default())
        }
        service::DatabaseType::Redis => {
            let payload: CacheClustersResponse =
                serde_json::from_str(output_stdout.join("").as_str()).unwrap_or_default();
            Ok(payload
                .cache_clusters
                .first()
                .map(|c| c.cache_cluster_status.clone())
                .unwrap_or_default())
        }
    }
}

fn start_stop_managed_database(
    db_type: service::DatabaseType,
    db_id: &str,
    credentials: &[(&str, &str)],
    should_stop: bool,
) -> Result<(), (cmd::command::CommandError, String)> {
    let action = if should_stop { "stop" } else { "start" };

    let mut output_stdout: Vec<String> = vec![];
    let mut output_stderr: Vec<String> = vec![];
    let ret = match db_type {
        service::DatabaseType::PostgreSQL | service::DatabaseType::MySQL => {
            let mut cmd = QoveryCommand::new(
                "aws",
                &[
                    "rds",
                    &format!("{}-db-instance", action),
                    "--db-instance-identifier",
                    db_id,
                ],
                credentials,
            );
            cmd.exec_with_output(&mut |line| output_stdout.push(line), &mut |line| output_stderr.push(line))
        }
        service::DatabaseType::MongoDB => {
            let mut cmd = QoveryCommand::new(
                "aws",
                &[
                    "docdb",
                    &format!("{}-db-cluster", action),
                    "--db-cluster-identifier",
                    db_id,
                ],
                credentials,
            );
            cmd.exec_with_output(&mut |line| output_stdout.push(line), &mut |line| output_stderr.push(line))
        }
        service::DatabaseType::Redis => {
            // can't pause elasticache
            Ok(())
        }
    };

    if let Err(cmd_error) = ret {
        output_stdout.extend(output_stderr);
        Err((cmd_error, output_stdout.join("\n").trim().to_string()))
    } else {
        Ok(())
    }
}

fn await_db_state(
    timeout: Duration,
    db_type: service::DatabaseType,
    db_id: &str,
    credentials: &[(&str, &str)],
    state: &str,
) -> Result<(), Option<(cmd::command::CommandError, String)>> {
    // Wait for the database to be in given state
    let now = Instant::now();
    loop {
        if now.elapsed() >= timeout {
            break Err(None);
        }

        match get_managed_database_status(db_type, db_id, credentials) {
            Ok(status) if status == state => break Ok(()),
            Ok(_) => thread::sleep(Duration::from_secs(30)),
            Err(err) => break Err(Some(err)),
        }
    }
}

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
            deploy_managed_database_service(target, self, event_details.clone())?;

            // We don't manage START/PAUSE for managed database elsewhere than for AWS
            if target.kubernetes.cloud_provider().kind() != Aws {
                return Ok(());
            }

            // Terraform does not ensure that the database is correctly started
            // So we must force it ourselves in case
            let credentials = {
                let mut credentials = target.kubernetes.cloud_provider().credentials_environment_variables();
                credentials.push((AWS_DEFAULT_REGION, target.kubernetes.region()));
                credentials
            };

            // If the database is not in the available state, try to start it
            match get_managed_database_status(self.db_type(), &self.fqdn_id, &credentials) {
                Ok(status) if status == DB_READY_STATE => {}
                Ok(_) | Err(_) => {
                    let _ = start_stop_managed_database(self.db_type(), &self.fqdn_id, &credentials, false);
                }
            }

            let ret = await_db_state(
                Duration::from_secs(60 * 30),
                self.db_type(),
                &self.fqdn_id,
                &credentials,
                DB_READY_STATE,
            );

            match ret {
                Ok(_) => Ok(()),
                // timeout
                Err(None) => Err(EngineError::new_database_failed_to_start_after_several_retries(
                    event_details.clone(),
                    self.id.to_string(),
                    self.db_type().to_string(),
                    Some(CommandError::new_from_safe_message(format!(
                        "Timeout reached waiting for the database to be in {} state",
                        DB_READY_STATE
                    ))),
                )),
                // Error ;'(
                Err(Some((cmd_err, msg))) => Err(EngineError::new_database_failed_to_start_after_several_retries(
                    event_details.clone(),
                    self.id.to_string(),
                    self.db_type().to_string(),
                    Some(CommandError::new_from_legacy_command_error(cmd_err, Some(msg))),
                )),
            }
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

            // Elasticache does not support being stopped/paused
            if self.db_type() == service::DatabaseType::Redis {
                return Ok(());
            }

            // Terraform does not ensure that the database is correctly started
            // So we must force it ourselves in case
            let credentials = {
                let mut credentials = target.kubernetes.cloud_provider().credentials_environment_variables();
                credentials.push((AWS_DEFAULT_REGION, target.kubernetes.region()));
                credentials
            };
            // We use the fqdn_id as db identifier, why not id or name like everything else ¯\_(ツ)_/¯
            start_stop_managed_database(self.db_type(), &self.fqdn_id, &credentials, true).map_err(
                |(cmd_error, msg)| {
                    EngineError::new_cannot_pause_managed_database(
                        event_details.clone(),
                        CommandError::new_from_legacy_command_error(cmd_error, Some(msg)),
                    )
                },
            )?;

            let ret = await_db_state(
                Duration::from_secs(60 * 30),
                self.db_type(),
                &self.fqdn_id,
                &credentials,
                DB_STOPPED_STATE,
            );

            match ret {
                Ok(_) => Ok(()),
                // timeout
                Err(None) => Err(EngineError::new_cannot_pause_managed_database(
                    event_details.clone(),
                    CommandError::new_from_safe_message(format!(
                        "Timeout reached waiting for the database to be in {} state",
                        DB_STOPPED_STATE
                    )),
                )),
                // Error ;'(
                Err(Some((cmd_err, msg))) => Err(EngineError::new_cannot_pause_managed_database(
                    event_details.clone(),
                    CommandError::new_from_legacy_command_error(cmd_err, Some(msg)),
                )),
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
