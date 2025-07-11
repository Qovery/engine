use crate::cmd;
use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::constants::AWS_DEFAULT_REGION;
use crate::environment::action::DeploymentAction;
use crate::environment::action::check_dns::CheckDnsForDomains;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::deploy_terraform::TerraformDeployment;
use crate::environment::action::pause_service::PauseServiceAction;
use crate::environment::models::database::{
    Container, Database, DatabaseError, DatabaseService, DatabaseType, Managed, get_database_with_invalid_storage_size,
};
use crate::environment::models::types::{CloudProvider, ToTeraContext, VersionsNumber};
use crate::environment::report::database::reporter::DatabaseDeploymentReporter;
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::{CommandError, EngineError, Tag};
use crate::events::{EnvironmentStep, EventDetails, Stage};
use crate::helm::{ChartInfo, ChartSetValue, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::service::{Action, Service, get_database_terraform_config};
use crate::infrastructure::models::cloud_provider::{DeploymentTarget, service};
use crate::kubers_utils::{KubeDeleteMode, kube_delete_all_from_selector};
use crate::runtime::block_on;
use crate::services::aws::models::QoveryAwsSdkConfigManagedDatabase;
use aws_types::SdkConfig;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use semver::Version;
use serde::Deserialize;
use std::collections::BTreeMap;

use super::utils::{are_pvcs_bound, delete_nlb_or_alb_service, update_pvcs};
use crate::environment::action::restart_service::RestartServiceAction;
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::infrastructure::models::cloud_provider;
use crate::infrastructure::models::kubernetes::Kind;
use async_trait::async_trait;
use aws_sdk_docdb::operation::describe_db_clusters::{DescribeDBClustersError, DescribeDbClustersOutput};
use aws_sdk_elasticache::operation::describe_cache_clusters::{
    DescribeCacheClustersError, DescribeCacheClustersOutput,
};
use aws_sdk_rds::error::SdkError;
use aws_sdk_rds::operation::describe_db_instances::{DescribeDBInstancesError, DescribeDbInstancesOutput};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const DB_READY_STATE: &str = "available";
const DB_STOPPED_STATE: &str = "stopped";

#[derive(Deserialize, Default)]
struct CacheCluster {
    #[serde(alias = "CacheClusterId")]
    pub cache_cluster_id: String,
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
        service::DatabaseType::Redis => {
            let redis_cache_cluster_id = find_redis_cache_cluster_id(db_id, credentials)?;
            if redis_cache_cluster_id.is_empty() {
                return Ok("".to_string());
            }
            QoveryCommand::new(
                "aws",
                &[
                    "elasticache",
                    "describe-cache-clusters",
                    "--cache-cluster-id",
                    &redis_cache_cluster_id,
                ],
                credentials,
            )
        }
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

/// We can have different cache_cluster_id patterns according to managed redis version:
/// - v5: "z${db_id}"
/// - v6 created before 2022-21-07: "z${db_id}"
/// - v6 created after 2022-21-07: "z${db_id}-001"
///
/// So we need to get the correct cache_cluster_id by filtering every cache cluster containing the
/// text "db_id"
fn find_redis_cache_cluster_id(
    db_id: &str,
    credentials: &[(&str, &str)],
) -> Result<String, (cmd::command::CommandError, String)> {
    let mut describe_cache_clusters_command =
        QoveryCommand::new("aws", &["elasticache", "describe-cache-clusters"], credentials);

    let mut output_stdout: Vec<String> = vec![];
    let mut output_stderr: Vec<String> = vec![];
    let cache_clusters_result = describe_cache_clusters_command
        .exec_with_output(&mut |line| output_stdout.push(line), &mut |line| output_stderr.push(line));

    if let Err(cmd_error) = cache_clusters_result {
        output_stdout.extend(output_stderr);
        return Err((cmd_error, output_stdout.join("\n").trim().to_string()));
    }

    let cache_clusters: CacheClustersResponse =
        serde_json::from_str(output_stdout.join("").as_str()).unwrap_or_default();
    let cache_cluster_id_or_default = cache_clusters
        .cache_clusters
        .into_iter()
        .find(|it| it.cache_cluster_id.contains(db_id))
        .map(|it| it.cache_cluster_id)
        // if no cache_cluster is found, return default will indicate to retry until timeout
        // is reached in parent method
        .unwrap_or_default();

    Ok(cache_cluster_id_or_default)
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
                    &format!("{action}-db-instance"),
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
                    &format!("{action}-db-cluster"),
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

fn on_create_managed_impl<C: CloudProvider, T: DatabaseType<C, Managed>>(
    db: &Database<C, Managed, T>,
    logger: &EnvProgressLogger,
    event_details: EventDetails,
    target: &DeploymentTarget,
) -> Result<(), Box<EngineError>>
where
    Database<C, Managed, T>: DatabaseService,
{
    let workspace_dir = db.workspace_directory();
    let tera_context = db.to_tera_context(target)?;

    // Execute terraform to provision database on cloud provider side
    let terraform_deploy = TerraformDeployment::new(
        tera_context.clone(),
        PathBuf::from(db.terraform_common_resource_dir_path()),
        PathBuf::from(db.terraform_resource_dir_path()),
        PathBuf::from(&workspace_dir),
        event_details.clone(),
        target.is_dry_run_deploy,
    );
    terraform_deploy.on_create(target)?;

    // Our terraform give us back a file with all the info we need to deploy the remaining stuff
    let database_config =
        get_database_terraform_config(format!("{}/database-tf-config.json", &workspace_dir,).as_str())
            .map_err(|err| EngineError::new_terraform_error(event_details.clone(), err))?;

    // Sending hostname to the core to update env variable with real hostname
    // useful when managed service requires TLS and using a CNAME is not possible due to certificate checks
    {
        let mut json: BTreeMap<&str, &str> = BTreeMap::new();
        json.insert("hostname", database_config.target_hostname.as_str());
        logger.core_configuration_for_database(
            format!(
                "🪡 Retrieved database hostname {}, environment variables are going to be stitched with it",
                database_config.target_hostname
            ),
            serde_json::to_string(&json).unwrap_or_default(),
        );
    }

    // Deploy the external service name
    let values = vec![
        ChartSetValue {
            key: "target_hostname".to_string(),
            value: database_config.target_hostname,
        },
        ChartSetValue {
            key: "source_fqdn".to_string(),
            value: database_config.target_fqdn,
        },
        ChartSetValue {
            key: "database_id".to_string(), // here we use the id and not the fqdn_id ¯\_(ツ)_/¯
            value: db.id().to_string(),
        },
        ChartSetValue {
            key: "database_long_id".to_string(),
            value: db.long_id().to_string(),
        },
        ChartSetValue {
            key: "environment_id".to_string(),
            value: target.environment.id.to_string(),
        },
        ChartSetValue {
            key: "environment_long_id".to_string(),
            value: target.environment.long_id.to_string(),
        },
        ChartSetValue {
            key: "project_long_id".to_string(),
            value: target.environment.project_long_id.to_string(),
        },
        ChartSetValue {
            key: "service_name".to_string(),
            value: database_config.target_fqdn_id,
        },
        ChartSetValue {
            key: "publicly_accessible".to_string(),
            value: db.publicly_accessible.to_string(),
        },
    ];

    let chart = ChartInfo {
        name: format!("{}-externalname", db.fqdn_id), // here it is the fqdn id :O
        path: format!("{}/{}", &workspace_dir, "service-chart"),
        namespace: HelmChartNamespaces::Custom,
        custom_namespace: Some(target.environment.namespace().to_string()),
        values,
        ..Default::default()
    };

    let helm = HelmDeployment::new(
        event_details.clone(),
        tera_context,
        PathBuf::from(db.helm_chart_external_name_service_dir()),
        None,
        chart,
    );

    helm.on_create(target)?;

    // We don't manage START/PAUSE for managed database elsewhere than for AWS
    if target.cloud_provider.kind() != cloud_provider::Kind::Aws {
        return Ok(());
    }

    // Terraform does not ensure that the database is correctly started
    // So we must force it ourselves in case
    let credentials = {
        let mut credentials = target.cloud_provider.credentials_environment_variables();
        credentials.push((AWS_DEFAULT_REGION, target.kubernetes.region()));
        credentials
    };

    // If the database is not in the available state, try to start it
    match get_managed_database_status(db.db_type(), &db.fqdn_id, &credentials) {
        Ok(status) if status == DB_READY_STATE => {}
        Ok(_) | Err(_) => {
            let _ = start_stop_managed_database(db.db_type(), &db.fqdn_id, &credentials, false);
        }
    }

    let ret = await_db_state(
        Duration::from_secs(60 * 30),
        db.db_type(),
        &db.fqdn_id,
        &credentials,
        DB_READY_STATE,
    );

    match ret {
        Ok(_) => Ok(()),
        // timeout
        Err(None) => Err(Box::new(EngineError::new_database_failed_to_start_after_several_retries(
            event_details,
            db.id.to_string(),
            db.db_type().to_string(),
            Some(CommandError::new_from_safe_message(format!(
                "Timeout reached waiting for the database to be in {DB_READY_STATE} state"
            ))),
        ))),
        // Error ;'(
        Err(Some((cmd_err, msg))) => Err(Box::new(EngineError::new_database_failed_to_start_after_several_retries(
            event_details,
            db.id.to_string(),
            db.db_type().to_string(),
            Some(CommandError::new_from_legacy_command_error(cmd_err, Some(msg))),
        ))),
    }
}

#[async_trait]
impl QoveryAwsSdkConfigManagedDatabase for SdkConfig {
    async fn find_managed_rds_database(
        &self,
        db_id: &str,
    ) -> Result<DescribeDbInstancesOutput, SdkError<DescribeDBInstancesError>> {
        let client = aws_sdk_rds::Client::new(self);
        client
            .describe_db_instances()
            .db_instance_identifier(db_id)
            .send()
            .await
    }

    async fn find_managed_elasticache_database(
        &self,
        db_id: &str,
    ) -> Result<DescribeCacheClustersOutput, SdkError<DescribeCacheClustersError>> {
        let client = aws_sdk_elasticache::Client::new(self);
        client.describe_cache_clusters().cache_cluster_id(db_id).send().await
    }

    async fn find_managed_doc_db_database(
        &self,
        db_id: &str,
    ) -> Result<DescribeDbClustersOutput, SdkError<DescribeDBClustersError>> {
        let client = aws_sdk_docdb::Client::new(self);
        client.describe_db_clusters().db_cluster_identifier(db_id).send().await
    }
}

fn managed_database_exists(
    db_type: service::DatabaseType,
    db_id: &str,
    db_version: &VersionsNumber,
    event_details: &EventDetails,
    sdk_config: Option<SdkConfig>,
) -> Result<bool, Box<EngineError>> {
    let aws_conn = match sdk_config {
        Some(x) => x,
        None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details.clone()))),
    };

    match db_type {
        service::DatabaseType::PostgreSQL | service::DatabaseType::MySQL => {
            let result = match block_on(aws_conn.find_managed_rds_database(db_id)) {
                Ok(result) => result,
                Err(e) => {
                    let err = e.to_string();
                    return match DatabaseError::from_rds_sdk_error(e, db_type, db_id.to_string()) {
                        DatabaseError::DatabaseNotFound {
                            database_type: _,
                            database_id: _,
                        } => Ok(false),
                        _ => Err(Box::new(EngineError::new_aws_sdk_cannot_list_elasticache_clusters(
                            event_details.clone(),
                            err,
                            Some(db_id),
                        ))),
                    };
                }
            };

            match result.db_instances().is_empty() {
                true => Ok(false),
                false => Ok(true),
            }
        }
        service::DatabaseType::MongoDB => {
            let result = match block_on(aws_conn.find_managed_doc_db_database(db_id)) {
                Ok(result) => result,
                Err(e) => {
                    let err = e.to_string();
                    return match DatabaseError::from_documentdb_sdk_error(e, db_type, db_id.to_string()) {
                        DatabaseError::DatabaseNotFound {
                            database_type: _,
                            database_id: _,
                        } => Ok(false),
                        _ => Err(Box::new(EngineError::new_aws_sdk_cannot_list_elasticache_clusters(
                            event_details.clone(),
                            err,
                            Some(db_id),
                        ))),
                    };
                }
            };

            match result.db_clusters().is_empty() {
                true => Ok(false),
                false => Ok(true),
            }
        }
        service::DatabaseType::Redis => {
            // Redis cluster append a suffix -001 to the db_id
            let db_id = if db_version.major != "5" {
                format!("{db_id}-001")
            } else {
                db_id.to_string()
            };

            let result = match block_on(aws_conn.find_managed_elasticache_database(&db_id)) {
                Ok(result) => result,
                Err(e) => {
                    let err = e.to_string();
                    return match DatabaseError::from_elasticache_sdk_error(e, db_type, db_id.to_string()) {
                        DatabaseError::DatabaseNotFound {
                            database_type: _,
                            database_id: _,
                        } => Ok(false),
                        _ => Err(Box::new(EngineError::new_aws_sdk_cannot_list_elasticache_clusters(
                            event_details.clone(),
                            err,
                            Some(&db_id),
                        ))),
                    };
                }
            };
            match result.cache_clusters().is_empty() {
                true => Ok(false),
                false => Ok(true),
            }
        }
    }
}

// For Managed database
impl<C: CloudProvider, T: DatabaseType<C, Managed>> DeploymentAction for Database<C, Managed, T>
where
    Database<C, Managed, T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let pre_run = |_: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };
        let run = |logger: &EnvProgressLogger, _: ()| -> Result<(), Box<EngineError>> {
            on_create_managed_impl(self, logger, event_details.clone(), target)
        };
        let post_run = |logger: &EnvSuccessLogger, _: ()| {
            if self.publicly_accessible {
                let domain_checker = CheckDnsForDomains {
                    resolve_to_ip: vec![self.fqdn.to_string()],
                    resolve_to_cname: vec![],
                    log: Box::new(move |msg| logger.send_success(msg)),
                };

                let _ = domain_checker.on_create(target);
                logger.send_success(format!("✅ {} of DNS for managed database succeeded", self.action));
            }
        };

        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Create),
            DeploymentTaskImpl {
                pre_run: &pre_run,
                run: &run,
                post_run_success: &post_run,
            },
        )
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Pause),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                // We don't manage PAUSE for managed database elsewhere than for AWS
                if target.cloud_provider.kind() != cloud_provider::Kind::Aws {
                    return Ok(());
                }

                // Elasticache does not support being stopped/paused
                if self.db_type() == service::DatabaseType::Redis {
                    return Ok(());
                }

                // Terraform does not ensure that the database is correctly started
                // So we must force it ourselves in case
                let credentials = {
                    let mut credentials = target.cloud_provider.credentials_environment_variables();
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
                    Err(None) => Err(Box::new(EngineError::new_cannot_pause_managed_database(
                        event_details.clone(),
                        CommandError::new_from_safe_message(format!(
                            "Timeout reached waiting for the database to be in {DB_STOPPED_STATE} state"
                        )),
                    ))),
                    // Error ;'(
                    Err(Some((cmd_err, msg))) => Err(Box::new(EngineError::new_cannot_pause_managed_database(
                        event_details.clone(),
                        CommandError::new_from_legacy_command_error(cmd_err, Some(msg)),
                    ))),
                }
            },
        )
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Delete),
            |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                // First we must ensure the DB is created by looking for the k8s ExternalName service and AWS side
                if !managed_database_exists(
                    self.db_type(),
                    &self.fqdn_id,
                    &self.version,
                    &event_details,
                    target
                        .cloud_provider
                        .downcast_ref()
                        .as_aws()
                        .map(|x| x.aws_sdk_client()),
                )? {
                    // if db has never been deployed. No need to go further
                    info!("Managed database not found on cloud provider. Assuming it does not exist");
                    return Ok(());
                }

                // Ensure it's in a ready state
                // because if not, the deletion is going to fail (i.e: cannot snapshot paused db)
                on_create_managed_impl(self, logger, event_details.clone(), target)?;

                let workspace_dir = self.workspace_directory();

                // Ok now delete it
                let terraform_deploy = TerraformDeployment::new(
                    self.to_tera_context(target)?,
                    PathBuf::from(self.terraform_common_resource_dir_path()),
                    PathBuf::from(self.terraform_resource_dir_path()),
                    PathBuf::from(&workspace_dir),
                    event_details.clone(),
                    target.is_dry_run_deploy,
                );
                terraform_deploy.on_delete(target)?;

                // Delete the service attached
                let chart = ChartInfo {
                    name: format!("{}-externalname", self.fqdn_id), // here it is the fqdn id :O
                    path: format!("{}/{}", &workspace_dir, "service-chart"),
                    namespace: HelmChartNamespaces::Custom,
                    custom_namespace: Some(target.environment.namespace().to_string()),
                    action: HelmAction::Destroy,
                    ..Default::default()
                };
                let helm = HelmDeployment::new(
                    event_details.clone(),
                    tera::Context::default(),
                    PathBuf::from(self.helm_chart_external_name_service_dir()),
                    None,
                    chart,
                );

                helm.on_delete(target)
            },
        )
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let command_error = CommandError::new_from_safe_message("Cannot restart Managed Database".to_string());
        Err(Box::new(EngineError::new_cannot_restart_service(
            self.get_event_details(Stage::Environment(EnvironmentStep::Restart)),
            target.environment.namespace(),
            &self.kube_label_selector(),
            command_error,
        )))
    }
}

// For Container database
impl<C: CloudProvider, T: DatabaseType<C, Container>> DeploymentAction for Database<C, Container, T>
where
    Database<C, Container, T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let pre_run = |_: &EnvProgressLogger| -> Result<(), Box<EngineError>> { Ok(()) };
        let run = |logger: &EnvProgressLogger, _: ()| -> Result<(), Box<EngineError>> {
            match get_database_with_invalid_storage_size(
                self,
                &target.kube,
                target.environment.namespace(),
                &event_details,
            ) {
                Ok(invalid_statefulset_storage) => {
                    if let Some(invalid_statefulset_storage) = invalid_statefulset_storage {
                        update_pvcs(
                            self.as_service(),
                            &invalid_statefulset_storage,
                            target.environment.namespace(),
                            &event_details,
                            &target.kube,
                        )?;
                    }
                }
                Err(e) => logger.warning(format!("invalid_statefulset_storage fail with error: {e}")),
            }

            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
                k8s_selector: Some(self.kube_label_selector()),
                values_files: vec![format!("{}/qovery-values.yaml", self.workspace_directory())],
                // need to perform reinstall (but keep PVC) to update the statefulset
                reinstall_chart_if_installed_version_is_below_than: match T::db_type() {
                    service::DatabaseType::PostgreSQL => Some(Version::new(12, 5, 1)),
                    service::DatabaseType::MongoDB => Some(Version::new(13, 13, 1)),
                    service::DatabaseType::MySQL => Some(Version::new(9, 10, 1)),
                    service::DatabaseType::Redis => Some(Version::new(17, 11, 4)),
                },
                ..Default::default()
            };
            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                Some(PathBuf::from(format!("{}/qovery-values.j2.yaml", self.helm_chart_values_dir()))),
                chart,
            );

            if target.kubernetes.kind() == Kind::Eks {
                delete_nlb_or_alb_service(
                    target.kube.clone(),
                    target.environment.namespace(),
                    format!("qovery.com/service-id={}", self.long_id()).as_str(),
                    target.kubernetes.advanced_settings().aws_eks_enable_alb_controller,
                    event_details.clone(),
                )?;
            }

            if let Err(e) = helm.on_create(target) {
                return match e.tag() {
                    Tag::TaskCancellationRequested => Err(e),
                    _ => match are_pvcs_bound(
                        self.as_service(),
                        target.environment.namespace(),
                        &event_details,
                        &target.kube,
                    ) {
                        Ok(_) => Err(e),
                        Err(err) => {
                            logger.warning(format!("Cannot find if pvc bound: {}", err.user_log_message()));
                            Err(e)
                        }
                    },
                };
            };

            Ok(())
        };

        let post_run = |logger: &EnvSuccessLogger, _: ()| {
            // check non custom domains
            if self.publicly_accessible {
                let domain_checker = CheckDnsForDomains {
                    resolve_to_ip: vec![self.fqdn.to_string()],
                    resolve_to_cname: vec![],
                    log: Box::new(move |msg| logger.send_success(msg)),
                };

                let _ = domain_checker.on_create(target);
                logger.send_success(format!("✅ {} of DNS for container database succeeded", self.action));
            }
        };

        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Create),
            DeploymentTaskImpl {
                pre_run: &pre_run,
                run: &run,
                post_run_success: &post_run,
            },
        )
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Pause),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                let pause_service = PauseServiceAction::new(
                    self.kube_label_selector(),
                    true,
                    Duration::from_secs(5 * 60),
                    self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                    true,
                );
                pause_service.on_pause(target)
            },
        )
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Delete),
            |logger: &EnvProgressLogger| {
                let chart = ChartInfo {
                    name: self.helm_release_name(),
                    action: HelmAction::Destroy,
                    namespace: HelmChartNamespaces::Custom,
                    custom_namespace: Some(target.environment.namespace().to_string()),
                    k8s_selector: Some(self.kube_label_selector()),
                    ..Default::default()
                };
                let helm = HelmDeployment::new(
                    event_details.clone(),
                    self.to_tera_context(target)?,
                    PathBuf::from(self.helm_chart_dir()),
                    None,
                    chart,
                );

                helm.on_delete(target)?;

                // FIXME(ENG-1606): Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
                logger.info("🪓 Terminating network volume of the database".to_string());
                if let Err(err) = block_on(kube_delete_all_from_selector::<PersistentVolumeClaim>(
                    &target.kube,
                    &format!("app={}", self.kube_name()), //FIXME: legacy labels ;(
                    target.environment.namespace(),
                    KubeDeleteMode::Normal,
                )) {
                    return Err(Box::new(EngineError::new_k8s_cannot_delete_pvcs(
                        event_details.clone(),
                        self.kube_label_selector(),
                        CommandError::new_from_safe_message(err.to_string()),
                    )));
                }

                Ok(())
            },
        )
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            DatabaseDeploymentReporter::new(self, target, Action::Restart),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                let restart_service = RestartServiceAction::new(
                    self.kube_label_selector(),
                    true,
                    self.get_event_details(Stage::Environment(EnvironmentStep::Restart)),
                );
                restart_service.on_restart(target)
            },
        )
    }
}
