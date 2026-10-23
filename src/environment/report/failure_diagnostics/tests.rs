use super::*;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EnvironmentStep, EventDetails, EventMessageVerbosity, Stage, Transmitter};
use crate::io_models::QoveryIdentifier;
use crate::logger::UnboundedSenderLogger;
use http::{Request, Response};
use kube::client::Body;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use uuid::Uuid;

pub(crate) enum Reply {
    Json(u16, Value),
    Bytes(Vec<u8>),
    Hang,
}
pub(crate) type Requests = Arc<Mutex<Vec<(String, String)>>>;
pub(crate) fn mock_client(handler: impl Fn(&str, &str) -> Reply + Send + Sync + 'static) -> (kube::Client, Requests) {
    let requests: Requests = Arc::default();
    let recorded = Arc::clone(&requests);
    let service = tower::service_fn(move |request: Request<Body>| {
        let path = request.uri().path().to_string();
        let query = request.uri().query().unwrap_or_default().to_string();
        recorded.lock().unwrap().push((path.clone(), query.clone()));
        let reply = handler(path.rsplit('/').next().unwrap_or_default(), &query);
        async move {
            let (status, body) = match reply {
                Reply::Json(status, value) => (status, serde_json::to_vec(&value).unwrap()),
                Reply::Bytes(body) => (200, body),
                Reply::Hang => std::future::pending().await,
            };
            Ok::<_, Infallible>(Response::builder().status(status).body(Body::from(body)).unwrap())
        }
    });
    (block_on(async { kube::Client::new(service, "default") }), requests)
}
pub(crate) fn list(items: Vec<Value>) -> Reply {
    Reply::Json(200, json!({"apiVersion": "v1", "kind": "List", "items": items}))
}
pub(crate) fn denied() -> Reply {
    Reply::Json(
        403,
        json!({
            "kind": "Status", "apiVersion": "v1", "status": "Failure",
            "reason": "Forbidden", "message": "denied", "code": 403,
        }),
    )
}
pub(crate) fn pod_json(status: Value) -> Value {
    json!({
        "metadata": {
            "name": "api", "uid": "pod-uid", "creationTimestamp": "2026-01-01T00:00:00Z",
            "annotations": {"qovery.com/service-version": "v2"},
            "ownerReferences": [{"apiVersion": "batch/v1", "kind": "Job", "name": "job", "uid": "job-uid"}],
        },
        "spec": {"containers": [{"name": "api", "env": [{"name": "TOKEN", "value": "SPEC-SECRET"}]}]},
        "status": status,
    })
}
fn container(state: Value, last: Value, restarts: i32) -> Value {
    json!({
        "phase": "Failed",
        "containerStatuses": [{
            "name": "api", "ready": false, "restartCount": restarts, "image": "img", "imageID": "",
            "state": state, "lastState": last,
        }],
    })
}
pub(crate) fn crash() -> Value {
    container(
        json!({"waiting": {"reason": "CrashLoopBackOff"}}),
        json!({"terminated": {"exitCode": 1}}),
        3,
    )
}
pub(crate) fn event_json(uid: &str, kind: &str, message: &str, at: &str) -> Value {
    let name = match kind {
        "Pod" => "api",
        "Job" => "job",
        _ => "rs",
    };
    json!({
        "involvedObject": {"uid": uid, "name": name, "kind": kind},
        "type": "Warning", "reason": "FailedMount", "message": message, "eventTime": at,
    })
}
pub(crate) fn test_logger() -> (EnvLogger, UnboundedReceiver<EngineEvent>) {
    let (sender, receiver) = unbounded_channel();
    let logger = UnboundedSenderLogger::new(sender, vec!["SECRET\nTOKEN".to_string(), "secret-token".to_string()]);
    let details = event_details(EnvironmentStep::Deploy);
    (EnvLogger::new_for_test(Arc::new(Box::new(logger)), details), receiver)
}

pub(crate) fn event_details(step: EnvironmentStep) -> EventDetails {
    EventDetails::new(
        None,
        QoveryIdentifier::new(Uuid::new_v4()),
        QoveryIdentifier::new(Uuid::new_v4()),
        "execution".to_string(),
        Stage::Environment(step),
        Transmitter::Application(Uuid::new_v4(), "app".to_string()),
    )
}
pub(crate) fn deployment_error() -> EngineError {
    EngineError::new_job_error(event_details(EnvironmentStep::DeployedError), "deployment failed".to_string())
}
pub(crate) fn sent(receiver: &mut UnboundedReceiver<EngineEvent>) -> Vec<EngineEvent> {
    std::iter::from_fn(|| receiver.try_recv().ok()).collect()
}
pub(crate) fn block(events: &[EngineEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.get_details().stage() == &Stage::Environment(EnvironmentStep::Recap))
        .map(|e| e.message(EventMessageVerbosity::SafeOnly))
        .skip_while(|m| !m.starts_with("Why it failed:") && !m.starts_with("Failure observed"))
        .collect()
}
fn pod(status: Value) -> Pod {
    serde_json::from_value(pod_json(status)).unwrap()
}
fn ready_pod() -> Value {
    json!({"phase": "Running", "conditions": [{"type": "Ready", "status": "True"}]})
}
fn collect_test(kube: &kube::Client, kind: WorkloadKind<'_>) -> Vec<String> {
    let (logger, mut receiver) = test_logger();
    let target = Target {
        kube,
        namespace: "ns",
        selector: "service=svc",
        kind,
    };
    for line in block_on(collect(
        &target,
        &logger,
        Instant::now() + Duration::from_millis(200),
        "Why it failed",
    ))
    .lines
    {
        logger.send_recap(line);
    }
    logger.send_error(deployment_error());
    block(&sent(&mut receiver))
}

#[test]
fn live_causes_are_printed_with_status_details() {
    let mut cases = vec![
        (crash(), vec!["CrashLoopBackOff", "exit code 1", "restarts 3"]),
        (
            container(
                json!({"terminated": {"reason": "OOMKilled", "exitCode": 137, "signal": 9}}),
                json!({}),
                2,
            ),
            vec!["OOMKilled", "exit code 137", "signal 9"],
        ),
    ];
    cases[0].0["containerStatuses"][0]["state"]["waiting"]["message"] = json!("界".repeat(500));
    cases[1].0["containerStatuses"][0]["state"]["terminated"]["message"] = json!("界".repeat(500));
    cases[1].0["conditions"] = json!([{"type": "Ready", "status": "True"}]);
    for message in [
        "Insufficient cpu",
        "Insufficient memory",
        "untolerated taint",
        "node affinity mismatch",
    ] {
        cases.push((json!({
            "phase": "Pending",
            "conditions": [{"type": "PodScheduled", "status": "False", "reason": "Unschedulable", "message": message}],
        }), vec![message]));
    }
    for (reason, message) in [
        ("ImagePullBackOff", "image unavailable"),
        ("CreateContainerConfigError", "configmap missing"),
        ("CreateContainerConfigError", "secret missing"),
    ] {
        cases.push((
            container(json!({"waiting": {"reason": reason, "message": message}}), json!({}), 0),
            vec![reason, message],
        ));
    }
    let mut init = crash();
    let mut successful = init["containerStatuses"][0].clone();
    successful["name"] = json!("successful-init");
    successful["state"] = json!({"terminated": {"exitCode": 0}});
    init["initContainerStatuses"] = json!([successful, init["containerStatuses"][0]]);
    init["containerStatuses"][0]["name"] = json!("qovery-wait-container-output");
    cases.push((init, vec!["init container 'api'", "CrashLoopBackOff"]));
    for (status, expected) in cases {
        let (kube, _) = mock_client(move |path, query| match path {
            "pods" => {
                assert!(query.contains("labelSelector=service%3Dsvc"));
                list(vec![pod_json(status.clone())])
            }
            "log" => Reply::Bytes(b"info\nWARNING\n".to_vec()),
            _ => list(vec![]),
        });
        let lines = collect_test(&kube, WorkloadKind::Deployment("v2"));
        for expected in expected {
            assert!(lines[0].contains(expected), "{lines:?}");
        }
        let text = lines.join("\n");
        assert!(
            !text.contains("SPEC-SECRET")
                && !text.contains("successful-init")
                && !text.contains("qovery-wait-container-output")
        );
        assert!(!text.contains("WARNING") && !text.contains("💭 Pod/"));
    }
}

#[test]
fn live_revision_uid_events_are_newest_first_deduplicated_and_supporting() {
    let (kube, _) = mock_client(|path, _| match path {
        "pods" => {
            let mut old = pod_json(crash());
            old["metadata"]["annotations"]["qovery.com/service-version"] = json!("v1");
            list(vec![old])
        }
        "replicasets" => list(vec![json!({
            "metadata": {
                "name": "rs", "uid": "rs-uid",
                "ownerReferences": [{"apiVersion": "apps/v1", "kind": "Deployment", "name": "rs", "uid": "deploy-uid"}],
            },
            "spec": {"selector": {}, "template": {"metadata": {"annotations": {"qovery.com/service-version": "v2"}}}},
        })]),
        _ => list(vec![
            event_json("rs-uid", "ReplicaSet", "older", "2026-01-01T00:00:00Z"),
            event_json("rs-uid", "ReplicaSet", "newer", "2026-01-01T00:00:00.900001Z"),
            event_json("rs-uid", "ReplicaSet", "newer", "2026-01-01T00:00:00.900002Z"),
            event_json("deploy-uid", "Deployment", "owner", "2026-01-01T00:00:00.8Z"),
            event_json("pod-uid", "Pod", "obsolete", "2026-01-01T00:00:01Z"),
            event_json("rs-uid-unrelated", "ReplicaSet", "unrelated", "2026-01-01T00:00:01Z"),
        ]),
    });
    let lines = collect_test(&kube, WorkloadKind::Deployment("v2"));
    assert!(lines[0].contains("no matching live pods"));
    assert_eq!(lines.len(), 4);
    assert!(lines[1].contains("Supporting warning") && lines[1].contains(".900002") && lines[1].ends_with("newer"));
    assert!(lines[2].ends_with("owner") && lines[3].ends_with("older"));
    let (kube, _) = mock_client(|_, _| list(vec![]));
    let lines = collect_test(&kube, WorkloadKind::Pods);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("unavailable") && !lines[0].contains("rollback"));
}

#[test]
fn failed_job_uses_live_owner_and_current_or_previous_logs() {
    for (restarts, terminated, exit_code, reason) in [
        (-1, true, 42, "Error"),
        (0, true, 42, "Error"),
        (2, false, 42, "Error"),
        (2, true, 137, "OOMKilled"),
    ] {
        let (kube, requests) = mock_client(move |path, query| match path {
            "jobs" => list(vec![json!({
                "metadata": {"name": "job", "uid": "job-uid"},
                "status": {"conditions": [{
                    "type": "Failed", "status": if restarts < 0 {"False"} else {"True"},
                    "reason": "BackoffLimitExceeded",
                }]},
            })]),
            "pods" => {
                let exit = json!({"terminated": {"exitCode": exit_code, "reason": reason}});
                let status = if terminated {
                    container(exit, json!({"terminated": {"exitCode": 1}}), restarts.max(0))
                } else {
                    container(json!({"waiting": {"reason": "CrashLoopBackOff"}}), exit, restarts)
                };
                let mut unrelated = pod_json(crash());
                unrelated["metadata"]["ownerReferences"][0]["uid"] = json!("old-job");
                unrelated["status"]["containerStatuses"][0]["name"] = json!("unrelated");
                let mut older = pod_json(container(json!({"terminated": {"exitCode": 13}}), json!({}), 0));
                older["metadata"]["name"] = json!("api-old");
                older["metadata"]["uid"] = json!("old-pod-uid");
                older["metadata"]["creationTimestamp"] = json!("2025-01-01T00:00:00Z");
                list(vec![unrelated, older, pod_json(status)])
            }
            "log" => Reply::Bytes(if query.contains("previous=true") {
                b"eRrOr: previous job failed\n".to_vec()
            } else {
                b"eRrOr: current job failed\n".to_vec()
            }),
            _ => list(vec![]),
        });
        let lines = collect_test(&kube, WorkloadKind::Job);
        assert_eq!(lines[0].contains("BackoffLimitExceeded"), restarts >= 0);
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&format!("{reason} (exit code {exit_code}")))
        );
        let run = if terminated { "current" } else { "previous" };
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Pod/api,") && l.ends_with(&format!("eRrOr: {run} job failed"))),
            "{lines:?}"
        );
        assert!(!lines.join("\n").contains("unrelated"));
        let requests = requests.lock().unwrap();
        let (path, query) = requests.iter().find(|(p, _)| p.ends_with("/log")).unwrap();
        assert!(path.ends_with("/pods/api/log"));
        assert_eq!(query.contains("previous=true"), !terminated && restarts > 0);
    }
}

#[test]
fn denied_malformed_and_hanging_reads_preserve_partial_results() {
    for failure in 0..3 {
        let (kube, requests) = mock_client(move |path, _| {
            if path == "pods" {
                return list(vec![pod_json(crash())]);
            }
            if path == "log" {
                return Reply::Bytes(b"fatal: retained\n".to_vec());
            }
            match failure {
                0 => denied(),
                1 => Reply::Bytes(b"invalid JSON".to_vec()),
                _ => Reply::Hang,
            }
        });
        let start = Instant::now();
        let lines = collect_test(&kube, WorkloadKind::Deployment("v2"));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(lines[0].contains("CrashLoopBackOff"));
        if failure != 2 {
            assert!(lines.last().unwrap().contains("retained"));
        }
        assert!(requests.lock().unwrap().len() <= 4);
    }
    let (kube, _) = mock_client(|_, _| denied());
    assert!(collect_test(&kube, WorkloadKind::Pods).is_empty());
    let (kube, _) = mock_client(|path, _| if path == "pods" { denied() } else { list(vec![]) });
    assert!(collect_test(&kube, WorkloadKind::Pods)[0].contains("could not be read"));
}

#[test]
fn complete_redaction_strict_last_thirty_physical_lines_and_output_bounds() {
    let (kube, requests) = mock_client(|path, _| match path {
        "pods" => list(
            (0..10)
                .map(|i| {
                    let mut pod = pod_json(crash());
                    pod["metadata"]["name"] = json!(format!("api-{i}"));
                    pod
                })
                .collect(),
        ),
        "log" => Reply::Bytes(
            format!(
                "error excluded\n{}eRrOr SECRET\nTOKEN\nFatal {}\nPanic\nException\n{}",
                "\n".repeat(30),
                "界\u{1b}[31m\r\u{2028}".repeat(400),
                "error tail\n".repeat(20)
            )
            .into_bytes(),
        ),
        _ => list(vec![]),
    });
    let lines = collect_test(&kube, WorkloadKind::Pods);
    let text = lines.join("\n");
    assert!(text.contains("eRrOr xxx") && !text.contains("TOKEN") && !text.contains("SECRET"));
    assert!(!text.contains("excluded") && !text.contains("SPEC-SECRET"));
    assert!(text.find("eRrOr").unwrap() < text.find("Fatal").unwrap());
    assert_eq!(lines.len(), 50);
    assert!(lines.iter().all(|l| l.chars().count() <= 300));
    assert!(
        lines
            .iter()
            .all(|l| !l.chars().any(|c| c.is_control() || c == '\u{2028}'))
    );
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/log"))
            .count(),
        4
    );
    let mut last = None;
    let observed = observe(&kube, &[pod(crash())], &mut last);
    let (gone, _) = mock_client(|_, _| list(vec![]));
    let (logger, mut receiver) = test_logger();
    report_failure(
        &Target {
            kube: &gone,
            namespace: "ns",
            selector: "service=svc",
            kind: WorkloadKind::Pods,
        },
        &logger,
        last,
    );
    logger.send_error(deployment_error());
    let replay = block(&sent(&mut receiver));
    assert_eq!(replay.len(), 50);
    assert_eq!(&replay[1..], &observed[1..]);
    assert!(replay[0].contains("final cause unconfirmed"));
    assert!(replay.iter().all(|line| line.chars().count() <= 300));
    assert!(!replay.join("\n").contains("SECRET") && !replay.join("\n").contains("TOKEN"));
    assert!(error_excerpt("info\nWARNING\n").is_empty());
    assert_eq!(
        error_excerpt("terror\nnot fatal\npAnIc\nException"),
        ["    | terror", "    | not fatal", "    | pAnIc", "    | Exception"]
    );
}

#[test]
fn incomplete_or_capped_streams_never_expose_secret_prefixes() {
    let cap = LOG_READ_BYTES as usize;
    assert!(block_on(read_complete(futures::io::Cursor::new(vec![b'a'; cap]))).is_some());
    let cut_secret = format!("{}SECRET\nTOKEN", "error ".repeat(cap / 6));
    assert!(block_on(read_complete(futures::io::Cursor::new(cut_secret.into_bytes()))).is_none());
    let chunks = futures::stream::iter(vec![Ok(b"error SECRET".to_vec()), Err(std::io::Error::other("reset"))]);
    assert!(block_on(read_complete(futures::TryStreamExt::into_async_read(chunks))).is_none());
    let chunks = futures::stream::iter(vec![
        Ok::<_, std::io::Error>(b"error SECRET".to_vec()),
        Ok(b"\nTOKEN".to_vec()),
    ]);
    let bytes = block_on(read_complete(futures::TryStreamExt::into_async_read(chunks))).unwrap();
    let (logger, _) = test_logger();
    assert_eq!(
        error_excerpt(&logger.obfuscate(String::from_utf8(bytes).unwrap())),
        ["    | error xxx"]
    );
    for secret in ["SECRET\nTOKEN", "sec\u{1b}[31mret-token"] {
        let at_cut = safe_line(&logger, format!("{}{secret}", "界".repeat(296)));
        assert!(!at_cut.contains("SEC") && !at_cut.contains("sec"));
    }
    logger.send_success("done".to_string());
    let (sender, mut receiver) = unbounded_channel();
    let logger = UnboundedSenderLogger::new(sender, vec!["x".to_string()]);
    let logger = EnvLogger::new_for_test(Arc::new(Box::new(logger)), event_details(EnvironmentStep::Deploy));
    logger.send_recap(safe_line(&logger, "x".repeat(400)));
    logger.send_success("done".to_string());
    let events = sent(&mut receiver);
    assert!(events[0].message(EventMessageVerbosity::SafeOnly).chars().count() <= 300);
}

#[test]
fn server_tail_cannot_expose_a_multiline_secret_suffix() {
    let complete = format!("SECRET\nTOKEN error: boundary\n{}error: retained\n", "info\n".repeat(28));
    let (kube, requests) = mock_client(move |path, query| match path {
        "pods" => list(vec![pod_json(crash())]),
        "log" => {
            // Model the API cutting the first secret line off a 31-line log.
            let text = if query.contains("tailLines=30") {
                complete.split_once('\n').unwrap().1
            } else {
                &complete
            };
            Reply::Bytes(text.as_bytes().to_vec())
        }
        _ => list(vec![]),
    });
    let lines = collect_test(&kube, WorkloadKind::Pods).join("\n");
    assert!(!lines.contains("TOKEN") && !lines.contains("SECRET"), "{lines}");
    assert!(lines.contains("xxx error: boundary") && lines.contains("error: retained"));
    let requests = requests.lock().unwrap();
    let (_, query) = requests.iter().find(|(path, _)| path.ends_with("/log")).unwrap();
    assert!(!query.contains("tailLines=") && query.contains("limitBytes=65537"));
}

fn observe(kube: &kube::Client, pods: &[Pod], last: &mut Option<ObservedFailure>) -> Vec<String> {
    let (logger, mut receiver) = test_logger();
    let target = Target {
        kube,
        namespace: "ns",
        selector: "service=svc",
        kind: WorkloadKind::Deployment("v2"),
    };
    report_observed_failure(&target, &logger, None, pods, last);
    logger.send_error(deployment_error());
    block(&sent(&mut receiver))
}

#[test]
fn observed_failure_is_printed_once_until_its_signature_changes() {
    let (kube, requests) = mock_client(|path, _| match path {
        "pods" => list(vec![pod_json(crash())]),
        "log" => Reply::Bytes(b"Error SECRET\nTOKEN\n".to_vec()),
        _ => list(vec![]),
    });
    let mut last = None;
    let lines = observe(&kube, &[pod(crash())], &mut last);
    assert!(lines[0].starts_with("Failure observed during deployment:") && lines[0].contains("CrashLoopBackOff"));
    assert!(lines.last().unwrap().contains("Error xxx"), "{lines:?}");
    let calls = requests.lock().unwrap().len();
    assert!(observe(&kube, &[pod(crash())], &mut last).is_empty());
    assert_eq!(requests.lock().unwrap().len(), calls);
    let mut restarted = crash();
    restarted["containerStatuses"][0]["restartCount"] = json!(4);
    let restarted_pod = pod(restarted.clone());
    let (changed_kube, _) = mock_client(move |path, _| match path {
        "pods" => list(vec![pod_json(restarted.clone())]),
        "log" => Reply::Bytes(b"Error latest observation\n".to_vec()),
        _ => list(vec![]),
    });
    let latest = observe(&changed_kube, &[restarted_pod], &mut last);
    assert_eq!(
        last.as_ref().map(|last| last.signature.as_str()),
        Some("pod-uid/api/4/CrashLoopBackOff")
    );
    let (gone, _) = mock_client(|_, _| list(vec![]));
    let (logger, mut receiver) = test_logger();
    report_failure(
        &Target {
            kube: &gone,
            namespace: "ns",
            selector: "service=svc",
            kind: WorkloadKind::Pods,
        },
        &logger,
        last,
    );
    logger.send_error(deployment_error());
    let replay = block(&sent(&mut receiver));
    assert_eq!(&replay[1..], &latest[1..]);
    assert!(replay.last().unwrap().contains("latest observation"));
    assert!(!replay.join("\n").contains("Error xxx"));
}

#[test]
fn healthy_old_version_and_terminating_pods_are_not_observed_as_failures() {
    let (kube, requests) = mock_client(|_, _| list(vec![]));
    let mut old = pod(crash());
    old.metadata.annotations = Some([("qovery.com/service-version".to_string(), "v1".to_string())].into());
    let mut terminating = pod_json(crash());
    terminating["metadata"]["deletionTimestamp"] = json!("2026-01-01T00:00:00Z");
    let terminating = serde_json::from_value(terminating).unwrap();
    let mut last = None;
    assert!(observe(&kube, &[pod(ready_pod()), old, terminating], &mut last).is_empty());
    assert!(last.is_none() && requests.lock().unwrap().is_empty());
}

#[test]
fn unreadable_evidence_prints_nothing_and_is_retried_on_the_next_observation() {
    let (kube, _) = mock_client(|_, _| denied());
    let mut last = None;
    assert!(observe(&kube, &[pod(crash())], &mut last).is_empty());
    assert!(last.is_none());
}

#[test]
fn a_stale_job_pod_cannot_trigger_a_block_for_the_live_job() {
    let (kube, requests) = mock_client(|path, _| match path {
        "pods" => list(vec![pod_json(crash())]),
        "jobs" => list(vec![json!({"metadata": {"name": "job", "uid": "job-uid"}})]),
        "log" => Reply::Bytes(vec![]),
        _ => list(vec![]),
    });
    let job: Job = serde_json::from_value(json!({"metadata": {"name": "job", "uid": "job-uid"}})).unwrap();
    let target = Target {
        kube: &kube,
        namespace: "ns",
        selector: "service=svc",
        kind: WorkloadKind::Job,
    };
    let mut last = None;
    for (field, value) in [("uid", "old-job-uid"), ("name", "old-job"), ("kind", "ReplicaSet")] {
        let mut stale = pod_json(crash());
        stale["metadata"]["ownerReferences"][0][field] = json!(value);
        let stale = serde_json::from_value(stale).unwrap();
        let (logger, mut receiver) = test_logger();
        report_observed_failure(&target, &logger, Some(&job), &[stale], &mut last);
        logger.send_error(deployment_error());
        assert!(block(&sent(&mut receiver)).is_empty() && last.is_none(), "{field}");
        assert!(requests.lock().unwrap().is_empty());
    }

    let (logger, mut receiver) = test_logger();
    report_observed_failure(&target, &logger, Some(&job), &[pod(crash())], &mut last);
    logger.send_error(deployment_error());
    assert!(block(&sent(&mut receiver))[0].starts_with("Failure observed"));
    assert!(last.is_some());
}

#[test]
fn final_failure_prefers_live_details_and_otherwise_replays_the_latest_observation() {
    for final_state in ["removed", "denied", "healthy", "changed"] {
        let (observed_kube, _) = mock_client(|path, _| match path {
            "pods" => list(vec![pod_json(crash())]),
            "log" => Reply::Bytes(b"Error observed A\n".to_vec()),
            _ => list(vec![]),
        });
        let mut last = None;
        let observed = observe(&observed_kube, &[pod(crash())], &mut last);
        let (kube, requests) = mock_client(move |path, _| match (final_state, path) {
            ("denied", _) => denied(),
            ("healthy", "pods") => list(vec![pod_json(ready_pod())]),
            ("changed", "pods") => list(vec![pod_json(container(
                json!({"terminated": {"reason": "OOMKilled", "exitCode": 137}}),
                json!({}),
                4,
            ))]),
            ("changed", "log") => Reply::Bytes(b"Fatal final B\n".to_vec()),
            _ => list(vec![]),
        });
        let (logger, mut receiver) = test_logger();
        let target = Target {
            kube: &kube,
            namespace: "ns",
            selector: "service=svc",
            kind: WorkloadKind::Deployment("v2"),
        };
        report_failure(&target, &logger, last);
        logger.send_error(deployment_error());
        let events = sent(&mut receiver);
        let lines = block(&events);
        assert!(!requests.lock().unwrap().is_empty(), "{final_state}");
        assert!(!lines.is_empty(), "{final_state}");
        assert_eq!(lines.iter().filter(|line| line.starts_with("Why it failed:")).count(), 1);
        assert!(matches!(events.last().unwrap(), EngineEvent::Error(..)));
        if final_state == "changed" {
            let text = lines.join("\n");
            assert!(text.contains("OOMKilled") && text.contains("Fatal final B"));
            assert!(!text.contains("observed A") && !text.contains("final cause unconfirmed"));
        } else {
            assert!(
                lines[0].contains("earlier deployment observation") && lines[0].contains("final cause unconfirmed")
            );
            assert_eq!(&lines[1..], &observed[1..]);
        }
    }
}

#[test]
fn failed_collection_preserves_observed_evidence_and_retries_a_changed_signature() {
    let (kube, _) = mock_client(|path, _| match path {
        "pods" => list(vec![pod_json(crash())]),
        "log" => Reply::Bytes(b"Error retained\n".to_vec()),
        _ => list(vec![]),
    });
    let mut last = None;
    let observed = observe(&kube, &[pod(crash())], &mut last);
    let mut changed = crash();
    changed["containerStatuses"][0]["restartCount"] = json!(4);
    for partial in [false, true] {
        let (unreadable, _) = mock_client(move |path, _| {
            if partial && path != "pods" {
                list(vec![])
            } else {
                denied()
            }
        });
        assert!(observe(&unreadable, &[pod(changed.clone())], &mut last).is_empty());
        assert_eq!(last.as_ref().unwrap().signature, "pod-uid/api/3/CrashLoopBackOff");
        assert_eq!(last.as_ref().unwrap().lines, observed);
    }
    let changed_pod = pod(changed.clone());
    let (readable, _) = mock_client(move |path, _| match path {
        "pods" => list(vec![pod_json(changed.clone())]),
        "log" => Reply::Bytes(b"Error retry succeeded\n".to_vec()),
        _ => list(vec![]),
    });
    let retried = observe(&readable, &[changed_pod], &mut last);
    assert!(retried.last().unwrap().contains("retry succeeded"));
    assert_eq!(
        last.as_ref().map(|last| last.signature.as_str()),
        Some("pod-uid/api/4/CrashLoopBackOff")
    );
}
