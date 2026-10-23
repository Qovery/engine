//! Concise diagnostics included in periodic status reports and before the terminal deployment error.
//! Only the latest rendered observation and its deduplication signature are retained for final replay.
use crate::environment::report::logger::EnvLogger;
use crate::environment::report::utils::QPodExt;
use crate::runtime::block_on;
use futures::{AsyncReadExt, future::join_all};
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::{ContainerState, ContainerStateTerminated, Event, Pod};
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
}

impl WorkloadKind<'_> {
    fn list_params(&self, selector: &str) -> ListParams {
        match self {
            Self::Deployment(_) | Self::Job | Self::Pods => ListParams::default().labels(selector),
        }
    }

    fn service_version(&self) -> Option<&str> {
        match self {
            Self::Deployment(version) => Some(version),
            Self::Job | Self::Pods => None,
        }
    }

    /// Whether the current Job must be listed to verify pod ownership.
    fn reads_jobs(&self) -> bool {
        match self {
            Self::Job => true,
            Self::Deployment(_) | Self::Pods => false,
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
            Self::Pods => true,
        }
    }
}

pub(crate) struct Target<'a> {
    pub kube: &'a kube::Client,
    pub namespace: &'a str,
    pub selector: &'a str,
    pub kind: WorkloadKind<'a>,
}

/// The latest redacted, bounded observation and its deduplication signature.
pub(crate) struct ObservedFailure {
    signature: String,
    lines: Vec<String>,
}

#[derive(Default)]
struct CollectedDiagnostics {
    /// Concise cause details and filtered log excerpts; supporting warnings only when no live cause exists.
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
        lines = vec!["Failure observed before rollback:".to_string()];
        lines.extend(without_heading(observed.lines));
    }
    if lines.is_empty() {
        lines.push(safe_line(
            logger,
            "Why it failed: live workload evidence could not be read; cause unavailable".to_string(),
        ));
    }
    lines.into_iter().for_each(|line| logger.send_recap(line));
}

/// Collect a failure for inclusion in the periodic status report before rollback can delete the pod.
/// Retain the latest evidence for the terminal recap, without emitting a separate recap event.
/// Reuse the rendered evidence while the live failure signature is unchanged.
/// `job` is the live Job of a `WorkloadKind::Job` target: only its own pods can trigger a block.
pub(crate) fn report_observed_failure(
    target: &Target<'_>,
    logger: &EnvLogger,
    job: Option<&Job>,
    pods: &[Pod],
    last: &mut Option<ObservedFailure>,
) -> Vec<String> {
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
        return vec![];
    };
    // A block that could not be read is retried at the next observation.
    if let Some(last) = last.as_ref().filter(|last| last.signature == signature) {
        return last.lines.clone();
    }
    let collected = block_on(collect(
        target,
        logger,
        Instant::now() + DEADLINE,
        "Failure observed during deployment",
    ));
    // A read without workload facts must not replace earlier evidence or suppress a retry.
    if collected.has_workload_details {
        let lines = collected.lines;
        *last = Some(ObservedFailure {
            signature,
            lines: lines.clone(),
        });
        return lines;
    }
    vec![]
}

/// Insert already sanitized diagnostics into the existing report frame without re-evaluating templates.
pub(crate) fn with_failure_diagnostics(mut report: String, lines: Vec<String>) -> String {
    if lines.is_empty() {
        return report;
    }
    let border = report.rfind("\n┗").unwrap_or(report.len());
    let mut diagnostics = String::new();
    for (index, line) in without_heading(lines).into_iter().enumerate() {
        diagnostics.push_str("\n┃ ");
        if index == 0 {
            diagnostics.push_str("⚠️ ");
        }
        diagnostics.push_str(&line);
    }
    report.insert_str(border, &diagnostics);
    report
}

fn without_heading(mut lines: Vec<String>) -> Vec<String> {
    if let Some(first) = lines.first_mut()
        && let Some((_, cause)) = first.split_once(": ")
    {
        *first = cause.to_string();
    }
    lines
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
/// the newest live Job.
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
    jobs.sort_by(|a, b| b.metadata.creation_timestamp.cmp(&a.metadata.creation_timestamp));
    jobs.truncate(1);
    pods.retain(|pod| target.kind.is_current(pod, jobs.first()));
    let mut details = Vec::new();
    append_job_failures(&jobs, &mut owners, &mut details);
    pods.sort_by(|a, b| b.metadata.creation_timestamp.cmp(&a.metadata.creation_timestamp));
    let mut pending = Vec::new();
    for pod in pods.iter().filter(|pod| !ready(pod)).take(4) {
        let previous_details = details.len();
        owners.extend(pod.metadata.uid.clone());
        owners.extend(warning_uids(&pod.metadata));
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
            let Some(description) = describe(current, last, container.restart_count) else {
                continue;
            };
            let kind = if init { "Init container" } else { "Container" };
            let detail = format!("{kind} '{}' {description}.", container.name);
            let previous_message =
                if current.is_none_or(|state| state.terminated.is_none()) && container.restart_count > 0 {
                    message(last)
                } else {
                    ""
                };
            let crash_loop = current
                .and_then(|state| state.waiting.as_ref())
                .is_some_and(|waiting| waiting.reason.as_deref() == Some("CrashLoopBackOff"));
            let state_message = if crash_loop || message(current).is_empty() {
                previous_message
            } else {
                message(current)
            };
            details.push(if state_message.is_empty() {
                detail
            } else {
                format!("{detail} {state_message}")
            });
            let terminated = current.and_then(|s| s.terminated.as_ref());
            let previous = terminated.is_none() && container.restart_count > 0;
            let exit = terminated.or_else(|| last.and_then(|s| s.terminated.as_ref()).filter(|_| previous));
            if exit.is_some_and(|exit| exit.exit_code != 0) && details.len() <= 2 {
                pending.push(log_excerpt(&pods_api, name, &container.name, previous, logger, deadline));
            }
            // Keep the compact cause and excerpt tied to the same container.
            break;
        }
        if details.len() == previous_details && (status.reason.is_some() || status.message.is_some()) {
            details.push(format!("Pod/{name}: {}", reason_message(&status.reason, &status.message)));
        }
        if details.len() > previous_details {
            break;
        }
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
    // The heading already contains the primary cause; retain at most one additional cause.
    let mut seen = BTreeSet::from([summary.to_string()]);
    lines.extend(details.into_iter().filter(|line| seen.insert(line.clone())).take(1));
    if !has_workload_details {
        append_supporting_warnings(events.map(|list| list.items).unwrap_or_default(), &owners, &mut lines);
    }
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

fn describe(current: Option<&ContainerState>, last: Option<&ContainerState>, restarts: i32) -> Option<String> {
    if let Some(exit) = current
        .and_then(|state| state.terminated.as_ref())
        .filter(|exit| exit.exit_code != 0)
    {
        return Some(format!("exited with code {}", termination_details(exit)));
    }
    let previous = last
        .and_then(|state| state.terminated.as_ref())
        .filter(|exit| restarts > 0 && exit.exit_code != 0);
    if let Some(waiting) = current.and_then(|state| state.waiting.as_ref())
        && let Some(reason) = waiting.reason.as_deref()
        && [
            "CrashLoopBackOff",
            "ErrImagePull",
            "ImagePullBackOff",
            "InvalidImageName",
            "CreateContainerConfigError",
            "CreateContainerError",
        ]
        .contains(&reason)
    {
        return Some(match previous {
            Some(exit) => format!("is in {reason} (last exit code {})", termination_details(exit)),
            None => format!("is in {reason}"),
        });
    }
    previous.map(|exit| format!("last exited with code {}", termination_details(exit)))
}

fn termination_details(exit: &ContainerStateTerminated) -> String {
    let mut detail = exit.exit_code.to_string();
    if let Some(reason) = exit.reason.as_deref().filter(|reason| *reason != "Error") {
        detail.push_str(&format!(", {reason}"));
    }
    if let Some(signal) = exit.signal.filter(|signal| *signal != 0) {
        detail.push_str(&format!(", signal {signal}"));
    }
    detail
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
    error_excerpt(&text)
        .into_iter()
        .map(|line| format!("💭 {line}"))
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
        .map(|line| line.trim().to_string())
        .filter({
            let mut seen = BTreeSet::new();
            move |line| seen.insert(line.clone())
        })
        .take(2)
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
