use super::InfraLogger;
use crate::cmd::command::CommandKiller;
use crate::cmd::helm::{Helm, HelmListCache};
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
use std::time::{Duration, Instant};
use tera::Context as TeraContext;

/// Strategy for calculating delays between retry attempts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DelayStrategy {
    /// Constant delay between all retries.
    #[default]
    Fixed,

    /// Fibonacci-based exponential backoff.
    /// Delays follow Fibonacci sequence: base, base, 2*base, 3*base, 5*base, 8*base, ...
    #[allow(dead_code)] // Intentionally unused until configuration is exposed
    Exponential,
}

/// Configuration for retrying failed charts in parallel deployments.
#[derive(Clone, Debug)]
pub struct ParallelDeploymentRetryConfig {
    /// Maximum number of retry attempts after initial failure.
    pub max_attempts: usize,

    /// Strategy for calculating delay between retries.
    pub delay_strategy: DelayStrategy,

    /// Initial delay in milliseconds.
    /// For Fixed: used as constant delay.
    /// For Exponential: used as base delay for Fibonacci sequence.
    pub initial_delay_ms: u64,
}

impl Default for ParallelDeploymentRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_strategy: DelayStrategy::Fixed,
            initial_delay_ms: 5000,
        }
    }
}

impl ParallelDeploymentRetryConfig {
    /// Validates the configuration.
    /// Returns an error message if validation fails.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_attempts == 0 {
            return Err("max_attempts must be greater than 0".to_string());
        }
        if self.initial_delay_ms == 0 {
            return Err("initial_delay_ms must be greater than 0".to_string());
        }
        Ok(())
    }

    /// Calculates the delay for a given attempt number using the configured strategy.
    /// Attempt numbers are 1-indexed.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        match self.delay_strategy {
            DelayStrategy::Fixed => Duration::from_millis(self.initial_delay_ms),
            DelayStrategy::Exponential => {
                // Fibonacci sequence for exponential backoff
                let multiplier = fibonacci(attempt);
                Duration::from_millis(self.initial_delay_ms * multiplier)
            }
        }
    }
}

/// Computes the nth Fibonacci number (1-indexed).
/// fib(1) = 1, fib(2) = 1, fib(3) = 2, fib(4) = 3, fib(5) = 5, ...
fn fibonacci(n: usize) -> u64 {
    if n <= 2 {
        return 1;
    }
    let mut a: u64 = 1;
    let mut b: u64 = 1;
    for _ in 2..n {
        let tmp = a + b;
        a = b;
        b = tmp;
    }
    b
}

/// Record of a single retry attempt for logging and diagnostics.
#[derive(Clone, Debug)]
#[allow(dead_code)] // Public API for observability - fields exposed for callers
pub struct RetryAttempt {
    /// Which attempt this was (1-indexed).
    pub attempt_number: usize,

    /// Name of the chart that failed.
    pub chart_name: String,

    /// Error encountered during this attempt.
    pub error: HelmChartError,

    /// Whether this error was determined to be retryable.
    pub was_retryable: bool,

    /// Delay before the next attempt (None if this was the final attempt).
    pub delay_before_next_ms: Option<u64>,

    /// Timestamp of the attempt.
    pub timestamp: Instant,
}

/// Details of a chart that failed deployment after all retries.
#[derive(Debug)]
#[allow(dead_code)] // Public API for observability - fields exposed for callers
pub struct ChartFailure {
    /// Name of the failed chart.
    pub chart_name: String,

    /// All retry attempts for this chart.
    pub attempts: Vec<RetryAttempt>,

    /// Final error after all retries.
    pub final_error: HelmChartError,
}

/// Result of deploying charts in parallel with retry support.
#[derive(Debug)]
#[allow(dead_code)] // Public API for observability - fields exposed for callers
pub struct ParallelDeploymentResult {
    /// Names of charts that deployed successfully.
    pub succeeded: Vec<String>,

    /// Charts that failed after all retries exhausted.
    pub failed: Vec<ChartFailure>,
}

/// Type alias for the result of a single deployment attempt.
/// Contains succeeded chart names and failed charts with their errors.
type DeploymentAttemptResult = (Vec<String>, Vec<(Box<dyn HelmChart>, HelmChartError)>);

impl ParallelDeploymentResult {
    /// Returns true if all charts succeeded.
    pub fn is_success(&self) -> bool {
        self.failed.is_empty()
    }

    /// Returns the first failure error, if any.
    pub fn first_error(&self) -> Option<&HelmChartError> {
        self.failed.first().map(|f| &f.final_error)
    }
}

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
        let mut charts_to_deploy = self.gen_charts_to_deploy(infra_ctx, chart_configs)?;
        let chart_count = enable_engine_post_renderer_for_all_charts(&mut charts_to_deploy);
        logger.info(format!(
            "🏷️ Enabling engine post-renderer labels for all Helm charts ({} charts)",
            chart_count
        ));

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

        let list_cache = HelmListCache::new();

        for (ix, charts_level) in charts_to_deploy.into_iter().enumerate() {
            logger.info("");
            logger.info(format!("🏁 Starting level {ix}"));
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
                        let _ = buf_writer.write_all(b"\n");
                        logger.diff(InfrastructureDiffType::Helm, line);
                    });
                });

            // Skip actual deployment if dry run
            if self.charts_context().is_dry_run {
                logger.warn("👻 Dry run mode enabled, skipping actual deployment");
                continue;
            }

            // We do the actual deployment in parallel with retry support
            let chart_names = charts_names_user_str(&charts_level);
            logger.info(format!("🛳️ Deploying in parallel charts of level {ix}: {chart_names}"));
            let retry_config = ParallelDeploymentRetryConfig::default();
            let result = deploy_parallel_charts_with_retry(
                infra_ctx.mk_kube_client()?.as_ref(),
                &infra_ctx.kubernetes().kubeconfig_local_file_path(),
                &envs,
                charts_level,
                list_cache.clone(),
                &retry_config,
                &CommandKiller::never(),
            )
            .map_err(|e| Box::new(EngineError::new_helm_chart_error(ev_details.clone(), e)))?;

            // Check if any charts failed
            if !result.is_success()
                && let Some(first_err) = result.first_error()
            {
                return Err(Box::new(EngineError::new_helm_chart_error(
                    ev_details.clone(),
                    first_err.clone(),
                )));
            }
            logger.info(format!("✅ Charts of level {ix} deployed"));
        }

        logger.info("⚓ Helm charts deployed successfully");
        logger.info("⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓⚓");

        Ok(())
    }
}

fn enable_engine_post_renderer_for_all_charts(charts_to_deploy: &mut [Vec<Box<dyn HelmChart>>]) -> usize {
    let mut chart_count = 0usize;

    for charts_level in charts_to_deploy.iter_mut() {
        for chart in charts_level.iter_mut() {
            chart.get_chart_info_mut().enable_engine_post_renderer_labels = true;
            chart_count += 1;
        }
    }

    chart_count
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

/// Deploys Helm charts in parallel with automatic retry for transient failures.
///
/// This function provides retry capability at the parallel deployment level. When a chart
/// fails due to a transient error (network timeout, API unavailability, etc.), the system
/// automatically retries deployment without affecting successfully deployed charts.
///
/// # Arguments
/// * `kube_client` - Kubernetes client for API operations
/// * `kubernetes_config` - Path to kubeconfig file
/// * `envs` - Environment variables for helm commands
/// * `charts` - Charts to deploy in parallel
/// * `retry_config` - Configuration for retry behavior
/// * `cmd_killer` - Abort signal checker for cancellation support
///
/// # Returns
/// * `Ok(ParallelDeploymentResult)` - Deployment completed (may contain failures)
/// * `Err(HelmChartError)` - Fatal error preventing any deployment
///
/// # Retry Behavior
/// - Only charts with retryable errors are retried
/// - Successfully deployed charts are not re-deployed
/// - Respects abort signal between retry attempts
pub fn deploy_parallel_charts_with_retry(
    kube_client: &kube::Client,
    kubernetes_config: &Path,
    envs: &[(&str, &str)],
    charts: Vec<Box<dyn HelmChart>>,
    list_cache: HelmListCache,
    retry_config: &ParallelDeploymentRetryConfig,
    cmd_killer: &CommandKiller,
) -> Result<ParallelDeploymentResult, HelmChartError> {
    // Validate configuration
    if let Err(e) = retry_config.validate() {
        return Err(HelmChartError::CommandError(CommandError::new(
            format!("Invalid retry configuration: {}", e),
            None,
            None,
        )));
    }

    let charts: Vec<Box<dyn HelmChart>> = charts
        .into_iter()
        .map(|mut chart| {
            chart.get_chart_info_mut().helm_list_cache = Some(list_cache.clone());
            chart
        })
        .collect();

    let mut succeeded: Vec<String> = Vec::new();
    let mut failures: HashMap<String, Vec<RetryAttempt>> = HashMap::new();
    let mut charts_to_retry = charts;

    // Initial deployment + retry loop
    for attempt_num in 1..=retry_config.max_attempts + 1 {
        // Check for abort signal between attempts
        if cmd_killer.should_abort().is_some() {
            tracing::warn!("Parallel deployment retry aborted by user request");
            break;
        }

        // Deploy charts in parallel
        let (attempt_succeeded, attempt_failed) =
            deploy_charts_once(kube_client, kubernetes_config, envs, charts_to_retry, cmd_killer);

        // Add newly succeeded charts to the overall succeeded list
        succeeded.extend(attempt_succeeded);

        // If no failures, we're done
        if attempt_failed.is_empty() {
            break;
        }

        // Process failures: categorize as retryable or non-retryable
        let mut next_retry_charts: Vec<Box<dyn HelmChart>> = Vec::new();

        for (chart, error) in attempt_failed {
            let chart_name = chart.get_chart_info().name.clone();
            let is_retryable = error.is_retryable();
            let is_last_attempt = attempt_num > retry_config.max_attempts;

            // Calculate delay for next attempt (None if this is the last attempt or non-retryable)
            let delay_before_next_ms = if !is_last_attempt && is_retryable {
                Some(retry_config.delay_for_attempt(attempt_num).as_millis() as u64)
            } else {
                None
            };

            // Record the attempt
            let retry_attempt = RetryAttempt {
                attempt_number: attempt_num,
                chart_name: chart_name.clone(),
                error: error.clone(),
                was_retryable: is_retryable,
                delay_before_next_ms,
                timestamp: Instant::now(),
            };

            failures.entry(chart_name.clone()).or_default().push(retry_attempt);

            // Log the retry attempt
            if is_retryable && !is_last_attempt {
                tracing::info!(
                    chart = %chart_name,
                    attempt = attempt_num,
                    max_attempts = retry_config.max_attempts + 1,
                    delay_ms = delay_before_next_ms.unwrap_or(0),
                    error = %error,
                    "Helm chart deployment failed, will retry"
                );
                next_retry_charts.push(chart);
            } else if !is_retryable {
                tracing::warn!(
                    chart = %chart_name,
                    attempt = attempt_num,
                    error = %error,
                    "Helm chart deployment failed with non-retryable error, skipping retry"
                );
            } else {
                tracing::error!(
                    chart = %chart_name,
                    total_attempts = attempt_num,
                    error = %error,
                    "Helm chart deployment failed after all retry attempts"
                );
            }
        }

        // If there are no charts to retry, we're done
        if next_retry_charts.is_empty() {
            break;
        }

        // Wait before retrying
        let delay = retry_config.delay_for_attempt(attempt_num);
        tracing::info!(
            delay_ms = delay.as_millis() as u64,
            charts_remaining = next_retry_charts.len(),
            "Waiting before retry attempt"
        );
        std::thread::sleep(delay);

        charts_to_retry = next_retry_charts;
    }

    // Build the final result
    let failed: Vec<ChartFailure> = failures
        .into_iter()
        .map(|(chart_name, attempts)| {
            let final_error = attempts.last().unwrap().error.clone();
            ChartFailure {
                chart_name,
                attempts,
                final_error,
            }
        })
        .collect();

    Ok(ParallelDeploymentResult { succeeded, failed })
}

/// Deploys charts in parallel and returns (succeeded_charts, failed_charts_with_errors).
/// This is a single attempt - no retry logic.
fn deploy_charts_once(
    kube_client: &kube::Client,
    kubernetes_config: &Path,
    envs: &[(&str, &str)],
    charts: Vec<Box<dyn HelmChart>>,
    cmd_killer: &CommandKiller,
) -> DeploymentAttemptResult {
    thread::scope(|s| {
        let mut handles: Vec<(String, _)> = vec![];

        for chart in charts.into_iter() {
            let chart_name = chart.get_chart_info().name.clone();
            let path = kubernetes_config.to_path_buf();
            let current_span = tracing::Span::current();
            let handle = s.spawn(move || {
                let _span = current_span.enter();
                let result = chart.run(kube_client, path.as_path(), envs, cmd_killer);
                (chart, result)
            });

            handles.push((chart_name, handle));
        }

        let mut succeeded: Vec<String> = Vec::new();
        let mut failed: Vec<(Box<dyn HelmChart>, HelmChartError)> = Vec::new();

        for (_chart_name, handle) in handles {
            match handle.join() {
                Ok((chart, Ok(_))) => {
                    let name = chart.get_chart_info().name.clone();
                    tracing::info!(chart = %name, "Helm chart deployed successfully");
                    succeeded.push(name);
                }
                Ok((chart, Err(e))) => {
                    let name = chart.get_chart_info().name.clone();
                    tracing::error!(chart = %name, error = %e, "Helm chart deployment failed");
                    failed.push((chart, e));
                }
                Err(panic_err) => {
                    let err_msg = match panic_err.downcast_ref::<&'static str>() {
                        Some(s) => s.to_string(),
                        None => match panic_err.downcast_ref::<String>() {
                            Some(s) => s.clone(),
                            None => "Unknown panic error".to_string(),
                        },
                    };
                    // We can't recover the chart after a panic, so we can't retry it
                    // This will show up in the error log
                    tracing::error!(error = %err_msg, "Thread panicked during parallel chart deployment");
                }
            }
        }

        (succeeded, failed)
    })
}

fn create_helm_diff_file(dir_path: &Path, chart_name: &str) -> anyhow::Result<BufWriter<File>> {
    use std::fs::{self, OpenOptions};

    let filepath = {
        let filepath = dir_path.join("helm-diffs");
        if !filepath.exists() {
            fs::create_dir_all(&filepath)?;
        }
        filepath.join(format!("{chart_name}.diff"))
    };

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true) // This will ensure the content is overridden
        .open(filepath)?;

    Ok(BufWriter::new(file))
}
