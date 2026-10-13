use super::config::{ContainerSpec, RESTART_POLICY_LABEL};
use super::docker::DockerOperation;
use super::fixtures::{CLUSTER_ID, ORGANIZATION_ID, container, identity, image, readme, running};
use super::model::{ImageInspect, PlatformOs, RestartPolicyName};
use super::*;
use crate::cmd::docker::Architecture;
use crate::errors::Tag;
use crate::events::{EngineEvent, EventMessage, EventMessageVerbosity, InfrastructureDiffType, Stage, Transmitter};
use crate::io_models::QoveryIdentifier;
use crate::io_models::platform_components::{ExecutionMode, OperatorContainerUpdateUnit, PlatformExecutionResult};
use crate::logger::UnboundedSenderLogger;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::unbounded_channel;
use uuid::Uuid;

const TARGET: &str = "localhost:5000/qovery-operator:n1";
const CRASHING_TARGET: &str = "localhost:5000/qovery-operator:crash";
const NAME: &str = "qovery-operator";
const ALWAYS: &str = r#"{"Name":"always","MaximumRetryCount":0}"#;

fn pinned(digest_byte: char) -> String {
    format!("localhost:5000/qovery-operator@sha256:{}", digest_byte.to_string().repeat(64))
}

struct TestLogger;

impl InfraLogger for TestLogger {
    fn info(&self, message: impl Into<EventMessage>) {
        println!("{}", message.into());
    }

    fn warn(&self, message: impl Into<EventMessage>) {
        println!("{}", message.into());
    }

    fn error(&self, error: EngineError, _message: Option<impl Into<EventMessage>>) {
        println!("{error:?}");
    }

    fn diff(&self, _from: InfrastructureDiffType, _message: String) {}
}

fn target_image(repo_digest: &str, default_value: &str) -> ImageInspect {
    let mut target = image();
    target.repo_digests = vec![repo_digest.to_string()];
    target.config.env.0 = vec![
        "PATH=/bin".to_string(),
        format!("QOVERY_FIXTURE_DEFAULT={default_value}"),
    ];
    target
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureTiming {
    BeforeActing,
    AfterActing,
}

struct FakeState {
    platform: Option<Platform>,
    containers: Vec<ContainerInspect>,
    images: HashMap<String, ImageInspect>,
    crashing_images: HashSet<String>,
    failing: Option<(DockerOperation, FailureTiming)>,
    repeated_failures: usize,
    failing_inspect: Option<ContainerId>,
    on_pull: Option<fn(&mut FakeState)>,
    calls: Vec<String>,
    next_id: u64,
}

/// An in-memory Docker daemon: containers move between states the way `docker` moves them.
struct FakeDocker {
    state: RefCell<FakeState>,
}

impl FakeDocker {
    fn new(containers: Vec<ContainerInspect>) -> Self {
        let old_image_id = readme().image.as_str().to_string();
        let healthy_target = target_image(&pinned('1'), "image-n1-value");
        let crashing_target = target_image(&pinned('2'), "crash");
        FakeDocker {
            state: RefCell::new(FakeState {
                platform: Some(Platform::parse("linux", "arm64").unwrap()),
                containers,
                images: HashMap::from([
                    (old_image_id, image()),
                    (TARGET.to_string(), healthy_target.clone()),
                    (pinned('1'), healthy_target),
                    (CRASHING_TARGET.to_string(), crashing_target.clone()),
                    (pinned('2'), crashing_target),
                ]),
                crashing_images: HashSet::from([pinned('2')]),
                failing: None,
                repeated_failures: 0,
                failing_inspect: None,
                on_pull: None,
                calls: Vec::new(),
                next_id: 1,
            }),
        }
    }

    fn failing(self, operation: DockerOperation, timing: FailureTiming) -> Self {
        self.state.borrow_mut().failing = Some((operation, timing));
        self
    }

    fn calls(&self) -> Vec<String> {
        self.state.borrow().calls.clone()
    }

    fn named(&self, name: &str) -> Option<ContainerInspect> {
        self.state
            .borrow()
            .containers
            .iter()
            .find(|container| container.name.as_str() == name)
            .cloned()
    }

    fn name_of(&self, id: &ContainerId) -> String {
        self.state
            .borrow()
            .containers
            .iter()
            .find(|container| &container.id == id)
            .map(|container| container.name.to_string())
            .unwrap_or_else(|| id.to_string())
    }

    /// Records the call, then runs `act` unless an injected failure fires before it; an injected
    /// failure after acting returns an error although the daemon changed its state.
    fn mutate<T>(
        &self,
        operation: DockerOperation,
        call: String,
        act: impl FnOnce(&mut FakeState) -> Result<T, DockerFailure>,
    ) -> Result<T, DockerFailure> {
        let injected = DockerFailure {
            operation,
            message: "injected failure".to_string(),
        };
        let mut state = self.state.borrow_mut();
        state.calls.push(call);
        let timing = match state.failing {
            Some((failing, timing)) if failing == operation => {
                match state.repeated_failures {
                    0 => state.failing = None,
                    _ => state.repeated_failures -= 1,
                }
                Some(timing)
            }
            _ => None,
        };
        if timing == Some(FailureTiming::BeforeActing) {
            return Err(injected);
        }
        let result = act(&mut state)?;
        match timing {
            Some(FailureTiming::AfterActing) => Err(injected),
            _ => Ok(result),
        }
    }
}

fn find<'a>(state: &'a mut FakeState, id: &ContainerId) -> Result<&'a mut ContainerInspect, DockerFailure> {
    state
        .containers
        .iter_mut()
        .find(|container| &container.id == id)
        .ok_or_else(|| DockerFailure {
            operation: DockerOperation::Inspect,
            message: format!("No such container: {id}"),
        })
}

impl OperatorDocker for FakeDocker {
    fn host_platform(&self) -> Result<Platform, DockerFailure> {
        self.state.borrow().platform.ok_or_else(|| DockerFailure {
            operation: DockerOperation::Version,
            message: "Cannot connect to the Docker daemon".to_string(),
        })
    }

    fn list_running_containers(&self) -> Result<Vec<ContainerInspect>, DockerFailure> {
        Ok(self
            .state
            .borrow()
            .containers
            .iter()
            .filter(|container| container.state.status == ContainerStatus::Running)
            .cloned()
            .collect())
    }

    fn find_containers_named(&self, names: &[ContainerName]) -> Result<Vec<ContainerInspect>, DockerFailure> {
        Ok(self
            .state
            .borrow()
            .containers
            .iter()
            .filter(|container| names.contains(&container.name))
            .cloned()
            .collect())
    }

    fn inspect_container(&self, id: &ContainerId) -> Result<ContainerInspect, DockerFailure> {
        let mut state = self.state.borrow_mut();
        if state.failing_inspect.as_ref() == Some(id) {
            state.failing_inspect = None;
            return Err(DockerFailure {
                operation: DockerOperation::Inspect,
                message: "injected failure".to_string(),
            });
        }
        find(&mut state, id).map(|container| container.clone())
    }

    fn container_exists(&self, id: &ContainerId) -> Result<bool, DockerFailure> {
        Ok(self
            .state
            .borrow()
            .containers
            .iter()
            .any(|container| &container.id == id))
    }

    fn pull_image(&self, image: &ImageReference) -> Result<(), DockerFailure> {
        self.mutate(DockerOperation::Pull, format!("pull {image}"), |state| {
            if let Some(on_pull) = state.on_pull.take() {
                on_pull(state);
            }
            Ok(())
        })
    }

    fn inspect_image(&self, image: ImageLookup<'_>) -> Result<ImageInspect, DockerFailure> {
        self.state
            .borrow()
            .images
            .get(image.as_str())
            .cloned()
            .ok_or_else(|| DockerFailure {
                operation: DockerOperation::Inspect,
                message: format!("No such image: {}", image.as_str()),
            })
    }

    fn create_container(&self, spec: &ContainerSpec) -> Result<ContainerId, DockerFailure> {
        let image = self
            .state
            .borrow()
            .images
            .get(spec.image.as_str())
            .cloned()
            .ok_or_else(|| DockerFailure {
                operation: DockerOperation::Create,
                message: format!("No such image: {}", spec.image),
            })?;
        self.mutate(DockerOperation::Create, format!("create {}", spec.name), |state| {
            if state.containers.iter().any(|container| container.name == spec.name) {
                return Err(DockerFailure {
                    operation: DockerOperation::Create,
                    message: format!("Conflict. The container name {} is already in use", spec.name),
                });
            }
            let id = format!("{:064x}", 0xfeed_0000_u64 + state.next_id);
            state.next_id += 1;
            let mut created = readme();
            created.id = ContainerId::from(id.as_str());
            created.name = spec.name.clone();
            created.image = serde_json::from_value(json!(spec.image.as_str())).unwrap();
            created.state.status = ContainerStatus::Created;
            created.config.hostname = id[..12].to_string();
            created.config.env.0 = spec
                .env
                .iter()
                .map(|variable| format!("{}={}", variable.name, variable.value))
                .chain(image.config.env.entries().iter().cloned())
                .collect();
            created.config.labels = spec.labels.clone();
            created.host_config.restart_policy = spec.restart_policy;
            state.containers.push(created);
            Ok(ContainerId::from(id.as_str()))
        })
    }

    fn update_restart_policy(&self, id: &ContainerId, policy: RestartPolicy) -> Result<(), DockerFailure> {
        let call = format!("update-restart {} {policy}", self.name_of(id));
        self.mutate(DockerOperation::UpdateRestartPolicy, call, |state| {
            find(state, id).map(|container| container.host_config.restart_policy = policy)
        })
    }

    fn stop_container(&self, id: &ContainerId) -> Result<(), DockerFailure> {
        let call = format!("stop {}", self.name_of(id));
        self.mutate(DockerOperation::Stop, call, |state| {
            find(state, id).map(|container| container.state.status = ContainerStatus::Exited)
        })
    }

    fn rename_container(&self, id: &ContainerId, name: &ContainerName) -> Result<(), DockerFailure> {
        let call = format!("rename {} -> {name}", self.name_of(id));
        self.mutate(DockerOperation::Rename, call, |state| {
            if state.containers.iter().any(|container| &container.name == name) {
                return Err(DockerFailure {
                    operation: DockerOperation::Rename,
                    message: format!("Conflict. The container name {name} is already in use"),
                });
            }
            find(state, id).map(|container| container.name = name.clone())
        })
    }

    fn start_container(&self, id: &ContainerId) -> Result<(), DockerFailure> {
        let call = format!("start {}", self.name_of(id));
        self.mutate(DockerOperation::Start, call, |state| {
            let image = find(state, id)?.image.as_str().to_string();
            let crashes = state.crashing_images.contains(&image);
            let container = find(state, id)?;
            match (crashes, container.host_config.restart_policy.name) {
                (false, _) => container.state.status = ContainerStatus::Running,
                (true, RestartPolicyName::No) => container.state.status = ContainerStatus::Exited,
                (true, _) => {
                    container.state.status = ContainerStatus::Running;
                    container.restart_count += 1;
                }
            }
            Ok(())
        })
    }

    fn remove_container(&self, id: &ContainerId) -> Result<(), DockerFailure> {
        let call = format!("rm {}", self.name_of(id));
        self.mutate(DockerOperation::Remove, call, |state| {
            find(state, id)?;
            state.containers.retain(|container| &container.id != id);
            Ok(())
        })
    }
}

fn plan(image: &str) -> UpdatePlan<'static> {
    UpdatePlan {
        identity: identity(),
        image: ImageReference::parse(image).unwrap(),
        operator_version: OperatorVersion::parse("n1").unwrap(),
        execution_id: "exec-42",
        health: HealthCheck {
            window: Duration::ZERO,
            poll_interval: Duration::ZERO,
        },
    }
}

fn readme_with_policy(policy: &str) -> ContainerInspect {
    let mut operator = readme();
    operator.host_config.restart_policy = serde_json::from_str(policy).unwrap();
    operator
}

fn is_mutation(call: &str) -> bool {
    ["create", "update-restart", "stop", "rename", "start", "rm"]
        .iter()
        .any(|prefix| call.starts_with(prefix))
}

fn assert_original_operator_restored(docker: &FakeDocker, operator: &ContainerInspect, case: &str) {
    let restored = docker.named(NAME).unwrap_or_else(|| panic!("{case}: no `{NAME}`"));
    assert_eq!(restored.id, operator.id, "{case}");
    assert_eq!(restored.state.status, ContainerStatus::Running, "{case}");
    assert_eq!(
        restored.host_config.restart_policy, operator.host_config.restart_policy,
        "{case}"
    );
    assert!(docker.named("qovery-operator-next").is_none(), "{case}");
    assert!(docker.named("qovery-operator-previous").is_none(), "{case}");
}

fn leftover(name: &str, id_suffix: &str, status: ContainerStatus) -> ContainerInspect {
    let mut container = readme();
    container.id = ContainerId::from(format!("{:0>64}", id_suffix).as_str());
    container.name = ContainerName::from(name);
    container.state.status = status;
    container
}

#[test]
fn successful_update_replaces_the_operator_and_keeps_the_previous_container_stopped() {
    let operator = readme();
    let worker = container("container_worker");
    let docker = FakeDocker::new(vec![worker.clone(), operator.clone()]);

    let result = run_update(&docker, &TestLogger, &plan(TARGET));

    assert_eq!(result.unwrap().as_str(), pinned('1'));
    assert_eq!(
        docker.calls(),
        [
            "pull localhost:5000/qovery-operator:n1",
            "create qovery-operator-next",
            "stop qovery-operator",
            "rename qovery-operator -> qovery-operator-previous",
            "rename qovery-operator-next -> qovery-operator",
            "start qovery-operator",
        ]
    );
    let updated = docker.named(NAME).unwrap();
    assert_eq!(updated.image.as_str(), pinned('1'));
    assert_eq!(updated.state.status, ContainerStatus::Running);
    assert_eq!(updated.config.env.value("OPERATOR_VERSION"), Some("n1"));
    assert_eq!(updated.config.env.value("CLUSTER_JWT_TOKEN"), Some("fixture-jwt"));
    assert_eq!(updated.config.env.value("QOVERY_FIXTURE_DEFAULT"), Some("image-n1-value"));
    assert_eq!(
        updated.config.labels.get(RESTART_POLICY_LABEL).map(String::as_str),
        Some("unless-stopped")
    );
    let previous = docker.named("qovery-operator-previous").unwrap();
    assert_eq!(previous.id, operator.id);
    assert_eq!(previous.state.status, ContainerStatus::Exited);
    let untouched_worker = docker.named("fx-worker").unwrap();
    assert_eq!(untouched_worker.id, worker.id);
    assert_eq!(untouched_worker.state.status, ContainerStatus::Running);
}

#[test]
fn restart_policy_is_set_to_no_only_when_it_would_revive_the_stopped_container() {
    for (policy, neutralised) in [
        (ALWAYS, true),
        (r#"{"Name":"on-failure","MaximumRetryCount":3}"#, true),
        (r#"{"Name":"unless-stopped","MaximumRetryCount":0}"#, false),
        (r#"{"Name":"no","MaximumRetryCount":0}"#, false),
    ] {
        let operator = readme_with_policy(policy);
        let original_policy = operator.host_config.restart_policy;
        let docker = FakeDocker::new(vec![operator]);

        run_update(&docker, &TestLogger, &plan(TARGET)).unwrap();

        let mutations: Vec<String> = docker.calls().into_iter().filter(|call| is_mutation(call)).collect();
        let mut expected = vec!["create qovery-operator-next".to_string()];
        if neutralised {
            expected.push("update-restart qovery-operator no".to_string());
        }
        expected.extend(
            [
                "stop qovery-operator",
                "rename qovery-operator -> qovery-operator-previous",
                "rename qovery-operator-next -> qovery-operator",
                "start qovery-operator",
            ]
            .map(str::to_string),
        );
        assert_eq!(mutations, expected, "policy {policy}");
        assert_eq!(
            docker.named(NAME).unwrap().host_config.restart_policy,
            original_policy,
            "policy {policy}"
        );
        let expected_previous_policy = if neutralised {
            RestartPolicy::NO
        } else {
            original_policy
        };
        assert_eq!(
            docker
                .named("qovery-operator-previous")
                .unwrap()
                .host_config
                .restart_policy,
            expected_previous_policy,
            "policy {policy}"
        );
    }
}

#[test]
fn unhealthy_target_is_rolled_back_to_the_previous_container() {
    for (policy, restore_policy_calls) in [
        (
            ALWAYS,
            vec![
                "update-restart qovery-operator no",
                "update-restart qovery-operator-previous always",
            ],
        ),
        (r#"{"Name":"no","MaximumRetryCount":0}"#, vec![]),
    ] {
        let operator = readme_with_policy(policy);
        let worker = container("container_worker");
        let docker = FakeDocker::new(vec![worker.clone(), operator.clone()]);

        let failure = run_update(&docker, &TestLogger, &plan(CRASHING_TARGET)).unwrap_err();

        assert_eq!(failure.kind, UpdateFailureKind::RolledBack, "policy {policy}");
        let mut expected = vec![
            "pull localhost:5000/qovery-operator:crash",
            "create qovery-operator-next",
        ];
        expected.extend(restore_policy_calls.first());
        expected.extend([
            "stop qovery-operator",
            "rename qovery-operator -> qovery-operator-previous",
            "rename qovery-operator-next -> qovery-operator",
            "start qovery-operator",
            "rm qovery-operator",
        ]);
        expected.extend(restore_policy_calls.get(1));
        expected.extend([
            "rename qovery-operator-previous -> qovery-operator",
            "start qovery-operator",
        ]);
        assert_eq!(docker.calls(), expected, "policy {policy}");
        assert_original_operator_restored(&docker, &operator, policy);
        assert_eq!(docker.named("fx-worker").unwrap().id, worker.id);
    }
}

#[test]
fn rollback_reads_the_daemon_state_when_a_cut_over_command_fails_after_acting() {
    for (operation, timing) in [
        (DockerOperation::UpdateRestartPolicy, FailureTiming::AfterActing),
        (DockerOperation::UpdateRestartPolicy, FailureTiming::BeforeActing),
        (DockerOperation::Stop, FailureTiming::AfterActing),
        (DockerOperation::Stop, FailureTiming::BeforeActing),
        (DockerOperation::Rename, FailureTiming::AfterActing),
        (DockerOperation::Rename, FailureTiming::BeforeActing),
        (DockerOperation::Start, FailureTiming::AfterActing),
    ] {
        let case = format!("{operation} {timing:?}");
        let operator = readme_with_policy(ALWAYS);
        let docker = FakeDocker::new(vec![operator.clone()]).failing(operation, timing);

        let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

        assert_eq!(failure.kind, UpdateFailureKind::RolledBack, "{case}: {}", failure.message);
        assert_original_operator_restored(&docker, &operator, &case);
    }
}

#[test]
fn failed_rollback_never_leaves_a_policy_that_would_revive_both_operators() {
    for (policy, expected_previous_policy, expected_hint) in [
        (ALWAYS, "no", Some("restore the `always` restart policy")),
        (
            r#"{"Name":"on-failure","MaximumRetryCount":3}"#,
            "no",
            Some("restore the `on-failure:3` restart policy"),
        ),
        (r#"{"Name":"unless-stopped","MaximumRetryCount":0}"#, "unless-stopped", None),
    ] {
        let operator = readme_with_policy(policy);
        let docker =
            FakeDocker::new(vec![operator.clone()]).failing(DockerOperation::Remove, FailureTiming::BeforeActing);
        docker.state.borrow_mut().crashing_images.insert(pinned('1'));

        let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

        assert_eq!(failure.kind, UpdateFailureKind::RollbackFailed, "{policy}");
        assert!(failure.message.contains("manual recovery required"), "{policy}");
        match expected_hint {
            Some(hint) => assert!(failure.message.contains(hint), "{policy}: {}", failure.message),
            None => assert!(!failure.message.contains("restart policy"), "{policy}: {}", failure.message),
        }
        let previous = docker.named("qovery-operator-previous").unwrap();
        assert_eq!(previous.id, operator.id, "{policy}");
        assert_eq!(
            previous.host_config.restart_policy.to_string(),
            expected_previous_policy,
            "{policy}"
        );
        assert_eq!(previous.state.status, ContainerStatus::Exited, "{policy}");
    }
}

#[test]
fn rollback_completes_when_docker_removed_the_new_container_but_reported_an_error() {
    let operator = readme_with_policy(ALWAYS);
    let docker = FakeDocker::new(vec![operator.clone()]).failing(DockerOperation::Remove, FailureTiming::AfterActing);
    docker.state.borrow_mut().crashing_images.insert(pinned('1'));

    let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

    assert_eq!(failure.kind, UpdateFailureKind::RolledBack, "{}", failure.message);
    assert_original_operator_restored(&docker, &operator, "rm error after acting");
}

#[test]
fn digest_request_absent_from_the_pulled_image_is_refused_before_anything_changes() {
    let requested = format!("localhost:5000/qovery-operator@sha256:{}", "3".repeat(64));
    let docker = FakeDocker::new(vec![readme()]);
    let target = docker.state.borrow().images[TARGET].clone();
    docker.state.borrow_mut().images.insert(requested.clone(), target);

    let failure = run_update(&docker, &TestLogger, &plan(&requested)).unwrap_err();

    assert_eq!(failure.kind, UpdateFailureKind::ImagePullFailed, "{}", failure.message);
    assert!(failure.message.contains("requested digest"), "{}", failure.message);
    assert!(docker.calls().iter().all(|call| !is_mutation(call)), "{:?}", docker.calls());
}

#[test]
fn failed_rollback_still_restores_a_policy_that_keeps_the_old_container_stopped() {
    let operator = readme_with_policy(r#"{"Name":"unless-stopped","MaximumRetryCount":0}"#);
    let docker = FakeDocker::new(vec![operator.clone()]).failing(DockerOperation::Remove, FailureTiming::BeforeActing);
    docker.state.borrow_mut().crashing_images.insert(pinned('1'));
    docker.state.borrow_mut().failing_inspect = Some(operator.id.clone());

    let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

    assert_eq!(failure.kind, UpdateFailureKind::RollbackFailed);
    assert!(
        docker
            .calls()
            .contains(&"update-restart qovery-operator-previous unless-stopped".to_string()),
        "{:?}",
        docker.calls()
    );
    assert!(!failure.message.contains("restart policy"), "{}", failure.message);
}

#[test]
fn rollback_reports_a_failure_when_the_old_container_cannot_be_inspected_but_still_restores_it() {
    let operator = readme_with_policy(ALWAYS);
    let docker = FakeDocker::new(vec![operator.clone()]).failing(DockerOperation::Start, FailureTiming::BeforeActing);
    docker.state.borrow_mut().failing_inspect = Some(operator.id.clone());

    let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

    assert_eq!(failure.kind, UpdateFailureKind::RollbackFailed, "{}", failure.message);
    assert_original_operator_restored(&docker, &operator, "inspect failure");
}

#[test]
fn leftover_next_is_removed_before_pulling_and_previous_just_before_the_cut_over() {
    let operator = readme();
    let mut leftover_next = leftover("qovery-operator-next", "abc", ContainerStatus::Created);
    leftover_next
        .config
        .labels
        .insert(UPDATE_LABEL.to_string(), "interrupted-exec".to_string());
    let stale_previous = leftover("qovery-operator-previous", "def", ContainerStatus::Exited);
    let docker = FakeDocker::new(vec![operator.clone(), leftover_next, stale_previous]);

    run_update(&docker, &TestLogger, &plan(TARGET)).unwrap();

    assert_eq!(
        docker.calls(),
        [
            "rm qovery-operator-next",
            "pull localhost:5000/qovery-operator:n1",
            "create qovery-operator-next",
            "rm qovery-operator-previous",
            "stop qovery-operator",
            "rename qovery-operator -> qovery-operator-previous",
            "rename qovery-operator-next -> qovery-operator",
            "start qovery-operator",
        ]
    );
    assert_eq!(docker.named("qovery-operator-previous").unwrap().id, operator.id);
}

fn operator_with_stale_previous() -> (ContainerInspect, ContainerInspect) {
    (readme(), leftover("qovery-operator-previous", "def", ContainerStatus::Exited))
}

fn assert_operator_and_previous_untouched(
    docker: &FakeDocker,
    operator: &ContainerInspect,
    previous: &ContainerInspect,
    case: &str,
) {
    let running = docker.named(NAME).unwrap_or_else(|| panic!("{case}: no `{NAME}`"));
    assert_eq!(running.id, operator.id, "{case}");
    assert_eq!(running.state.status, ContainerStatus::Running, "{case}");
    let kept = docker
        .named("qovery-operator-previous")
        .unwrap_or_else(|| panic!("{case}: `qovery-operator-previous` was removed"));
    assert_eq!(kept.id, previous.id, "{case}");
    assert!(docker.named("qovery-operator-next").is_none(), "{case}");
    assert!(
        !docker.calls().contains(&"rm qovery-operator-previous".to_string()),
        "{case}: {:?}",
        docker.calls()
    );
}

#[test]
fn failures_before_the_cut_over_keep_the_previous_operator() {
    struct Case {
        name: &'static str,
        prepare: fn(&FakeDocker),
        expected: UpdateFailureKind,
    }
    for case in [
        Case {
            name: "pull failure",
            prepare: |docker| {
                docker.state.borrow_mut().failing = Some((DockerOperation::Pull, FailureTiming::BeforeActing))
            },
            expected: UpdateFailureKind::ImagePullFailed,
        },
        Case {
            name: "platform mismatch",
            prepare: |docker| {
                docker.state.borrow_mut().platform = Some(Platform {
                    os: PlatformOs::Linux,
                    architecture: Architecture::AMD64,
                })
            },
            expected: UpdateFailureKind::ImagePlatformMismatch,
        },
        Case {
            name: "create failure",
            prepare: |docker| {
                docker.state.borrow_mut().failing = Some((DockerOperation::Create, FailureTiming::BeforeActing))
            },
            expected: UpdateFailureKind::DockerOperationFailed,
        },
    ] {
        let (operator, previous) = operator_with_stale_previous();
        let docker = FakeDocker::new(vec![operator.clone(), previous.clone()]);
        (case.prepare)(&docker);

        let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

        assert_eq!(failure.kind, case.expected, "{}: {}", case.name, failure.message);
        assert_operator_and_previous_untouched(&docker, &operator, &previous, case.name);
    }
}

#[test]
fn failed_removal_of_the_previous_operator_aborts_before_stopping_the_old_one() {
    let (operator, previous) = operator_with_stale_previous();
    let docker = FakeDocker::new(vec![operator.clone(), previous.clone()])
        .failing(DockerOperation::Remove, FailureTiming::BeforeActing);

    let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

    assert_eq!(failure.kind, UpdateFailureKind::DockerOperationFailed, "{}", failure.message);
    assert_eq!(
        docker.calls(),
        [
            "pull localhost:5000/qovery-operator:n1",
            "create qovery-operator-next",
            "rm qovery-operator-previous",
            "rm qovery-operator-next",
        ]
    );
    let running = docker.named(NAME).unwrap();
    assert_eq!(running.id, operator.id);
    assert_eq!(running.state.status, ContainerStatus::Running);
    assert_eq!(docker.named("qovery-operator-previous").unwrap().id, previous.id);
    assert!(docker.named("qovery-operator-next").is_none());
}

fn start_previous_during_the_pull(docker: &FakeDocker) {
    docker.state.borrow_mut().on_pull = Some(|state| {
        let previous = state
            .containers
            .iter_mut()
            .find(|container| container.name.as_str() == "qovery-operator-previous")
            .unwrap();
        previous.state.status = ContainerStatus::Running;
    });
}

#[test]
fn previous_operator_started_during_the_pull_is_refused_and_kept() {
    let (operator, previous) = operator_with_stale_previous();
    let docker = FakeDocker::new(vec![operator.clone(), previous.clone()]);
    start_previous_during_the_pull(&docker);

    let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

    assert_eq!(failure.kind, UpdateFailureKind::UnsupportedConfiguration, "{}", failure.message);
    assert_operator_and_previous_untouched(&docker, &operator, &previous, "started during the pull");
    assert_eq!(
        docker.named("qovery-operator-previous").unwrap().state.status,
        ContainerStatus::Running
    );
}

#[test]
fn interrupted_cut_over_restores_the_restart_policy_recorded_on_the_next_container() {
    let operator = readme_with_policy(r#"{"Name":"no","MaximumRetryCount":0}"#);
    let mut leftover_next = leftover("qovery-operator-next", "abc", ContainerStatus::Created);
    leftover_next.config.labels.extend(
        [
            (UPDATE_LABEL, "interrupted-exec"),
            (PREVIOUS_CONTAINER_LABEL, operator.id.as_str()),
            (RESTART_POLICY_LABEL, "always"),
        ]
        .map(|(key, value)| (key.to_string(), value.to_string())),
    );
    let docker = FakeDocker::new(vec![operator, leftover_next]);

    run_update(&docker, &TestLogger, &plan(TARGET)).unwrap();

    assert_eq!(
        docker.calls()[..4],
        [
            "update-restart qovery-operator always",
            "rm qovery-operator-next",
            "pull localhost:5000/qovery-operator:n1",
            "create qovery-operator-next",
        ]
    );
    let updated = docker.named(NAME).unwrap();
    assert_eq!(updated.host_config.restart_policy.to_string(), "always");
    assert_eq!(
        updated.config.labels.get(RESTART_POLICY_LABEL).map(String::as_str),
        Some("always")
    );
    assert_eq!(
        docker
            .named("qovery-operator-previous")
            .unwrap()
            .host_config
            .restart_policy,
        RestartPolicy::NO
    );
}

#[test]
fn leftovers_that_are_not_ours_are_refused_before_anything_changes() {
    let mut foreign_next = leftover("qovery-operator-next", "abc", ContainerStatus::Created);
    foreign_next.config.labels.clear();
    let restarting_previous = leftover("qovery-operator-previous", "def", ContainerStatus::Restarting);
    let mut foreign_previous = leftover("qovery-operator-previous", "def", ContainerStatus::Exited);
    foreign_previous
        .config
        .env
        .0
        .retain(|entry| !entry.starts_with("CLUSTER_ID="));
    for (name, leftover) in [
        ("next without update label", foreign_next),
        ("restarting previous", restarting_previous),
        ("previous of another operator", foreign_previous),
    ] {
        let docker = FakeDocker::new(vec![readme(), leftover]);

        let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

        assert_eq!(failure.kind, UpdateFailureKind::UnsupportedConfiguration, "{name}");
        assert!(docker.calls().is_empty(), "{name}: {:?}", docker.calls());
    }
}

#[test]
fn preflight_refusals_leave_the_operator_untouched() {
    struct Case {
        name: &'static str,
        containers: Vec<ContainerInspect>,
        prepare: fn(&FakeDocker),
        expected: UpdateFailureKind,
    }
    for case in [
        Case {
            name: "docker unavailable",
            containers: vec![readme()],
            prepare: |docker| docker.state.borrow_mut().platform = None,
            expected: UpdateFailureKind::DockerUnavailable,
        },
        Case {
            name: "no operator",
            containers: vec![container("container_worker")],
            prepare: |_| {},
            expected: UpdateFailureKind::OperatorNotFound,
        },
        Case {
            name: "several operators",
            containers: vec![readme(), running("container_custom_name_network_log")],
            prepare: |_| {},
            expected: UpdateFailureKind::OperatorAmbiguous,
        },
        Case {
            name: "command override",
            containers: vec![running("container_command_override")],
            prepare: |_| {},
            expected: UpdateFailureKind::UnsupportedConfiguration,
        },
        Case {
            name: "pull failure",
            containers: vec![readme()],
            prepare: |docker| {
                docker.state.borrow_mut().failing = Some((DockerOperation::Pull, FailureTiming::BeforeActing))
            },
            expected: UpdateFailureKind::ImagePullFailed,
        },
        Case {
            name: "platform mismatch",
            containers: vec![readme()],
            prepare: |docker| {
                docker.state.borrow_mut().platform = Some(Platform {
                    os: PlatformOs::Linux,
                    architecture: Architecture::AMD64,
                })
            },
            expected: UpdateFailureKind::ImagePlatformMismatch,
        },
    ] {
        let docker = FakeDocker::new(case.containers);
        (case.prepare)(&docker);

        let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

        assert_eq!(failure.kind, case.expected, "{}", case.name);
        assert!(
            docker.calls().iter().all(|call| !is_mutation(call)),
            "{}: {:?}",
            case.name,
            docker.calls()
        );
    }
}

#[test]
fn next_left_behind_after_an_aborted_cut_over_is_reported_with_the_original_failure() {
    struct Case {
        name: &'static str,
        prepare: fn(&FakeDocker),
        expected: UpdateFailureKind,
        expected_calls: &'static [&'static str],
    }
    for case in [
        Case {
            name: "previous cannot be removed",
            prepare: |docker| docker.state.borrow_mut().repeated_failures = 1,
            expected: UpdateFailureKind::DockerOperationFailed,
            expected_calls: &[
                "pull localhost:5000/qovery-operator:n1",
                "create qovery-operator-next",
                "rm qovery-operator-previous",
                "rm qovery-operator-next",
            ],
        },
        Case {
            name: "previous started during the pull",
            prepare: start_previous_during_the_pull,
            expected: UpdateFailureKind::UnsupportedConfiguration,
            expected_calls: &[
                "pull localhost:5000/qovery-operator:n1",
                "create qovery-operator-next",
                "rm qovery-operator-next",
            ],
        },
    ] {
        let (operator, previous) = operator_with_stale_previous();
        let docker = FakeDocker::new(vec![operator.clone(), previous.clone()])
            .failing(DockerOperation::Remove, FailureTiming::BeforeActing);
        (case.prepare)(&docker);

        let failure = run_update(&docker, &TestLogger, &plan(TARGET)).unwrap_err();

        assert_eq!(failure.kind, case.expected, "{}: {}", case.name, failure.message);
        assert!(
            failure
                .message
                .contains("`qovery-operator-next` could not be removed: docker rm failed"),
            "{}: {}",
            case.name,
            failure.message
        );
        assert_eq!(docker.calls(), case.expected_calls, "{}", case.name);
        let running = docker.named(NAME).unwrap();
        assert_eq!(running.id, operator.id, "{}", case.name);
        assert_eq!(running.state.status, ContainerStatus::Running, "{}", case.name);
        assert_eq!(
            docker.named("qovery-operator-previous").unwrap().id,
            previous.id,
            "{}",
            case.name
        );
        assert!(docker.named("qovery-operator-next").is_some(), "{}", case.name);
    }
}

fn metadata(engine_v2_options: Value) -> Metadata {
    serde_json::from_value(json!({
        "dry_run_deploy": false,
        "cluster_operator_engine_v2": true,
        "engine_v2_options": engine_v2_options,
    }))
    .unwrap()
}

fn q_core_options(image: &str) -> Value {
    json!({
        "schema_version": "1",
        "execution_mode": "operator_container_update",
        "template": {"key": "eks-anywhere", "version": "1.0.0"},
        "platform_helm_units": [],
        "operator_container_update": {
            "key": "qovery-operator",
            "image": image,
            "operator_version": "v1.223.0",
        },
    })
}

struct ReportedRun {
    result: Result<(), Box<EngineError>>,
    unit: Value,
}

fn execute_and_read_the_report(docker: &FakeDocker, metadata: &Metadata) -> ReportedRun {
    let (sender, mut receiver) = unbounded_channel();
    let cluster_id = Uuid::parse_str(CLUSTER_ID).unwrap();
    let request = UpdateRequest {
        execution_id: "exec-42",
        action: Action::Create,
        metadata: Some(metadata),
        identity: identity(),
        event_details: EventDetails::new(
            None,
            QoveryIdentifier::new(Uuid::parse_str(ORGANIZATION_ID).unwrap()),
            QoveryIdentifier::new(cluster_id),
            "exec-42".to_string(),
            Stage::Infrastructure(InfrastructureStep::Create),
            Transmitter::Kubernetes(cluster_id, "bastion".to_string()),
        ),
    };
    let health = HealthCheck {
        window: Duration::ZERO,
        poll_interval: Duration::ZERO,
    };

    let result = execute(
        docker,
        request,
        Box::new(UnboundedSenderLogger::new(sender, Vec::new())),
        health,
    );

    let mut reports = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        if let EngineEvent::Info(details, message) = &event
            && *details.stage() == Stage::Infrastructure(InfrastructureStep::PlatformExecutionResult)
        {
            reports.push(message.message(EventMessageVerbosity::SafeOnly));
        }
    }
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report: PlatformExecutionResult = serde_json::from_str(&reports[0]).unwrap();
    assert_eq!(report.execution_id, "exec-42");
    assert_eq!(report.units.len(), 1);
    ReportedRun {
        result,
        unit: serde_json::to_value(&report.units[0]).unwrap(),
    }
}

#[test]
fn reported_unit_carries_the_digest_on_success_and_the_failure_code_otherwise() {
    let success = execute_and_read_the_report(&FakeDocker::new(vec![readme()]), &metadata(q_core_options(TARGET)));
    assert!(success.result.is_ok());
    assert_eq!(
        success.unit,
        json!({"key": "qovery-operator", "status": "SUCCEEDED", "message": pinned('1')})
    );

    let rolled_back =
        execute_and_read_the_report(&FakeDocker::new(vec![readme()]), &metadata(q_core_options(CRASHING_TARGET)));
    assert_eq!(rolled_back.unit["error_code"], "ROLLED_BACK");
    assert_eq!(rolled_back.unit["key"], "qovery-operator");
    assert_eq!(*rolled_back.result.unwrap_err().tag(), Tag::OperatorContainerUpdateFailed);

    let not_found = execute_and_read_the_report(&FakeDocker::new(Vec::new()), &metadata(q_core_options(TARGET)));
    assert_eq!(not_found.unit["error_code"], "OPERATOR_NOT_FOUND");
    assert_eq!(*not_found.result.unwrap_err().tag(), Tag::OperatorContainerUpdateFailed);

    let unavailable = FakeDocker::new(vec![readme()]);
    unavailable.state.borrow_mut().platform = None;
    let unavailable = execute_and_read_the_report(&unavailable, &metadata(q_core_options(TARGET)));
    assert_eq!(unavailable.unit["error_code"], "DOCKER_UNAVAILABLE");
    assert_eq!(*unavailable.result.unwrap_err().tag(), Tag::DockerError);

    let invalid =
        execute_and_read_the_report(&FakeDocker::new(vec![readme()]), &metadata(q_core_options("qovery/operator")));
    assert_eq!(invalid.unit["error_code"], "INVALID_PAYLOAD");
    assert_eq!(*invalid.result.unwrap_err().tag(), Tag::InvalidEnginePayload);
}

#[test]
fn q_core_operator_container_update_request_is_accepted() {
    let image = "public.ecr.aws/r3m4q3r9/qovery-operator:v1.223.0";
    let metadata = metadata(q_core_options(image));
    assert_eq!(
        metadata.engine_v2_options.as_ref().unwrap().execution_mode,
        ExecutionMode::OperatorContainerUpdate
    );

    let (unit_key, target) = validate_request(Action::Create, Some(&metadata)).unwrap();

    assert_eq!(unit_key, OPERATOR_UNIT_KEY);
    assert_eq!(
        target,
        UpdateTarget {
            image: ImageReference::parse(image).unwrap(),
            operator_version: OperatorVersion::parse("v1.223.0").unwrap(),
        }
    );
}

#[test]
fn invalid_operator_container_update_requests_are_refused() {
    let valid = || q_core_options("public.ecr.aws/r3m4q3r9/qovery-operator:v1.223.0");
    let with = |change: fn(&mut Value)| {
        let mut options = valid();
        change(&mut options);
        metadata(options)
    };
    for (name, action, metadata, expected_key) in [
        ("delete action", Action::Delete, metadata(valid()), OPERATOR_UNIT_KEY),
        (
            "missing unit",
            Action::Create,
            with(|options| {
                options.as_object_mut().unwrap().remove("operator_container_update");
            }),
            OPERATOR_UNIT_KEY,
        ),
        (
            "helm units",
            Action::Create,
            with(|options| {
                options["platform_helm_units"] = json!([{
                    "key": "cluster-agent", "action": "CREATE", "release_name": "cluster-agent",
                    "namespace": "qovery", "values_yaml": "",
                    "chart": {"repository": "https://helm.qovery.com", "name": "a", "version": "1"}
                }])
            }),
            OPERATOR_UNIT_KEY,
        ),
        (
            "preflight",
            Action::Create,
            with(|options| {
                options["schema_version"] = json!("2");
                options["preflight"] = json!({"mode": "OBSERVE", "checks": []});
            }),
            OPERATOR_UNIT_KEY,
        ),
        (
            "schema version",
            Action::Create,
            with(|options| options["schema_version"] = json!("3")),
            OPERATOR_UNIT_KEY,
        ),
        (
            "wrong key",
            Action::Create,
            with(|options| options["operator_container_update"]["key"] = json!("cluster-agent")),
            "cluster-agent",
        ),
        (
            "image without tag",
            Action::Create,
            with(|options| options["operator_container_update"]["image"] = json!("qovery/operator")),
            OPERATOR_UNIT_KEY,
        ),
        (
            "version with a newline",
            Action::Create,
            with(|options| options["operator_container_update"]["operator_version"] = json!("v1\nCLUSTER_ID=x")),
            OPERATOR_UNIT_KEY,
        ),
    ] {
        let violation = validate_request(action, Some(&metadata)).unwrap_err();
        assert_eq!(violation.unit_key, expected_key, "{name}");
    }

    let mut dry_run = metadata(valid());
    dry_run.dry_run_deploy = Some(true);
    assert!(validate_request(Action::Create, Some(&dry_run)).is_err());
    assert!(validate_request(Action::Create, None).is_err());
}

#[test]
fn helm_mode_payload_never_carries_an_operator_container_update() {
    let metadata = metadata(json!({
        "schema_version": "1",
        "execution_mode": "platform_components_only",
        "platform_helm_units": [],
    }));
    let options = metadata.engine_v2_options.unwrap();
    assert_eq!(options.operator_container_update, None::<OperatorContainerUpdateUnit>);
}

/// Runs against the local Docker daemon: `cargo test -p qovery-engine --lib operator_container_update::tests::e2e -- --ignored`.
mod e2e {
    use super::super::docker::DockerCli;
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::tempdir;

    const DOCKERFILE: &str = r#"FROM registry:2 AS src
FROM scratch
COPY --from=src /bin /bin
COPY --from=src /lib /lib
ARG VERSION
ARG COMMAND
ENV PATH=/bin QOVERY_FIXTURE_DEFAULT=${VERSION}
LABEL org.opencontainers.image.version=${VERSION}
WORKDIR /home/app
ENTRYPOINT ["/bin/sh", "-c"]
CMD ["${COMMAND}"]
"#;
    const LONG_RUNNING: &str = "trap 'exit 0' TERM; while true; do sleep 1; done";

    fn docker(args: &[&str]) -> String {
        let output = Command::new("docker").args(args).output().unwrap();
        assert!(
            output.status.success(),
            "docker {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn inspect(name: &str, format: &str) -> Option<String> {
        let output = Command::new("docker")
            .args(["container", "inspect", "--format", format, name])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    struct Cleanup {
        containers: Vec<String>,
        images: RefCell<Vec<String>>,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            for name in &self.containers {
                let _ = Command::new("docker")
                    .args(["container", "rm", "--force", name])
                    .output();
            }
            for image in self.images.borrow().iter() {
                let _ = Command::new("docker").args(["image", "rm", "--force", image]).output();
            }
        }
    }

    fn build_and_push(registry: &str, tag: &str, command: &str) -> String {
        let context = tempdir().unwrap();
        fs::write(
            context.path().join("Dockerfile"),
            DOCKERFILE.replace("${COMMAND}", &command.replace('"', "\\\"")),
        )
        .unwrap();
        let reference = format!("{registry}/qovery-operator-e2e:{tag}");
        docker(&[
            "build",
            "--quiet",
            "--build-arg",
            &format!("VERSION={tag}"),
            "--tag",
            &reference,
            context.path().to_str().unwrap(),
        ]);
        docker(&["push", "--quiet", &reference]);
        reference
    }

    #[test]
    #[ignore]
    fn updates_then_rolls_back_a_real_operator_container() {
        let prefix = format!("qovery-e2e-{}", &Uuid::new_v4().simple().to_string()[..8]);
        let operator_name = format!("{prefix}-operator");
        let worker_name = format!("{prefix}-worker");
        let registry_name = format!("{prefix}-registry");
        let cleanup = Cleanup {
            containers: vec![
                operator_name.clone(),
                format!("{operator_name}-next"),
                format!("{operator_name}-previous"),
                worker_name.clone(),
                registry_name.clone(),
            ],
            images: RefCell::new(Vec::new()),
        };
        let cluster_id = Uuid::new_v4();

        let registry = format!("127.0.0.1:{}", 40_000 + Uuid::new_v4().as_u128() % 20_000);
        docker(&[
            "run",
            "--detach",
            "--name",
            &registry_name,
            "--network",
            "host",
            "--env",
            &format!("REGISTRY_HTTP_ADDR={registry}"),
            "registry:2",
        ]);
        thread::sleep(Duration::from_secs(2));
        let image_n = build_and_push(&registry, "n", LONG_RUNNING);
        let image_n1 = build_and_push(&registry, "n1", LONG_RUNNING);
        let image_crash = build_and_push(&registry, "crash", "exit 3");
        cleanup
            .images
            .borrow_mut()
            .extend([image_n.clone(), image_n1.clone(), image_crash.clone()]);

        let identity_env = [
            "--env".to_string(),
            "QOVERY_OPERATOR_RUNTIME=DOCKER".to_string(),
            "--env".to_string(),
            format!("CLUSTER_ID={cluster_id}"),
            "--env".to_string(),
            format!("ORGANIZATION_ID={}", Uuid::nil()),
            "--env".to_string(),
            "CLUSTER_JWT_TOKEN=e2e-jwt".to_string(),
        ];
        let socket = "/var/run/docker.sock:/var/run/docker.sock";
        let mut run_operator = vec![
            "run",
            "--detach",
            "--name",
            &operator_name,
            "--restart",
            "always",
            "--group-add",
            "0",
        ];
        run_operator.extend(identity_env.iter().map(String::as_str));
        run_operator.extend(["--volume", socket, &image_n]);
        let original_operator = docker(&run_operator);
        let worker = docker(&[
            "run",
            "--detach",
            "--name",
            &worker_name,
            "--env",
            &format!("CLUSTER_ID={cluster_id}"),
            "--env",
            "RUN_ONCE=true",
            "--volume",
            socket,
            &image_n,
        ]);
        let next_name = format!("{operator_name}-next");
        let mut interrupted_preflight = vec![
            "create",
            "--name",
            &next_name,
            "--label",
            "qovery.com/operator-update=interrupted",
        ];
        interrupted_preflight.extend(identity_env.iter().map(String::as_str));
        interrupted_preflight.push(&image_n);
        docker(&interrupted_preflight);

        let cli = DockerCli::new(None);
        assert_eq!(cli.container_exists(&ContainerId::from(worker.as_str())), Ok(true));
        assert_eq!(cli.container_exists(&ContainerId::from(&*"0".repeat(64))), Ok(false));
        let identity = OperatorIdentity { cluster_id };
        let health = HealthCheck {
            window: Duration::from_secs(8),
            poll_interval: Duration::from_secs(1),
        };
        let update = |image: &str| {
            run_update(
                &cli,
                &TestLogger,
                &UpdatePlan {
                    identity,
                    image: ImageReference::parse(image).unwrap(),
                    operator_version: OperatorVersion::parse("n1").unwrap(),
                    execution_id: "e2e",
                    health,
                },
            )
        };

        let pinned = update(&image_n1).unwrap();

        assert!(
            pinned
                .as_str()
                .starts_with(&format!("{registry}/qovery-operator-e2e@sha256:"))
        );
        assert_eq!(inspect(&operator_name, "{{.Config.Image}}").unwrap(), pinned.as_str());
        assert_eq!(inspect(&operator_name, "{{.State.Status}}").unwrap(), "running");
        assert_eq!(inspect(&operator_name, "{{.HostConfig.RestartPolicy.Name}}").unwrap(), "always");
        assert!(
            inspect(&operator_name, "{{json .Config.Env}}")
                .unwrap()
                .contains("OPERATOR_VERSION=n1")
        );
        let updated_operator = inspect(&operator_name, "{{.Id}}").unwrap();
        let previous_name = format!("{operator_name}-previous");
        assert_eq!(inspect(&previous_name, "{{.Id}}").unwrap(), original_operator);
        assert_eq!(inspect(&previous_name, "{{.State.Status}}").unwrap(), "exited");
        assert_eq!(inspect(&previous_name, "{{.HostConfig.RestartPolicy.Name}}").unwrap(), "no");
        assert_eq!(inspect(&next_name, "{{.Id}}"), None);
        assert_eq!(inspect(&worker_name, "{{.Id}}").unwrap(), worker);
        assert_eq!(inspect(&worker_name, "{{.State.Status}}").unwrap(), "running");

        let failure = update(&format!("{registry}/qovery-operator-e2e:missing")).unwrap_err();

        assert_eq!(failure.kind, UpdateFailureKind::ImagePullFailed, "{}", failure.message);
        assert_eq!(inspect(&previous_name, "{{.Id}}").unwrap(), original_operator);
        assert_eq!(inspect(&previous_name, "{{.State.Status}}").unwrap(), "exited");
        assert_eq!(inspect(&operator_name, "{{.Id}}").unwrap(), updated_operator);
        assert_eq!(inspect(&operator_name, "{{.State.Status}}").unwrap(), "running");
        assert_eq!(inspect(&next_name, "{{.Id}}"), None);

        let failure = update(&image_crash).unwrap_err();

        assert_eq!(failure.kind, UpdateFailureKind::RolledBack, "{}", failure.message);
        assert_eq!(inspect(&operator_name, "{{.Id}}").unwrap(), updated_operator);
        assert_eq!(inspect(&operator_name, "{{.State.Status}}").unwrap(), "running");
        assert_eq!(inspect(&operator_name, "{{.HostConfig.RestartPolicy.Name}}").unwrap(), "always");
        assert_eq!(inspect(&next_name, "{{.Id}}"), None);
        assert_eq!(inspect(&previous_name, "{{.Id}}"), None);
        assert_eq!(inspect(&worker_name, "{{.Id}}").unwrap(), worker);
        assert_eq!(inspect(&worker_name, "{{.State.Status}}").unwrap(), "running");
    }
}
