//! Diagnostics emitted when a periodic reporter sees a failing pod and before the terminal deployment error.
//! Only the latest rendered observation and its deduplication signature are retained for final replay.
use crate::environment::report::logger::EnvLogger;
use crate::environment::report::utils::{QPodExt, exit_code_to_msg};
use crate::runtime::block_on;
use futures::{AsyncReadExt, future::join_all};
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{ContainerState, Event, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Api;
use kube::api::{ListParams, LogParams};
use regex::Regex;
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

const LOG_READ_BYTES: u64 = 64 * 1024;
const DEADLINE: Duration = Duration::from_secs(3);
const OUTPUT_WAITER: &str = "qovery-wait-container-output";

pub(crate) enum WorkloadKind<'a> {
    Deployment(&'a str),
    Job,
    Pods,
    NamedPod,
}

impl WorkloadKind<'_> {
    fn list_params(&self, selector: &str) -> ListParams {
        match self {
            Self::NamedPod => ListParams::default().fields(&format!("metadata.name={selector}")),
            Self::Deployment(_) | Self::Job | Self::Pods => ListParams::default().labels(selector),
        }
    }

    fn service_version(&self) -> Option<&str> {
        match self {
            Self::Deployment(version) => Some(version),
            Self::Job | Self::Pods | Self::NamedPod => None,
        }
    }

    /// Whether Jobs are listed by selector; named pods verify their owner with a separate GET.
    fn reads_jobs(&self) -> bool {
        match self {
            Self::Job => true,
            Self::Deployment(_) | Self::Pods | Self::NamedPod => false,
        }
    }

    fn trusts_owner_references(&self) -> bool {
        match self {
            // A named pod's Job must instead be verified by a live GET and UID check.
            Self::NamedPod => false,
            Self::Deployment(_) | Self::Job | Self::Pods => true,
        }
    }

    fn is_current(&self, pod: &Pod, job: Option<&Job>) -> bool {
        match self {
            Self::Deployment(version) => pod.service_version().as_deref() == Some(*version),
            Self::Job => pod.metadata.owner_references.iter().flatten().any(|owner| {
                owner.kind == "Job"
                    && job.is_some_and(|job| {
                        job.metadata.uid.as_deref() == Some(&owner.uid)
                            && job.metadata.name.as_deref() == Some(&owner.name)
                    })
            }),
            Self::Pods | Self::NamedPod => true,
        }
    }
}

pub(crate) struct Target<'a> {
    pub kube: &'a kube::Client,
    pub namespace: &'a str,
    pub selector: &'a str,
    pub kind: WorkloadKind<'a>,
}

/// The latest redacted, bounded block emitted during deployment and its deduplication signature.
pub(crate) struct ObservedFailure {
    signature: String,
    lines: Vec<String>,
}

#[derive(Default)]
struct CollectedDiagnostics {
    /// Customer recap lines mixing status/cause details, supporting warnings and filtered log excerpts.
    /// Already redacted, sanitized and bounded; these are ready to emit, not raw container log lines.
    lines: Vec<String>,
    /// Live Job or pod status supplied details. Warnings and log excerpts do not set this flag.
    has_workload_details: bool,
}

/// Only terminal failures call this; diagnostics never change the caller's result or logger state.
/// Prefer fresh workload facts; otherwise label the retained observation as historical evidence.
pub(crate) fn report_failure(target: &Target<'_>, logger: &EnvLogger, last: Option<ObservedFailure>) {
    let collected = block_on(collect(target, logger, Instant::now() + DEADLINE, "Why it failed"));
    let mut lines = collected.lines;
    if !collected.has_workload_details
        && let Some(observed) = last
    {
        lines = observed.lines;
        if let Some(heading) = lines.first_mut() {
            *heading = safe_line(
                logger,
                "Why it failed: earlier deployment observation (final cause unconfirmed)".to_string(),
            );
        }
    }
    if lines.is_empty() {
        lines.push(safe_line(
            logger,
            "Why it failed: live workload evidence could not be read; cause unavailable".to_string(),
        ));
    }
    lines.into_iter().for_each(|line| logger.send_recap(line));
}

/// Called by a periodic reporter with the pods it just listed. A failure seen on a live pod is printed at once,
/// before a rollback can delete the pod. The deployment may still succeed, so the block only says what was observed.
/// `last` holds only the signature and rendered block of the last printed failure.
/// `job` is the live Job of a `WorkloadKind::Job` target: only its own pods can trigger a block.
pub(crate) fn report_observed_failure(
    target: &Target<'_>,
    logger: &EnvLogger,
    job: Option<&Job>,
    pods: &[Pod],
    last: &mut Option<ObservedFailure>,
) {
    let mut pods: Vec<&Pod> = pods
        .iter()
        .filter(|pod| pod.metadata.deletion_timestamp.is_none() && target.kind.is_current(pod, job))
        .collect();
    // An older failed Job can leave pods behind: keep the newest pod owned by the live Job.
    if target.kind.reads_jobs() {
        pods = pods
            .into_iter()
            .max_by(|a, b| a.metadata.creation_timestamp.cmp(&b.metadata.creation_timestamp))
            .into_iter()
            .collect();
    }
    let Some(signature) = pods.into_iter().find_map(failure_signature) else {
        return;
    };
    // A block that could not be read is retried at the next observation.
    if last.as_ref().is_some_and(|last| last.signature == signature) {
        return;
    }
    let collected = block_on(collect(
        target,
        logger,
        Instant::now() + DEADLINE,
        "Failure observed during deployment",
    ));
    // A read without workload facts must not replace earlier evidence or suppress a retry.
    if collected.has_workload_details {
        collected.lines.iter().for_each(|line| logger.send_recap(line.clone()));
        *last = Some(ObservedFailure {
            signature,
            lines: collected.lines,
        });
    }
}

/// `pod uid/container/restart count/reason` of the first concrete failure: unschedulable, image or config error,
/// crash loop or a container that terminated with an error.
fn failure_signature(pod: &Pod) -> Option<String> {
    let uid = pod.metadata.uid.as_deref()?;
    let status = pod.status.as_ref()?;
    if ready(pod) {
        return None;
    }
    if status
        .conditions
        .iter()
        .flatten()
        .any(|c| c.type_ == "PodScheduled" && c.status == "False")
    {
        return Some(format!("{uid}//0/Unschedulable"));
    }
    status
        .init_container_statuses
        .iter()
        .chain(&status.container_statuses)
        .flatten()
        .filter(|s| !s.ready && s.name != OUTPUT_WAITER)
        .find_map(|s| {
            let state = s.state.as_ref()?;
            let reason = match (&state.waiting, &state.terminated) {
                (Some(w), _) => w.reason.as_deref().filter(|r| {
                    [
                        "CrashLoopBackOff",
                        "ErrImagePull",
                        "ImagePullBackOff",
                        "InvalidImageName",
                        "CreateContainerConfigError",
                        "CreateContainerError",
                    ]
                    .contains(r)
                })?,
                (_, Some(t)) if t.exit_code != 0 => t.reason.as_deref().unwrap_or("Terminated"),
                _ => return None,
            };
            Some(format!("{uid}/{}/{}/{reason}", s.name, s.restart_count))
        })
}

async fn read<T>(deadline: Instant, query: impl Future<Output = Result<T, kube::Error>>) -> Option<T> {
    timeout_at(deadline, query)
        .await
        .map_err(|e| e.to_string())
        .and_then(|result| result.map_err(|e| e.to_string()))
        .inspect_err(|error| debug!("failure diagnostics: {error}"))
        .ok()
}

/// Gather diagnostics for the namespace, selector and workload kind in `target` within one absolute `deadline`.
/// Read live pod/Job status, warnings attributed through pod/ReplicaSet/Job UIDs, and bounded container
/// log excerpts matching error/fatal/panic/exception. Scope deployments to their revision and Job pods to
/// the newest live Job; a named pod's Job owner must be verified by a live read.
/// Return customer report lines under `heading`, redacted with `logger`, plus live-workload-detail metadata;
/// this function does not emit them. Independent read failures preserve any other available evidence.
async fn collect(target: &Target<'_>, logger: &EnvLogger, deadline: Instant, heading: &str) -> CollectedDiagnostics {
    let pods_api: Api<Pod> = Api::namespaced(target.kube.clone(), target.namespace);
    let events_api: Api<Event> = Api::namespaced(target.kube.clone(), target.namespace);
    let rs_api: Api<ReplicaSet> = Api::namespaced(target.kube.clone(), target.namespace);
    let jobs_api: Api<Job> = Api::namespaced(target.kube.clone(), target.namespace);
    let params = target.kind.list_params(target.selector);
    let warnings = ListParams::default().fields("type=Warning");
    // Independent reads share an absolute deadline: a denied or hanging endpoint cannot discard other evidence.
    let (pods, events, replicasets, jobs) = tokio::join!(
        read(deadline, pods_api.list(&params)),
        read(deadline, events_api.list(&warnings)),
        async {
            target.kind.service_version()?;
            read(deadline, rs_api.list(&params)).await
        },
        async {
            if !target.kind.reads_jobs() {
                return None;
            }
            read(deadline, jobs_api.list(&params)).await
        }
    );
    if pods.is_none() && events.is_none() && replicasets.is_none() && jobs.is_none() {
        return CollectedDiagnostics::default();
    }
    let workload_read = pods.is_some()
        && (!target.kind.reads_jobs() || jobs.is_some())
        && (target.kind.service_version().is_none() || replicasets.is_some());
    let mut pods = pods.map(|list| list.items).unwrap_or_default();
    let mut jobs = jobs.map(|list| list.items).unwrap_or_default();
    let mut owners = BTreeSet::new();
    if let Some(version) = target.kind.service_version() {
        for rs in replicasets.into_iter().flat_map(|list| list.items).filter(|rs| {
            rs.spec
                .as_ref()
                .and_then(|spec| spec.template.as_ref())
                .and_then(|template| template.metadata.as_ref())
                .and_then(|meta| meta.annotations.as_ref())
                .and_then(|annotations| annotations.get("qovery.com/service-version"))
                .is_some_and(|v| v == version)
        }) {
            owners.extend(warning_uids(&rs.metadata));
        }
    }
    // Terraform has only a pod address. Its Job is trusted only after a live GET verifies the referenced UID.
    if !target.kind.trusts_owner_references()
        && let Some(owner) = pods
            .first()
            .and_then(|pod| pod.metadata.owner_references.as_ref())
            .and_then(|owners| owners.iter().find(|owner| owner.kind == "Job"))
        && let Some(job) = read(deadline, jobs_api.get(&owner.name)).await
        && job.metadata.uid.as_deref() == Some(&owner.uid)
    {
        jobs.push(job);
    }
    jobs.sort_by(|a, b| b.metadata.creation_timestamp.cmp(&a.metadata.creation_timestamp));
    jobs.truncate(1);
    pods.retain(|pod| target.kind.is_current(pod, jobs.first()));
    let mut details = Vec::new();
    append_job_failures(&jobs, &mut owners, &mut details);
    pods.sort_by(|a, b| b.metadata.creation_timestamp.cmp(&a.metadata.creation_timestamp));
    let mut pending = Vec::new();
    for pod in pods.iter().filter(|pod| !ready(pod)).take(4) {
        owners.extend(pod.metadata.uid.clone());
        if target.kind.trusts_owner_references() {
            owners.extend(warning_uids(&pod.metadata));
        }
        let name = pod.metadata.name.as_deref().unwrap_or("?");
        let Some(status) = &pod.status else {
            continue;
        };
        for condition in status
            .conditions
            .iter()
            .flatten()
            .filter(|c| c.type_ == "PodScheduled" && c.status == "False")
        {
            let line = format!("Pod/{name}: {}", reason_message(&condition.reason, &condition.message));
            details.push(line);
        }
        for (init, container) in status
            .init_container_statuses
            .iter()
            .flatten()
            .map(|s| (true, s))
            .chain(status.container_statuses.iter().flatten().map(|s| (false, s)))
            .filter(|(_, s)| {
                !s.ready
                    && s.name != OUTPUT_WAITER
                    && s.state
                        .as_ref()
                        .and_then(|s| s.terminated.as_ref())
                        .is_none_or(|s| s.exit_code != 0)
            })
            .take(3)
        {
            let current = container.state.as_ref();
            let last = container.last_state.as_ref();
            // Keep state and exit facts ahead of potentially long messages.
            details.push(format!(
                "{} '{}' in Pod/{name}: {}; previous: {}; restarts {}; {} {}",
                if init { "init container" } else { "container" },
                container.name,
                describe(current),
                describe(last),
                container.restart_count,
                message(current),
                message(last)
            ));
            let terminated = current.and_then(|s| s.terminated.as_ref());
            let previous = terminated.is_none() && container.restart_count > 0;
            let exit = terminated.or_else(|| last.and_then(|s| s.terminated.as_ref()).filter(|_| previous));
            if let Some(exit) = exit.filter(|exit| exit.exit_code != 0) {
                if let Some(hint) = exit_code_to_msg(exit.exit_code) {
                    details.push(format!("💭 {hint}"));
                }
                if pending.len() < 4 {
                    pending.push(log_excerpt(&pods_api, name, &container.name, previous, logger, deadline));
                }
            }
        }
        details.push(format!(
            "Pod/{name}: {} {}",
            status.phase.as_deref().unwrap_or("Unknown"),
            reason_message(&status.reason, &status.message)
        ));
    }
    let unavailable = if !workload_read {
        "live workload evidence could not be read; cause unavailable"
    } else if pods.is_empty() {
        "no matching live pods; evidence removed by cleanup is unavailable"
    } else {
        "no specific failure is established by live workload status"
    };
    let has_workload_details = !details.is_empty();
    let summary = details.first().map(String::as_str).unwrap_or(unavailable);
    let mut lines = vec![format!("{heading}: {summary}")];
    lines.extend(details.into_iter().map(|line| format!("⚠️ {line}")));
    append_supporting_warnings(events.map(|list| list.items).unwrap_or_default(), &owners, &mut lines);
    lines.extend(join_all(pending).await.into_iter().flatten());
    CollectedDiagnostics {
        lines: lines.into_iter().take(50).map(|line| safe_line(logger, line)).collect(),
        has_workload_details,
    }
}

fn append_job_failures(jobs: &[Job], owners: &mut BTreeSet<String>, details: &mut Vec<String>) {
    for job in jobs {
        owners.extend(warning_uids(&job.metadata));
        if let Some(condition) = job
            .status
            .as_ref()
            .and_then(|status| status.conditions.as_ref())
            .and_then(|conditions| conditions.iter().find(|c| c.type_ == "Failed" && c.status == "True"))
        {
            let line = format!(
                "Job/{} failed: {}",
                job.metadata.name.as_deref().unwrap_or("?"),
                reason_message(&condition.reason, &condition.message)
            );
            details.push(line);
        }
    }
}

fn append_supporting_warnings(mut events: Vec<Event>, owners: &BTreeSet<String>, lines: &mut Vec<String>) {
    events.sort_by_key(|event| std::cmp::Reverse(event_time(event)));
    let mut seen = BTreeSet::new();
    for event in events
        .iter()
        .filter(|event| {
            event.type_.as_deref() == Some("Warning")
                && event
                    .involved_object
                    .uid
                    .as_ref()
                    .is_some_and(|uid| owners.contains(uid))
        })
        .filter(|event| seen.insert((&event.involved_object.uid, &event.reason, &event.message)))
        .take(6)
    {
        // Warnings may outlive the condition. Keep them timestamped and never promote them to the current cause.
        lines.push(format!(
            "⚠️ Supporting warning at {}: {}/{} {}: {}",
            event_time(event)
                .map(|time| time.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            event.involved_object.kind.as_deref().unwrap_or("?"),
            event.involved_object.name.as_deref().unwrap_or("?"),
            event.reason.as_deref().unwrap_or_default(),
            event.message.as_deref().unwrap_or_default()
        ));
    }
}

// References on live objects scope supporting warnings; they do not establish the owner's current state.
fn warning_uids(metadata: &ObjectMeta) -> impl Iterator<Item = String> + '_ {
    metadata.uid.iter().cloned().chain(
        metadata
            .owner_references
            .iter()
            .flatten()
            .map(|owner| owner.uid.clone()),
    )
}

fn ready(pod: &Pod) -> bool {
    pod.status.as_ref().is_some_and(|s| {
        s.phase.as_deref() == Some("Succeeded")
            || s.phase.as_deref() != Some("Failed")
                && s.conditions
                    .iter()
                    .flatten()
                    .any(|c| c.type_ == "Ready" && c.status == "True")
    })
}

fn reason_message(reason: &Option<String>, message: &Option<String>) -> String {
    format!(
        "{}: {}",
        reason.as_deref().unwrap_or_default(),
        message.as_deref().unwrap_or_default()
    )
}

fn describe(state: Option<&ContainerState>) -> String {
    if let Some(waiting) = state.and_then(|s| s.waiting.as_ref()) {
        format!("waiting {}", waiting.reason.as_deref().unwrap_or("unknown"))
    } else if let Some(exit) = state.and_then(|s| s.terminated.as_ref()) {
        format!(
            "{} (exit code {}, signal {})",
            exit.reason.as_deref().unwrap_or("terminated"),
            exit.exit_code,
            exit.signal.unwrap_or_default()
        )
    } else if state.is_some_and(|s| s.running.is_some()) {
        "running, not ready".to_string()
    } else {
        "unknown".to_string()
    }
}

fn message(state: Option<&ContainerState>) -> &str {
    state
        .and_then(|s| {
            s.waiting
                .as_ref()
                .and_then(|s| s.message.as_deref())
                .or_else(|| s.terminated.as_ref().and_then(|s| s.message.as_deref()))
        })
        .unwrap_or_default()
}

fn event_time(event: &Event) -> Option<k8s_openapi::jiff::Timestamp> {
    [
        event
            .series
            .as_ref()
            .and_then(|s| s.last_observed_time.as_ref())
            .map(|t| t.0),
        event.event_time.as_ref().map(|t| t.0),
        event.last_timestamp.as_ref().map(|t| t.0),
        event.first_timestamp.as_ref().map(|t| t.0),
        event.metadata.creation_timestamp.as_ref().map(|t| t.0),
    ]
    .into_iter()
    .flatten()
    .max()
}

async fn log_excerpt(
    api: &Api<Pod>,
    pod: &str,
    container: &str,
    previous: bool,
    logger: &EnvLogger,
    deadline: Instant,
) -> Vec<String> {
    let params = LogParams {
        container: Some(container.to_string()),
        previous,
        // A server-side tail can begin inside a multiline secret. Select lines only after redaction.
        limit_bytes: Some(LOG_READ_BYTES as i64 + 1),
        ..Default::default()
    };
    let Some(stream) = read(deadline, api.log_stream(pod, &params)).await else {
        return vec![];
    };
    let Ok(Some(bytes)) = timeout_at(deadline, read_complete(Box::pin(stream))).await else {
        return vec![];
    };
    // Redact the complete stream before splitting, matching, sanitizing or truncating any part of it.
    let text = logger.obfuscate(String::from_utf8_lossy(&bytes).into_owned());
    let run = if previous { "previous" } else { "current" };
    error_excerpt(&text)
        .into_iter()
        .map(|line| format!("💭 Pod/{pod}, container '{container}', {run}: {line}"))
        .collect()
}

async fn read_complete(reader: impl futures::AsyncRead + Unpin) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    match reader.take(LOG_READ_BYTES + 1).read_to_end(&mut bytes).await {
        Ok(_) if bytes.len() as u64 <= LOG_READ_BYTES => Some(bytes),
        _ => {
            debug!("failure diagnostics: incomplete or capped log stream suppressed");
            None
        }
    }
}

fn error_excerpt(text: &str) -> Vec<String> {
    let lines: Vec<_> = text.lines().collect();
    lines
        .iter()
        .skip(lines.len().saturating_sub(30))
        .filter(|line| {
            let lower = line.to_lowercase();
            ["error", "fatal", "panic", "exception"]
                .iter()
                .any(|marker| lower.contains(marker))
        })
        .map(|line| format!("    | {line}"))
        .collect()
}

// Applied after complete-text redaction. Strip ANSI before replacing controls and line separators.
fn safe_line(logger: &EnvLogger, text: String) -> String {
    static ANSI: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|.)").expect("valid ANSI regex")
    });
    let text = logger.obfuscate(text);
    let text = ANSI.replace_all(&text, "");
    static CONTROLS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[\p{Cc}\x{200b}-\x{200f}\x{2028}-\x{202e}\x{2060}-\x{2069}\x{feff}]").expect("valid control regex")
    });
    let text = CONTROLS.replace_all(&text, " ").into_owned();
    // Removing ANSI can join pieces into a secret. Mask again before the character cap.
    let mut line: String = logger.obfuscate(text).chars().take(300).collect();
    // Normal logging masks once more; a one-character secret can expand into "xxx" there.
    while !line.is_empty() && logger.obfuscate(line.clone()).chars().count() > 300 {
        line.pop();
    }
    line
}

#[cfg(test)]
pub(crate) mod tests;
