mod config;
mod docker;
#[cfg(test)]
mod fixtures;
mod model;

use self::config::{
    DiscoveryError, OperatorIdentity, PREVIOUS_CONTAINER_LABEL, RESTART_POLICY_LABEL, SupportedOperator, UPDATE_LABEL,
    check_supported_profile, find_operator,
};
use self::docker::{DockerCli, DockerFailure, OperatorDocker};
use self::model::{
    ContainerId, ContainerInspect, ContainerName, ContainerStatus, ImageLookup, ImageReference, OperatorVersion,
    PinnedImage, Platform, RestartPolicy,
};
use super::platform_components::{
    LEGACY_REQUEST_SCHEMA_VERSION, PREFLIGHT_REQUEST_SCHEMA_VERSION, result_schema_version_for,
    write_platform_execution_result,
};
use super::{InfraLogger, InfraLoggerImpl};
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::io_models::Action;
use crate::io_models::context::Metadata;
use crate::io_models::engine_request::InfrastructureEngineRequest;
use crate::io_models::platform_components::{PlatformUnitErrorCode, PlatformUnitResult};
use crate::logger::Logger;
use std::fmt::{Display, Formatter};
use std::slice;
use std::thread;
use std::time::{Duration, Instant};
use url::Url;

const OPERATOR_UNIT_KEY: &str = "qovery-operator";

#[derive(Clone, Copy, Debug)]
struct HealthCheck {
    window: Duration,
    poll_interval: Duration,
}

impl HealthCheck {
    const PRODUCTION: HealthCheck = HealthCheck {
        window: Duration::from_secs(60),
        poll_interval: Duration::from_secs(2),
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UpdateFailureKind {
    DockerUnavailable,
    OperatorNotFound,
    OperatorAmbiguous,
    UnsupportedConfiguration,
    ImagePullFailed,
    ImagePlatformMismatch,
    DockerOperationFailed,
    RolledBack,
    RollbackFailed,
}

impl From<UpdateFailureKind> for PlatformUnitErrorCode {
    fn from(kind: UpdateFailureKind) -> Self {
        match kind {
            UpdateFailureKind::DockerUnavailable => PlatformUnitErrorCode::DockerUnavailable,
            UpdateFailureKind::OperatorNotFound => PlatformUnitErrorCode::OperatorNotFound,
            UpdateFailureKind::OperatorAmbiguous => PlatformUnitErrorCode::OperatorAmbiguous,
            UpdateFailureKind::UnsupportedConfiguration => PlatformUnitErrorCode::UnsupportedConfiguration,
            UpdateFailureKind::ImagePullFailed => PlatformUnitErrorCode::ImagePullFailed,
            UpdateFailureKind::ImagePlatformMismatch => PlatformUnitErrorCode::ImagePlatformMismatch,
            UpdateFailureKind::DockerOperationFailed => PlatformUnitErrorCode::DockerOperationFailed,
            UpdateFailureKind::RolledBack => PlatformUnitErrorCode::RolledBack,
            UpdateFailureKind::RollbackFailed => PlatformUnitErrorCode::RollbackFailed,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct UpdateFailure {
    kind: UpdateFailureKind,
    message: String,
}

impl UpdateFailure {
    fn new(kind: UpdateFailureKind, message: impl Into<String>) -> Self {
        UpdateFailure {
            kind,
            message: message.into(),
        }
    }

    fn into_engine_error(self, event_details: EventDetails, image: &ImageReference) -> EngineError {
        match self.kind {
            UpdateFailureKind::DockerUnavailable | UpdateFailureKind::DockerOperationFailed => {
                EngineError::new_operator_container_docker_failure(event_details, self.message)
            }
            UpdateFailureKind::ImagePullFailed => EngineError::new_docker_pull_image_error(
                event_details,
                image.as_str().to_string(),
                image.repository().to_string(),
                CommandError::new_from_safe_message(self.message),
            ),
            UpdateFailureKind::OperatorNotFound
            | UpdateFailureKind::OperatorAmbiguous
            | UpdateFailureKind::UnsupportedConfiguration
            | UpdateFailureKind::ImagePlatformMismatch
            | UpdateFailureKind::RolledBack
            | UpdateFailureKind::RollbackFailed => {
                EngineError::new_operator_container_update_failed(event_details, self.message)
            }
        }
    }
}

impl From<DockerFailure> for UpdateFailure {
    fn from(failure: DockerFailure) -> Self {
        UpdateFailure::new(UpdateFailureKind::DockerOperationFailed, failure.to_string())
    }
}

#[derive(Debug)]
struct UpdatePlan<'a> {
    identity: OperatorIdentity,
    image: ImageReference,
    operator_version: OperatorVersion,
    execution_id: &'a str,
    health: HealthCheck,
}

struct UpdateRequest<'a> {
    execution_id: &'a str,
    action: Action,
    metadata: Option<&'a Metadata>,
    identity: OperatorIdentity,
    event_details: EventDetails,
}

/// Replaces the Docker Operator container of the request's cluster and reports the unit result.
pub(crate) fn update_operator_container(
    request: &InfrastructureEngineRequest,
    docker_socket: Option<&Url>,
    logger: Box<dyn Logger>,
) -> Result<(), Box<EngineError>> {
    execute(
        &DockerCli::new(docker_socket),
        UpdateRequest {
            execution_id: &request.id,
            action: request.action,
            metadata: request.metadata.as_ref(),
            identity: OperatorIdentity {
                cluster_id: request.kubernetes.long_id,
            },
            event_details: request.event_details(),
        },
        logger,
        HealthCheck::PRODUCTION,
    )
}

fn execute<D: OperatorDocker>(
    docker: &D,
    request: UpdateRequest<'_>,
    logger: Box<dyn Logger>,
    health: HealthCheck,
) -> Result<(), Box<EngineError>> {
    let event_details =
        EventDetails::clone_changing_stage(request.event_details, Infrastructure(InfrastructureStep::Create));
    let step_logger = InfraLoggerImpl {
        event_details: event_details.clone(),
        logger: logger.clone_dyn(),
    };
    let result_logger = InfraLoggerImpl {
        event_details: EventDetails::clone_changing_stage(
            event_details.clone(),
            Infrastructure(InfrastructureStep::PlatformExecutionResult),
        ),
        logger,
    };
    let schema_version = request
        .metadata
        .and_then(|metadata| metadata.engine_v2_options.as_ref())
        .map(|options| options.schema_version.as_str());
    let report = |unit: PlatformUnitResult| {
        write_platform_execution_result(
            &step_logger,
            &result_logger,
            result_schema_version_for(schema_version),
            request.execution_id,
            vec![unit],
            None,
        )
    };

    let (unit_key, target) = match validate_request(request.action, request.metadata) {
        Ok(validated) => validated,
        Err(violation) => {
            report(PlatformUnitResult::failed(
                violation.unit_key,
                PlatformUnitErrorCode::InvalidPayload,
                &violation.message,
            ));
            return Err(Box::new(EngineError::new_invalid_engine_payload(
                event_details,
                &violation.message,
                None,
            )));
        }
    };
    let plan = UpdatePlan {
        identity: request.identity,
        image: target.image,
        operator_version: target.operator_version,
        execution_id: request.execution_id,
        health,
    };

    match run_update(docker, &step_logger, &plan) {
        Ok(pinned_image) => {
            report(PlatformUnitResult::succeeded_with_message(unit_key, pinned_image.as_str()));
            Ok(())
        }
        Err(failure) => {
            report(PlatformUnitResult::failed(unit_key, failure.kind.into(), &failure.message));
            Err(Box::new(failure.into_engine_error(event_details, &plan.image)))
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct RequestViolation<'a> {
    unit_key: &'a str,
    message: String,
}

#[derive(Debug, Eq, PartialEq)]
struct UpdateTarget {
    image: ImageReference,
    operator_version: OperatorVersion,
}

fn validate_request(action: Action, metadata: Option<&Metadata>) -> Result<(&str, UpdateTarget), RequestViolation<'_>> {
    let options = metadata.and_then(|metadata| metadata.engine_v2_options.as_ref());
    let unit = options.and_then(|options| options.operator_container_update.as_ref());
    let unit_key = unit.map(|unit| unit.key.as_str()).unwrap_or(OPERATOR_UNIT_KEY);
    let refuse = |message: String| Err(RequestViolation { unit_key, message });

    if action != Action::Create {
        return refuse(format!(
            "unsupported action `{action}` for operator_container_update: only `create` is allowed"
        ));
    }
    if metadata.and_then(|metadata| metadata.dry_run_deploy) == Some(true) {
        return refuse("operator_container_update does not support dry runs".to_string());
    }
    let Some(options) = options else {
        return refuse("operator_container_update requires engine_v2_options".to_string());
    };
    match options.schema_version.parse::<u32>() {
        Ok(LEGACY_REQUEST_SCHEMA_VERSION | PREFLIGHT_REQUEST_SCHEMA_VERSION) => {}
        _ => return refuse(format!("unsupported request schema_version `{}`", options.schema_version)),
    }
    if options.preflight.is_some() {
        return refuse("operator_container_update does not accept preflight instructions".to_string());
    }
    if !options.platform_helm_units.is_empty() {
        return refuse("operator_container_update does not accept platform_helm_units".to_string());
    }
    let Some(unit) = unit else {
        return refuse(
            "execution_mode is operator_container_update but the request carries no operator_container_update"
                .to_string(),
        );
    };
    if unit.key != OPERATOR_UNIT_KEY {
        return refuse(format!(
            "operator_container_update key must be `{OPERATOR_UNIT_KEY}`, got `{}`",
            unit.key
        ));
    }
    let image = match ImageReference::parse(&unit.image) {
        Ok(image) => image,
        Err(err) => return refuse(format!("invalid operator_container_update image: {err}")),
    };
    let operator_version = match OperatorVersion::parse(&unit.operator_version) {
        Ok(version) => version,
        Err(err) => return refuse(format!("invalid operator_container_update: {err}")),
    };

    Ok((
        unit_key,
        UpdateTarget {
            image,
            operator_version,
        },
    ))
}

fn run_update<D: OperatorDocker>(
    docker: &D,
    logger: &impl InfraLogger,
    plan: &UpdatePlan<'_>,
) -> Result<PinnedImage, UpdateFailure> {
    let host_platform = docker.host_platform().map_err(|failure| {
        UpdateFailure::new(
            UpdateFailureKind::DockerUnavailable,
            format!("the Docker daemon is not reachable from the worker: {failure}"),
        )
    })?;

    let operator = find_operator(docker.list_running_containers()?, &plan.identity).map_err(|err| match err {
        DiscoveryError::NotFound => UpdateFailure::new(
            UpdateFailureKind::OperatorNotFound,
            "no running Docker Operator container matches this cluster",
        ),
        DiscoveryError::Ambiguous(names) => UpdateFailure::new(
            UpdateFailureKind::OperatorAmbiguous,
            format!(
                "several running Docker Operator containers match this cluster: {}",
                names.iter().map(ContainerName::as_str).collect::<Vec<_>>().join(", ")
            ),
        ),
    })?;
    logger.info(format!(
        "🔎 Operator container `{}` found ({})",
        operator.name,
        operator.image.as_str()
    ));

    let old_image = docker.inspect_image(ImageLookup::Id(&operator.image))?;
    let mut operator = check_supported_profile(operator, old_image.config).map_err(|unsupported| {
        UpdateFailure::new(UpdateFailureKind::UnsupportedConfiguration, unsupported.to_string())
    })?;
    check_leftovers_and_remove_next(docker, logger, &mut operator, &plan.identity)?;

    logger.info(format!("⬇️ Pulling {}", plan.image));
    let pull_failure =
        |failure: DockerFailure| UpdateFailure::new(UpdateFailureKind::ImagePullFailed, failure.to_string());
    docker.pull_image(&plan.image).map_err(pull_failure)?;
    let new_image = docker
        .inspect_image(ImageLookup::Reference(&plan.image))
        .map_err(pull_failure)?;
    match Platform::parse(&new_image.os, &new_image.architecture) {
        Ok(image_platform) if image_platform == host_platform => {}
        _ => {
            return Err(UpdateFailure::new(
                UpdateFailureKind::ImagePlatformMismatch,
                format!(
                    "image platform {}/{} does not match the host platform {host_platform}",
                    new_image.os, new_image.architecture
                ),
            ));
        }
    }
    let pinned_image = PinnedImage::resolve(&plan.image, &new_image.repo_digests).ok_or_else(|| {
        UpdateFailure::new(
            UpdateFailureKind::ImagePullFailed,
            match plan.image.digest() {
                Some(digest) => format!("the pulled image does not carry the requested digest {digest}"),
                None => format!("the pulled image has no digest for repository {}", plan.image.repository()),
            },
        )
    })?;
    logger.info(format!("📌 Resolved {} to {pinned_image}", plan.image));

    let spec = operator.container_spec(&pinned_image, &plan.operator_version, plan.execution_id);
    let new_container = docker.create_container(&spec)?;
    logger.info(format!("🆕 Created `{}` from {pinned_image}", spec.name));
    if let Err(failure) = remove_previous(docker, logger, operator.container(), &plan.identity) {
        return Err(discard_new_container(docker, &new_container, &spec.name, failure));
    }

    let cutover = Cutover {
        old: operator.container(),
        new_container: &new_container,
    };
    logger.info(format!("🔁 Replacing `{}`", cutover.old.name));
    if let Err(failure) = cutover.run(docker) {
        return Err(cutover.roll_back(docker, logger, RollbackCause::CutOver(failure)));
    }

    logger.info(format!(
        "🩺 Waiting {}s for `{}` to stay running",
        plan.health.window.as_secs(),
        cutover.old.name
    ));
    if let Err(unhealthy) = wait_until_healthy(docker, &new_container, plan.health) {
        return Err(cutover.roll_back(docker, logger, RollbackCause::Unhealthy(unhealthy)));
    }

    logger.info(format!(
        "✅ `{}` runs {pinned_image}; `{}` is kept stopped as the rollback point",
        cutover.old.name,
        cutover.old.name.previous()
    ));
    Ok(pinned_image)
}

fn refuse_leftover(container: &ContainerInspect) -> UpdateFailure {
    UpdateFailure::new(
        UpdateFailureKind::UnsupportedConfiguration,
        format!(
            "`{}` exists but is not a stopped leftover of an update of this Operator",
            container.name
        ),
    )
}

fn check_previous(previous: &ContainerInspect, identity: &OperatorIdentity) -> Result<(), UpdateFailure> {
    let is_stopped = matches!(
        previous.state.status,
        ContainerStatus::Created | ContainerStatus::Exited | ContainerStatus::Dead
    );
    if !is_stopped || !identity.matches(previous) {
        return Err(refuse_leftover(previous));
    }
    Ok(())
}

fn check_leftovers_and_remove_next<D: OperatorDocker>(
    docker: &D,
    logger: &impl InfraLogger,
    operator: &mut SupportedOperator,
    identity: &OperatorIdentity,
) -> Result<(), UpdateFailure> {
    let name = &operator.container().name;
    let leftover_names = [name.next(), name.previous()];
    for leftover in docker.find_containers_named(&leftover_names)? {
        if leftover.name == leftover_names[0] {
            if !leftover.config.labels.contains_key(UPDATE_LABEL) || !identity.matches(&leftover) {
                return Err(refuse_leftover(&leftover));
            }
            restore_interrupted_restart_policy(docker, logger, operator, &leftover)?;
            docker.remove_container(&leftover.id)?;
            logger.info(format!("🧹 Removed leftover `{}`", leftover.name));
        } else if leftover.name == leftover_names[1] {
            check_previous(&leftover, identity)?;
        }
    }
    Ok(())
}

fn remove_previous<D: OperatorDocker>(
    docker: &D,
    logger: &impl InfraLogger,
    operator: &ContainerInspect,
    identity: &OperatorIdentity,
) -> Result<(), UpdateFailure> {
    let name = operator.name.previous();
    let Some(previous) = docker
        .find_containers_named(slice::from_ref(&name))?
        .into_iter()
        .find(|container| container.name == name)
    else {
        return Ok(());
    };
    check_previous(&previous, identity)?;
    docker.remove_container(&previous.id)?;
    logger.info(format!("🧹 Removed the former rollback point `{}`", previous.name));
    Ok(())
}

fn discard_new_container<D: OperatorDocker>(
    docker: &D,
    new_container: &ContainerId,
    name: &ContainerName,
    failure: UpdateFailure,
) -> UpdateFailure {
    match docker.remove_container(new_container) {
        Ok(()) => failure,
        Err(leftover) => UpdateFailure::new(
            failure.kind,
            format!("{}; `{name}` could not be removed: {leftover}", failure.message),
        ),
    }
}

fn restore_interrupted_restart_policy<D: OperatorDocker>(
    docker: &D,
    logger: &impl InfraLogger,
    operator: &mut SupportedOperator,
    next: &ContainerInspect,
) -> Result<(), UpdateFailure> {
    let container = operator.container();
    let labels = &next.config.labels;
    if container.host_config.restart_policy != RestartPolicy::NO
        || labels.get(PREVIOUS_CONTAINER_LABEL).map(String::as_str) != Some(container.id.as_str())
    {
        return Ok(());
    }
    let Some(label) = labels.get(RESTART_POLICY_LABEL) else {
        return Ok(());
    };
    let policy: RestartPolicy = label.parse().map_err(|err| {
        UpdateFailure::new(
            UpdateFailureKind::UnsupportedConfiguration,
            format!("`{}` carries an invalid {RESTART_POLICY_LABEL} label: {err}", next.name),
        )
    })?;
    if policy == RestartPolicy::NO {
        return Ok(());
    }
    docker.update_restart_policy(&container.id, policy)?;
    logger.info(format!(
        "↩️ Restored the `{policy}` restart policy of `{}` left by an interrupted update",
        container.name
    ));
    operator.restore_restart_policy(policy);
    Ok(())
}

#[derive(Debug)]
enum HealthFailure {
    Inspect(DockerFailure),
    NotRunning(ContainerStatus),
    Restarted(u64),
}

impl Display for HealthFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthFailure::Inspect(failure) => write!(f, "cannot check the new Operator container: {failure}"),
            HealthFailure::NotRunning(status) => {
                write!(f, "the new Operator container is {status:?} instead of running")
            }
            HealthFailure::Restarted(count) => write!(f, "the new Operator container restarted {count} time(s)"),
        }
    }
}

#[derive(Debug)]
enum RollbackCause {
    CutOver(DockerFailure),
    Unhealthy(HealthFailure),
}

impl Display for RollbackCause {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RollbackCause::CutOver(failure) => write!(f, "cut-over failed: {failure}"),
            RollbackCause::Unhealthy(failure) => write!(f, "{failure}"),
        }
    }
}

struct Cutover<'a> {
    old: &'a ContainerInspect,
    new_container: &'a ContainerId,
}

impl Cutover<'_> {
    fn run<D: OperatorDocker>(&self, docker: &D) -> Result<(), DockerFailure> {
        let old = self.old;
        if !old.host_config.restart_policy.keeps_a_stopped_container_stopped() {
            docker.update_restart_policy(&old.id, RestartPolicy::NO)?;
        }
        docker.stop_container(&old.id)?;
        docker.rename_container(&old.id, &old.name.previous())?;
        docker.rename_container(self.new_container, &old.name)?;
        docker.start_container(self.new_container)
    }

    fn roll_back<D: OperatorDocker>(
        &self,
        docker: &D,
        logger: &impl InfraLogger,
        cause: RollbackCause,
    ) -> UpdateFailure {
        let old = self.old;
        logger.warn(format!("↩️ Rolling back `{}`: {cause}", old.name));
        let mut failures: Vec<DockerFailure> = Vec::new();
        let new_removed = match docker.remove_container(self.new_container) {
            Ok(()) => true,
            Err(failure) => match docker.container_exists(self.new_container) {
                Ok(false) => {
                    logger.warn(format!("{failure}, but the new Operator container is gone"));
                    true
                }
                Ok(true) | Err(_) => {
                    failures.push(failure);
                    false
                }
            },
        };
        let (needs_rename, needs_policy) = match docker.inspect_container(&old.id) {
            Ok(current) => (
                current.name != old.name,
                current.host_config.restart_policy != old.host_config.restart_policy,
            ),
            Err(failure) => {
                failures.push(failure);
                (true, true)
            }
        };
        let original_policy = old.host_config.restart_policy;
        let can_restore_policy = new_removed || original_policy.keeps_a_stopped_container_stopped();
        if needs_policy
            && can_restore_policy
            && let Err(failure) = docker.update_restart_policy(&old.id, original_policy)
        {
            failures.push(failure);
        }
        if new_removed {
            if needs_rename && let Err(failure) = docker.rename_container(&old.id, &old.name) {
                failures.push(failure);
            }
            if let Err(failure) = docker.start_container(&old.id) {
                failures.push(failure);
            }
        }

        if failures.is_empty() {
            return UpdateFailure::new(
                UpdateFailureKind::RolledBack,
                format!("{cause}; the previous Operator container was restored"),
            );
        }
        let failures: Vec<String> = failures.iter().map(DockerFailure::to_string).collect();
        let pending_policy = if needs_policy && !can_restore_policy {
            format!(
                "; restore the `{original_policy}` restart policy of container {} once the new container is removed",
                old.id.short()
            )
        } else {
            String::new()
        };
        UpdateFailure::new(
            UpdateFailureKind::RollbackFailed,
            format!(
                "{cause}; rollback failed ({}): manual recovery required{pending_policy}",
                failures.join("; ")
            ),
        )
    }
}

fn wait_until_healthy<D: OperatorDocker>(
    docker: &D,
    container: &ContainerId,
    health: HealthCheck,
) -> Result<(), HealthFailure> {
    let deadline = Instant::now() + health.window;
    loop {
        let inspect = docker.inspect_container(container).map_err(HealthFailure::Inspect)?;
        if inspect.state.status != ContainerStatus::Running {
            return Err(HealthFailure::NotRunning(inspect.state.status));
        }
        if inspect.restart_count > 0 {
            return Err(HealthFailure::Restarted(inspect.restart_count));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        thread::sleep(health.poll_interval.min(deadline - now));
    }
}

#[cfg(test)]
mod tests;
