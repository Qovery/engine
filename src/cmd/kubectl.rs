use json_patch::{AddOperation, PatchOperation, RemoveOperation, ReplaceOperation, TestOperation};
use jsonptr::PointerBuf;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{ApiResource, DeleteParams, Patch, PatchParams, PropagationPolicy};
use kube::core::GroupVersionKind;
use kube::core::params::ListParams;
use kube::{Api, Client, ResourceExt};
use semver::Version;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use serde_yaml::Deserializer;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs::{File, read_dir, read_to_string};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use url::Url;
use uuid::Uuid;

use crate::runtime::block_on;

use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::cmd::structs::{
    Configmap, KubernetesIngress, KubernetesIngressStatusLoadBalancerIngress, KubernetesJob, KubernetesKind,
    KubernetesList, KubernetesNode, KubernetesPod, KubernetesPodStatusReason, KubernetesVersion, MetricsServer, PDB,
    PVC, SVC, Secrets,
};
use crate::constants::KUBECONFIG;
use crate::errors::{CommandError, ErrorMessageVerbosity};

const LOCALHOST: &str = "127.0.0.1";
const PORT_FORWARD_START_TIMEOUT: Duration = Duration::from_secs(30);

pub enum ScalingKind {
    Deployment,
    Statefulset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KubernetesServicePortForwardTarget {
    pub namespace: String,
    pub service_name: String,
    pub remote_port: u16,
}

impl KubernetesServicePortForwardTarget {
    pub fn from_service_url(url: &Url) -> Option<Self> {
        let host = url.host_str()?;
        let mut parts = host.split('.');
        let service_name = parts.next()?;
        let namespace = parts.next()?;

        if parts.next() != Some("svc") {
            return None;
        }

        Some(Self {
            namespace: namespace.to_string(),
            service_name: service_name.to_string(),
            remote_port: url.port_or_known_default()?,
        })
    }
}

pub struct KubectlPortForward {
    child: Child,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    target: KubernetesServicePortForwardTarget,
    local_port: u16,
}

impl KubectlPortForward {
    pub fn start(
        kubeconfig: &Path,
        target: KubernetesServicePortForwardTarget,
        envs: &[(&str, &str)],
    ) -> Result<Self, CommandError> {
        let local_port = reserve_local_port()?;
        let mut command = Command::new("kubectl");
        command
            .arg("-n")
            .arg(target.namespace.as_str())
            .arg("port-forward")
            .arg(format!("service/{}", target.service_name))
            .arg(format!("{local_port}:{}", target.remote_port))
            .arg("--address")
            .arg(LOCALHOST)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env(KUBECONFIG, kubeconfig)
            .envs(envs.iter().copied());

        let mut child = command.spawn().map_err(|error| {
            CommandError::new(
                "Cannot start kubectl port-forward".to_string(),
                Some(format!("Cannot start kubectl port-forward for {target:?}: {error}")),
                None,
            )
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            CommandError::new(
                "Cannot read kubectl port-forward stdout".to_string(),
                Some(format!("Cannot read kubectl port-forward stdout for {target:?}")),
                None,
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            CommandError::new(
                "Cannot read kubectl port-forward stderr".to_string(),
                Some(format!("Cannot read kubectl port-forward stderr for {target:?}")),
                None,
            )
        })?;

        let (sender, receiver) = channel();
        let stdout_thread = spawn_output_reader(stdout, sender.clone());
        let stderr_thread = spawn_output_reader(stderr, sender);

        let mut port_forward = Self {
            child,
            stdout_thread: Some(stdout_thread),
            stderr_thread: Some(stderr_thread),
            target,
            local_port,
        };

        if let Err(error) = port_forward.wait_until_ready(&receiver) {
            port_forward.stop();
            return Err(error);
        }

        Ok(port_forward)
    }

    pub fn local_url(&self) -> Result<Url, CommandError> {
        Url::parse(&format!("http://{LOCALHOST}:{}", self.local_port)).map_err(|error| {
            CommandError::new(
                "Cannot build kubectl port-forward local URL".to_string(),
                Some(format!(
                    "Cannot build kubectl port-forward local URL for {:?}: {error}",
                    self.target
                )),
                None,
            )
        })
    }

    fn wait_until_ready(&mut self, receiver: &Receiver<String>) -> Result<(), CommandError> {
        let started_at = Instant::now();
        let mut output = Vec::new();

        while started_at.elapsed() < PORT_FORWARD_START_TIMEOUT {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    return Err(CommandError::new(
                        "kubectl port-forward stopped before being ready".to_string(),
                        Some(format!(
                            "kubectl port-forward for {:?} exited with {status} before being ready.\n{}",
                            self.target,
                            output.join("\n")
                        )),
                        None,
                    ));
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(CommandError::new(
                        "Cannot read kubectl port-forward status".to_string(),
                        Some(format!(
                            "Cannot read kubectl port-forward status for {:?}: {error}",
                            self.target
                        )),
                        None,
                    ));
                }
            }

            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => {
                    info!("kubectl port-forward: {}", line);
                    if line.contains(&format!("Forwarding from {LOCALHOST}:{}", self.local_port)) {
                        return Ok(());
                    }
                    output.push(line);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(CommandError::new(
                        "kubectl port-forward output stream closed".to_string(),
                        Some(format!(
                            "kubectl port-forward output stream closed before being ready for {:?}.\n{}",
                            self.target,
                            output.join("\n")
                        )),
                        None,
                    ));
                }
            }
        }

        Err(CommandError::new(
            "kubectl port-forward did not become ready in time".to_string(),
            Some(format!(
                "kubectl port-forward did not become ready in {:?} for {:?}.\n{}",
                PORT_FORWARD_START_TIMEOUT,
                self.target,
                output.join("\n")
            )),
            None,
        ))
    }

    fn stop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();

        if let Some(thread) = self.stdout_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for KubectlPortForward {
    fn drop(&mut self) {
        self.stop();
    }
}

fn reserve_local_port() -> Result<u16, CommandError> {
    TcpListener::bind((LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| {
            CommandError::new(
                "Cannot reserve local port for kubectl port-forward".to_string(),
                Some(format!("Cannot reserve local port for kubectl port-forward: {error}")),
                None,
            )
        })
}

fn spawn_output_reader<R>(reader: R, sender: std::sync::mpsc::Sender<String>) -> JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    })
}

#[derive(Debug)]
pub enum PodCondition {
    Ready,
    Complete,
    Delete,
}

pub fn kubectl_exec_with_output<F, X>(
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: &mut F,
    stderr_output: &mut X,
) -> Result<(), CommandError>
where
    F: FnMut(String),
    X: FnMut(String),
{
    let mut cmd = QoveryCommand::new("kubectl", &args, &envs);

    if let Err(err) = cmd.exec_with_output(stdout_output, stderr_output) {
        let args_string = args.join(" ");
        let msg = format!("Error on command: kubectl {}. {:?}", args_string, err);
        error!("{}", &msg);
        return Err(CommandError::new_from_command_line(
            "Error while executing a kubectl command.".to_string(),
            "kubectl".to_string(),
            args.into_iter().map(|a| a.to_string()).collect(),
            envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            None,
            None,
        ));
    };

    Ok(())
}

pub fn kubectl_exec_get_number_of_restart<P>(
    kubernetes_config: P,
    namespace: &str,
    service_id: &Uuid,
    envs: Vec<(&str, &str)>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        _envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    kubectl_exec_with_output(
        vec![
            "get",
            "po",
            "-l",
            &format!("qovery.com/service-id={service_id}"),
            "-n",
            namespace,
            "-o=custom-columns=:.status.containerStatuses..restartCount",
        ],
        _envs,
        &mut |line| output_vec.push(line),
        &mut |line| error!("{}", line),
    )?;

    let output_string: String = output_vec.join("");
    Ok(output_string)
}

pub fn kubectl_exec_get_external_ingress<P>(
    kubernetes_config: P,
    namespace: &str,
    name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<KubernetesIngressStatusLoadBalancerIngress>, CommandError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec::<P, KubernetesIngress>(
        vec!["get", "-n", namespace, "ing", name, "-o", "json"],
        kubernetes_config,
        envs,
    )?;

    if result.status.load_balancer.ingress.is_empty() {
        return Ok(None);
    }

    Ok(Some(result.status.load_balancer.ingress.first().unwrap().clone()))
}

pub fn kubectl_exec_get_secrets<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Secrets, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, Secrets>(
        vec![
            "get",
            "secrets",
            "-o",
            "json",
            "-n",
            namespace,
            "-l",
            selector,
            "--sort-by=.metadata.creationTimestamp",
        ],
        kubernetes_config,
        envs,
    )
}

pub fn kubectl_update_crd(kube_client: &Client, chart_name: &str, crd_folder: &str) -> Result<(), CommandError> {
    let crds_api: Api<CustomResourceDefinition> = Api::all(kube_client.clone());

    // Read all CRD files in the folder
    let mut dir = read_dir(crd_folder).map_err(|e| {
        CommandError::new(
            format!("Error while trying to read CRD folder `{crd_folder}`"),
            Some(e.to_string()),
            None,
        )
    })?;

    while let Some(Ok(entry)) = dir.next() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
            let crd_yaml = read_to_string(&path).map_err(|e| {
                CommandError::new(
                    format!("Error while trying to read CRD file `{}`", path.display()),
                    Some(e.to_string()),
                    None,
                )
            })?;

            for crd in Deserializer::from_str(&crd_yaml) {
                match serde_yaml::from_value::<CustomResourceDefinition>(serde_yaml::Value::deserialize(crd).map_err(
                    |e| {
                        CommandError::new(
                            format!("Error while trying to parse CRD file `{}`", path.display()),
                            Some(e.to_string()),
                            None,
                        )
                    },
                )?) {
                    Ok(crd) => {
                        let pp = PatchParams::apply(chart_name).force();
                        let patch = Patch::Apply(&crd);

                        block_on(crds_api.patch(&crd.name_any(), &pp, &patch)).map_err(|e| {
                            CommandError::new(
                                format!("Error while trying to update CRD `{}` (`{}`)", crd.name_any(), path.display()),
                                Some(e.to_string()),
                                None,
                            )
                        })?;
                    }
                    Err(e) => {
                        return Err(CommandError::new(
                            format!("Error while trying to parse CRD file `{}`", path.display()),
                            Some(e.to_string()),
                            None,
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn kubectl_exec_delete_crd<P>(
    kubernetes_config: P,
    crd_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        _envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    _envs.extend(envs);

    kubectl_exec_with_output(
        vec!["delete", "crd", crd_name],
        _envs,
        &mut |line| info!("{}", line),
        &mut |line| error!("{}", line),
    )?;

    Ok(())
}

pub fn kubectl_exec_delete_secret<P>(
    kubernetes_config: P,
    namespace: &str,
    secret: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        _envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    _envs.extend(envs);

    kubectl_exec_with_output(
        vec!["-n", namespace, "delete", "secret", secret],
        _envs,
        &mut |line| info!("{}", line),
        &mut |line| error!("{}", line),
    )?;

    Ok(())
}

pub fn kubectl_exec_version<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<KubernetesVersion, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, KubernetesVersion>(vec!["version", "-o", "json"], kubernetes_config, envs)
}

pub fn kubectl_exec_rollout_restart_deployment<P>(
    kubernetes_config: P,
    name: &str,
    namespace: &str,
    envs: &[(&str, &str)],
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let mut environment_variables: Vec<(&str, &str)> = envs.to_owned();
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        environment_variables.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    let args = vec!["-n", namespace, "rollout", "restart", "deployment", name];

    kubectl_exec_with_output(args, environment_variables, &mut |line| info!("{}", line), &mut |line| {
        error!("{}", line)
    })
}

pub fn kubectl_exec_get_node<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    selector: Option<&str>,
) -> Result<KubernetesList<KubernetesNode>, CommandError>
where
    P: AsRef<Path>,
{
    let mut args = vec!["get", "node", "-o", "json"];
    if let Some(s) = selector {
        args.push("--selector");
        args.push(s);
    }

    kubectl_exec::<P, KubernetesList<KubernetesNode>>(args, kubernetes_config, envs)
}

pub fn kubectl_exec_count_all_objects<P>(
    kubernetes_config: P,
    object_kind: &str,
    envs: Vec<(&str, &str)>,
) -> Result<usize, CommandError>
where
    P: AsRef<Path>,
{
    match kubectl_exec::<P, KubernetesList<KubernetesKind>>(
        vec!["get", object_kind, "-A", "-o", "json"],
        kubernetes_config,
        envs,
    ) {
        Ok(o) => Ok(o.items.len()),
        Err(e) => Err(e),
    }
}

pub fn kubectl_exec_get_pods<P>(
    kubernetes_config: P,
    namespace: Option<&str>,
    selector: Option<&str>,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesPod>, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["get", "pods", "-o", "json"];

    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    if let Some(s) = selector {
        cmd_args.push("--selector");
        cmd_args.push(s);
    }

    kubectl_exec::<P, KubernetesList<KubernetesPod>>(cmd_args, kubernetes_config, envs)
}

/// kubectl_exec_get_pod_by_name: allows to retrieve a pod by its name
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `namespace` - kubernetes namespace
/// * `pod_name` - pod's name
/// * `envs` - environment variables required for kubernetes connection
pub fn kubectl_exec_get_pod_by_name<P>(
    kubernetes_config: P,
    namespace: Option<&str>,
    pod_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesPod, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["get", "pod", "-o", "json"];

    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    cmd_args.push(pod_name);

    kubectl_exec::<P, KubernetesPod>(cmd_args, kubernetes_config, envs)
}

pub fn kubectl_exec_get_configmap<P>(
    kubernetes_config: P,
    namespace: &str,
    name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Configmap, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, Configmap>(
        vec!["get", "configmap", "-o", "json", "-n", namespace, name],
        kubernetes_config,
        envs,
    )
}

pub fn kubectl_exec_get_events<P>(
    kubernetes_config: P,
    namespace: Option<&str>,
    envs: Vec<(&str, &str)>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut environment_variables = envs;
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        environment_variables.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }

    let arg_namespace = match namespace {
        Some(n) => format!("-n {n}"),
        None => "-A".to_string(),
    };

    let args = vec!["get", "event", arg_namespace.as_str(), "--sort-by='.lastTimestamp'"];

    let mut result_ok = String::new();
    match kubectl_exec_with_output(args, environment_variables, &mut |line| result_ok = line, &mut |_| {}) {
        Ok(()) => Ok(result_ok),
        Err(err) => Err(err),
    }
}

pub fn kubectl_delete_objects_in_all_namespaces<P>(
    kubernetes_config: P,
    object: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec_raw_output(
        vec!["delete", object, "--all-namespaces", "--all"],
        kubernetes_config,
        envs,
        false,
    );

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            let lower_case_message = e.message(ErrorMessageVerbosity::FullDetails).to_lowercase();
            if lower_case_message.contains("no resources found") || lower_case_message.ends_with(" deleted") {
                return Ok(());
            }
            Err(e)
        }
    }
}

/// scale down replicas by name
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `envs` - environment variables required for kubernetes connection
/// * `namespace` - kubernetes namespace
/// * `kind` - kind of kubernetes resource to scale
/// * `names` - name of the kind of resource to scale
/// * `replicas_count` - desired number of replicas
pub fn kubectl_exec_scale_replicas<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: &str,
    kind: ScalingKind,
    name: &str,
    replicas_count: u32,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let kind_formatted = match kind {
        ScalingKind::Deployment => "deployment.v1.apps",
        ScalingKind::Statefulset => "statefulset.v1.apps",
    };
    let kind_with_name = format!("{kind_formatted}/{name}");

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        _envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    _envs.extend(envs);

    kubectl_exec_with_output(
        vec![
            "-n",
            namespace,
            "scale",
            &kind_with_name,
            "--replicas",
            &replicas_count.to_string(),
        ],
        _envs,
        &mut |_| {},
        &mut |_| {},
    )
}

pub fn kubectl_exec_wait_for_pods_condition<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: &str,
    selector: &str,
    condition: PodCondition,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let condition_format = format!(
        "--for={}",
        match condition {
            PodCondition::Delete => format!("{:?}", condition).to_lowercase(),
            _ => format!("condition={:?}", condition).to_lowercase(),
        }
    );

    let mut complete_envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        complete_envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    complete_envs.extend(envs);

    kubectl_exec_with_output(
        vec![
            "-n",
            namespace,
            "wait",
            condition_format.as_str(),
            "pod",
            "--selector",
            selector,
            "--timeout=300s",
        ],
        complete_envs,
        &mut |out| info!("{:?}", out),
        &mut |out| warn!("{:?}", out),
    )
}

pub fn kubectl_get_pvc<P>(kubernetes_config: P, namespace: &str, envs: Vec<(&str, &str)>) -> Result<PVC, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, PVC>(vec!["get", "pvc", "-o", "json", "-n", namespace], kubernetes_config, envs)
}

pub fn kubectl_get_svc<P>(kubernetes_config: P, namespace: &str, envs: Vec<(&str, &str)>) -> Result<SVC, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, SVC>(vec!["get", "svc", "-o", "json", "-n", namespace], kubernetes_config, envs)
}

/// kubectl_delete_crash_looping_pods: delete crash looping pods.
///
/// Arguments
///
/// * `kubernetes_config`: kubernetes config file path.
/// * `namespace`: namespace to delete pods from, if None, will delete from all namespaces.
/// * `selector`: selector for pods to be deleted. If None, will delete all crash looping pods.
/// * `envs`: environment variables to be passed to kubectl.
pub fn kubectl_delete_crash_looping_pods<P>(
    kubernetes_config: P,
    namespace: Option<&str>,
    selector: Option<&str>,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<KubernetesPod>, CommandError>
where
    P: AsRef<Path>,
{
    let crash_looping_pods =
        kubectl_get_crash_looping_pods(&kubernetes_config, namespace, selector, None, envs.clone())?;

    for crash_looping_pod in crash_looping_pods.iter() {
        kubectl_exec_delete_pod(
            &kubernetes_config,
            crash_looping_pod.metadata.namespace.as_str(),
            crash_looping_pod.metadata.name.as_str(),
            envs.clone(),
        )?;
    }

    Ok(crash_looping_pods)
}

pub fn kubectl_delete_apiservice<P>(
    kubernetes_config: P,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let cmd_args = vec!["delete", "apiservice", "-l", selector];

    kubectl_exec_raw_output(cmd_args, kubernetes_config, envs, false)
}

/// kubectl_get_validating_admission_policy: gets a ValidatingAdmissionPolicy and its binding as YAML.
///
/// Returns a tuple of (policy_yaml, binding_yaml) if both exist, or None if either doesn't exist.
///
/// Arguments
///
/// * `kubernetes_config`: kubernetes config file path.
/// * `policy_name`: name of the ValidatingAdmissionPolicy to get.
/// * `envs`: environment variables to be passed to kubectl.
pub fn kubectl_get_validating_admission_policy<P>(
    kubernetes_config: P,
    policy_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<(String, String)>, CommandError>
where
    P: AsRef<Path>,
{
    // Get the policy
    let policy_cmd_args = vec!["get", "validatingadmissionpolicy", policy_name, "-o", "yaml"];
    let policy_yaml = match kubectl_exec_raw_output(policy_cmd_args, kubernetes_config.as_ref(), envs.clone(), false) {
        Ok(yaml) => yaml,
        Err(_) => return Ok(None), // Policy doesn't exist
    };

    // Get the binding
    let binding_cmd_args = vec!["get", "validatingadmissionpolicybinding", policy_name, "-o", "yaml"];
    let binding_yaml = match kubectl_exec_raw_output(binding_cmd_args, kubernetes_config.as_ref(), envs, false) {
        Ok(yaml) => yaml,
        Err(_) => return Ok(None), // Binding doesn't exist
    };

    Ok(Some((policy_yaml, binding_yaml)))
}

/// kubectl_apply_validating_admission_policy: applies a ValidatingAdmissionPolicy and its binding from YAML.
///
/// Arguments
///
/// * `kubernetes_config`: kubernetes config file path.
/// * `policy_yaml`: YAML content of the ValidatingAdmissionPolicy.
/// * `binding_yaml`: YAML content of the ValidatingAdmissionPolicyBinding.
/// * `envs`: environment variables to be passed to kubectl.
pub fn kubectl_apply_validating_admission_policy<P>(
    kubernetes_config: P,
    policy_yaml: &str,
    binding_yaml: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    // Apply the policy
    kubectl_apply_with_server_side_apply(kubernetes_config.as_ref(), envs.clone(), None, policy_yaml, true)?;

    // Apply the binding
    kubectl_apply_with_server_side_apply(kubernetes_config.as_ref(), envs, None, binding_yaml, true)
}

pub fn kubectl_does_crd_exist(kube_client: &Client, crd_name: &str) -> bool {
    let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());

    match block_on(crds.get(crd_name)) {
        Ok(crd) => crd
            .status
            .as_ref()
            .and_then(|s| s.conditions.as_ref())
            .map(|conditions| {
                conditions
                    .iter()
                    .any(|c| c.type_ == "Established" && c.status == "True")
            })
            .unwrap_or(false),
        Err(e) => {
            debug!("CRD '{}' not found or not accessible: {}", crd_name, e);
            false
        }
    }
}

/// Returns the Gateway API bundle version read from the `gateway.networking.k8s.io/bundle-version`
/// annotation on a core CRD (e.g. `gateways.gateway.networking.k8s.io`), or `None` if the CRD is
/// absent or the annotation is missing / unparseable.
pub fn kubectl_get_gateway_api_bundle_version(kube_client: &Client) -> Option<Version> {
    let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());
    let crd = match block_on(crds.get("gateways.gateway.networking.k8s.io")) {
        Ok(crd) => crd,
        Err(e) => {
            debug!("Could not fetch Gateway CRD to determine bundle version: {}", e);
            return None;
        }
    };

    let raw = crd
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get("gateway.networking.k8s.io/bundle-version"))
        .map(|s| s.as_str())?;

    // The annotation value may be prefixed with "v" (e.g. "v1.8.0").
    let trimmed = raw.trim_start_matches('v');
    match Version::parse(trimmed) {
        Ok(v) => Some(v),
        Err(e) => {
            debug!("Could not parse Gateway API bundle-version annotation '{}': {}", raw, e);
            None
        }
    }
}

pub fn kubectl_check_gateway_api_crds_available(kube_client: &Client) -> bool {
    // Core CRDs that must always be present.
    let core_crds = [
        "referencegrants.gateway.networking.k8s.io",
        "gateways.gateway.networking.k8s.io",
        "httproutes.gateway.networking.k8s.io",
    ];

    for crd_name in &core_crds {
        if !kubectl_does_crd_exist(kube_client, crd_name) {
            info!(
                "Gateway API CRD '{}' not found or not established - Gateway API features will be disabled",
                crd_name
            );
            return false;
        }
    }

    // ListenerSet was introduced as a standard resource in Gateway API >= 1.8.0.
    // Only require it when the installed bundle version is >= 1.8.0; older installs
    // (e.g. the version GKE ships) don't have it and should still be considered valid
    // for core Gateway API usage.
    let min_listenerset_version = Version::new(1, 8, 0);
    match kubectl_get_gateway_api_bundle_version(kube_client) {
        Some(v) if v >= min_listenerset_version => {
            if !kubectl_does_crd_exist(kube_client, "listenersets.gateway.networking.k8s.io") {
                info!(
                    "Gateway API bundle version is {} (>= 1.8.0) but ListenerSet CRD is not established - Gateway API features will be disabled",
                    v
                );
                return false;
            }
        }
        Some(v) => {
            info!("Gateway API bundle version is {} (< 1.8.0) - ListenerSet CRD not required", v);
        }
        None => {
            info!("Could not determine Gateway API bundle version - skipping ListenerSet CRD check");
        }
    }

    info!("All required Gateway API CRDs are available and established");
    true
}

/// Returns true if ListenerSet resources should be deployed based on the installed Gateway API
/// bundle version and CRD availability.
///
/// Why this exists:
/// During the envoy-gateway 1.8 rollout, some managed GKE clusters exposed enough Gateway API
/// surface to enable the stack while still not supporting ListenerSet attachment end-to-end.
/// Requiring the ListenerSet CRD itself keeps the feature gate aligned with the live cluster
/// instead of assuming bundle detection is always reliable.
pub fn kubectl_should_deploy_listenerset(kube_client: &Client) -> bool {
    kubectl_does_crd_exist(kube_client, "listenersets.gateway.networking.k8s.io")
}

/// Returns true if the Gateway CRD schema exposes the `allowedListeners` field.
///
/// Why this matters:
/// Cross-namespace ListenerSet attachment depends on the parent Gateway accepting the
/// `spec.listeners[*].allowedListeners` field. Some managed GKE clusters lag here even when
/// other Gateway API CRDs are present, so callers use this to decide whether to rely on
/// ListenerSet attachment or fall back to direct Gateway TLS secret references.
pub fn kubectl_gateway_crd_supports_allowed_listeners(kube_client: &Client) -> bool {
    let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());
    let crd = match block_on(crds.get("gateways.gateway.networking.k8s.io")) {
        Ok(crd) => crd,
        Err(e) => {
            debug!("Could not fetch Gateway CRD to check allowedListeners support: {}", e);
            return false;
        }
    };

    let Ok(crd_value) = serde_json::to_value(&crd) else {
        debug!("Could not serialize Gateway CRD to inspect schema");
        return false;
    };

    let versions = crd_value
        .get("spec")
        .and_then(|v| v.get("versions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for version in versions {
        let served = version.get("served").and_then(Value::as_bool).unwrap_or(false);
        if !served {
            continue;
        }

        let allowed_listeners = version
            .get("schema")
            .and_then(|v| v.get("openAPIV3Schema"))
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.get("spec"))
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.get("listeners"))
            .and_then(|v| v.get("items"))
            .and_then(|v| v.get("properties"))
            .and_then(|v| v.get("allowedListeners"));

        if allowed_listeners.is_some() {
            return true;
        }
    }

    false
}

/// Returns the preferred served Gateway API version for the Gateway CRD (e.g. "v1" or "v1beta1").
/// Prefers v1 if served, otherwise falls back to v1beta1, or the first served version if any.
pub fn kubectl_get_gateway_api_served_version(kube_client: &Client) -> Option<String> {
    let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());
    let crd = match block_on(crds.get("gateways.gateway.networking.k8s.io")) {
        Ok(crd) => crd,
        Err(e) => {
            debug!("Could not fetch Gateway CRD to determine served version: {}", e);
            return None;
        }
    };

    let Ok(crd_value) = serde_json::to_value(&crd) else {
        debug!("Could not serialize Gateway CRD to inspect versions");
        return None;
    };

    let versions = crd_value
        .get("spec")
        .and_then(|v| v.get("versions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut served_versions: Vec<String> = Vec::new();
    for version in versions {
        let served = version.get("served").and_then(Value::as_bool).unwrap_or(false);
        if !served {
            continue;
        }
        if let Some(name) = version.get("name").and_then(Value::as_str) {
            served_versions.push(name.to_string());
        }
    }

    if served_versions.iter().any(|v| v == "v1") {
        return Some("v1".to_string());
    }
    if served_versions.iter().any(|v| v == "v1beta1") {
        return Some("v1beta1".to_string());
    }

    served_versions.into_iter().next()
}

/// Returns the preferred served Gateway API version for the ReferenceGrant CRD (e.g. "v1" or "v1beta1").
/// Prefers v1 if served, otherwise falls back to v1beta1, or the first served version if any.
pub fn kubectl_get_reference_grant_served_version(kube_client: &Client) -> Option<String> {
    let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());
    let crd = match block_on(crds.get("referencegrants.gateway.networking.k8s.io")) {
        Ok(crd) => crd,
        Err(e) => {
            debug!("Could not fetch ReferenceGrant CRD to determine served version: {}", e);
            return None;
        }
    };

    let Ok(crd_value) = serde_json::to_value(&crd) else {
        debug!("Could not serialize ReferenceGrant CRD to inspect versions");
        return None;
    };

    let versions = crd_value
        .get("spec")
        .and_then(|v| v.get("versions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut served_versions: Vec<String> = Vec::new();
    for version in versions {
        let served = version.get("served").and_then(Value::as_bool).unwrap_or(false);
        if !served {
            continue;
        }
        if let Some(name) = version.get("name").and_then(Value::as_str) {
            served_versions.push(name.to_string());
        }
    }

    if served_versions.iter().any(|v| v == "v1") {
        return Some("v1".to_string());
    }
    if served_versions.iter().any(|v| v == "v1beta1") {
        return Some("v1beta1".to_string());
    }

    served_versions.into_iter().next()
}

/// Returns the preferred served Gateway API version for the ListenerSet CRD (e.g. "v1" or "v1beta1").
/// Prefers v1 if served, otherwise falls back to v1beta1, or the first served version if any.
pub fn kubectl_get_listenerset_served_version(kube_client: &Client) -> Option<String> {
    let crds: Api<CustomResourceDefinition> = Api::all(kube_client.clone());
    let crd = match block_on(crds.get("listenersets.gateway.networking.k8s.io")) {
        Ok(crd) => crd,
        Err(e) => {
            debug!("Could not fetch ListenerSet CRD to determine served version: {}", e);
            return None;
        }
    };

    let Ok(crd_value) = serde_json::to_value(&crd) else {
        debug!("Could not serialize ListenerSet CRD to inspect versions");
        return None;
    };

    let versions = crd_value
        .get("spec")
        .and_then(|v| v.get("versions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut served_versions: Vec<String> = Vec::new();
    for version in versions {
        let served = version.get("served").and_then(Value::as_bool).unwrap_or(false);
        if !served {
            continue;
        }
        if let Some(name) = version.get("name").and_then(Value::as_str) {
            served_versions.push(name.to_string());
        }
    }

    if served_versions.iter().any(|v| v == "v1") {
        return Some("v1".to_string());
    }
    if served_versions.iter().any(|v| v == "v1beta1") {
        return Some("v1beta1".to_string());
    }

    served_versions.into_iter().next()
}

/// Reconciles Gateway TLS certificateRefs to exactly match live router TLS Secrets (`router-tls-*`).
///
/// Non-router certificate references are preserved. Router references are derived from live
/// Ingresses, ListenerSets, and GKE fallback ReferenceGrants, so a cluster update removes stale
/// fallback references left by interrupted environment deletions. Returns true if the Gateway was
/// patched.
pub fn kubectl_reconcile_gateway_certrefs_for_router_tls_secrets(
    kube_client: &Client,
    gateway_namespace: &str,
    gateway_name: &str,
    listener_name: &str,
) -> Result<bool, CommandError> {
    let api_version = kubectl_get_gateway_api_served_version(kube_client).unwrap_or_else(|| "v1".to_string());
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &api_version, "Gateway");
    let api: Api<kube::core::DynamicObject> =
        Api::namespaced_with(kube_client.clone(), gateway_namespace, &ApiResource::from_gvk(&gvk));

    for _ in 0..MAX_GATEWAY_CERTIFICATE_REF_RECONCILIATION_ATTEMPTS {
        // Fetch the Gateway before its sources of truth. If router cleanup changes the Gateway
        // after this read, the resourceVersion test below rejects this attempt and the next one
        // recomputes desired references after the cleanup has completed.
        let gateway = block_on(api.get(gateway_name)).map_err(|error| {
            CommandError::new_from_safe_message(format!(
                "Failed to fetch Gateway {gateway_namespace}/{gateway_name}: {error}"
            ))
        })?;
        let mut desired_refs = gateway_router_tls_certificate_refs(kube_client)?;
        let fallback_ownership = gateway_fallback_certificate_ref_ownership(&gateway);
        let live_fallback_ownership =
            live_gateway_fallback_certificate_ref_ownership(kube_client, &fallback_ownership)?;
        let stale_fallback_ownership = fallback_ownership
            .difference(&live_fallback_ownership)
            .cloned()
            .collect();
        desired_refs.extend(live_fallback_ownership);
        let legacy_fallback_refs =
            gateway_legacy_reference_grant_certificate_refs(kube_client, gateway_namespace, &fallback_ownership)?;
        desired_refs.extend(legacy_fallback_refs.iter().cloned());
        ensure_gateway_reference_grants_for_router_tls_secrets(kube_client, gateway_namespace, &desired_refs)?;
        let Some(patch) = gateway_certificate_refs_reconciliation_patch(
            &gateway,
            listener_name,
            &desired_refs,
            &legacy_fallback_refs,
            &stale_fallback_ownership,
            gateway_namespace,
        )?
        else {
            return Ok(false);
        };

        let patch: Patch<kube::core::DynamicObject> = Patch::Json(patch);
        match block_on(api.patch(gateway_name, &PatchParams::default(), &patch)) {
            Ok(_) => return Ok(true),
            Err(error) if is_kubernetes_optimistic_concurrency_conflict(&error) => continue,
            Err(error) => {
                return Err(CommandError::new_from_safe_message(format!(
                    "Failed to patch Gateway {gateway_namespace}/{gateway_name} certificateRefs: {error}"
                )));
            }
        }
    }

    Err(CommandError::new_from_safe_message(format!(
        "Failed to reconcile Gateway {gateway_namespace}/{gateway_name} certificateRefs: the Gateway changed concurrently"
    )))
}

const MAX_GATEWAY_CERTIFICATE_REF_RECONCILIATION_ATTEMPTS: usize = 3;

fn gateway_router_tls_certificate_refs(kube_client: &Client) -> Result<BTreeSet<(String, String)>, CommandError> {
    let mut desired_refs = BTreeSet::new();

    // Collect secrets referenced by Qovery-managed Ingress resources.
    let ingress_api: Api<Ingress> = Api::all(kube_client.clone());
    let ingress_list = block_on(ingress_api.list(&ListParams::default().labels("qovery.com/service-type=router")))
        .map_err(|e| {
            CommandError::new_from_safe_message(format!(
                "Failed to list Ingress resources for Gateway reconciliation: {e}"
            ))
        })?;

    for ingress in ingress_list.items {
        let Some(namespace) = ingress.metadata.namespace.clone() else {
            continue;
        };
        let Some(spec) = ingress.spec else { continue };
        let Some(tls_entries) = spec.tls else { continue };
        for tls in tls_entries {
            let Some(secret_name) = tls.secret_name else { continue };
            if !secret_name.starts_with("router-tls-") {
                continue;
            }
            desired_refs.insert((namespace.clone(), secret_name));
        }
    }

    // Collect secrets referenced by Qovery-managed ListenerSet resources (when available).
    if let Some(listenerset_version) = kubectl_get_listenerset_served_version(kube_client) {
        let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &listenerset_version, "ListenerSet");
        let api: Api<kube::core::DynamicObject> = Api::all_with(kube_client.clone(), &ApiResource::from_gvk(&gvk));
        let listenersets = block_on(api.list(&ListParams::default().labels("qovery.com/service-type=router")))
            .map_err(|e| {
                CommandError::new_from_safe_message(format!(
                    "Failed to list ListenerSet resources for Gateway reconciliation: {e}"
                ))
            })?;

        for ls in listenersets.items {
            let ls_namespace = match ls.metadata.namespace.clone() {
                Some(ns) => ns,
                None => continue,
            };
            let listeners = ls
                .data
                .get("spec")
                .and_then(|spec| spec.get("listeners"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            for listener in listeners {
                let cert_refs = listener
                    .get("tls")
                    .and_then(|tls| tls.get("certificateRefs"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for r in cert_refs {
                    let name = r.get("name").and_then(Value::as_str);
                    if let Some(name) = name {
                        if !name.starts_with("router-tls-") {
                            continue;
                        }
                        let namespace = r
                            .get("namespace")
                            .and_then(Value::as_str)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| ls_namespace.clone());
                        desired_refs.insert((namespace, name.to_string()));
                    }
                }
            }
        }
    }

    live_router_tls_certificate_refs(kube_client, &desired_refs)
}

pub(crate) const GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL: &str = "qovery.com/gateway-fallback-router-tls";
pub(crate) const GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE: &str = "true";
const QOVERY_ENGINE_FIELD_MANAGER: &str = "qovery-engine";

/// Ensures every cross-namespace router TLS Secret has granted the shared Gateway access.
///
/// The Gateway TLS reconciliation runs only for GKE. Its fallback places router TLS Secrets from
/// environment namespaces directly on the shared Gateway, which requires a ReferenceGrant in each
/// Secret namespace before the Gateway certificate reference can be valid.
fn ensure_gateway_reference_grants_for_router_tls_secrets(
    kube_client: &Client,
    gateway_namespace: &str,
    router_tls_certificate_refs: &BTreeSet<(String, String)>,
) -> Result<(), CommandError> {
    if !router_tls_certificate_refs
        .iter()
        .any(|(secret_namespace, _)| secret_namespace != gateway_namespace)
    {
        return Ok(());
    }

    let api_version = kubectl_get_reference_grant_served_version(kube_client).unwrap_or_else(|| "v1beta1".to_string());
    for (secret_namespace, secret_name) in router_tls_certificate_refs {
        if secret_namespace == gateway_namespace {
            continue;
        }
        ensure_gateway_to_secret_reference_grant_with_api_version(
            kube_client,
            &api_version,
            gateway_namespace,
            secret_namespace,
            secret_name,
        )?;
    }

    Ok(())
}

/// Creates or updates the ReferenceGrant allowing a Gateway to use a TLS Secret in another namespace.
pub(crate) fn kubectl_ensure_gateway_to_secret_reference_grant(
    kube_client: &Client,
    gateway_namespace: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<(), CommandError> {
    let api_version = kubectl_get_reference_grant_served_version(kube_client).unwrap_or_else(|| "v1beta1".to_string());
    ensure_gateway_to_secret_reference_grant_with_api_version(
        kube_client,
        &api_version,
        gateway_namespace,
        secret_namespace,
        secret_name,
    )
}

fn ensure_gateway_to_secret_reference_grant_with_api_version(
    kube_client: &Client,
    api_version: &str,
    gateway_namespace: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Result<(), CommandError> {
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", api_version, "ReferenceGrant");
    let api: Api<kube::core::DynamicObject> =
        Api::namespaced_with(kube_client.clone(), secret_namespace, &ApiResource::from_gvk(&gvk));
    let grant_name = format!("allow-gateway-to-{secret_name}");
    let grant =
        gateway_to_secret_reference_grant_manifest(api_version, gateway_namespace, secret_namespace, secret_name);

    block_on(api.patch(
        &grant_name,
        &PatchParams::apply(QOVERY_ENGINE_FIELD_MANAGER),
        &Patch::Apply(&grant),
    ))
    .map_err(|error| {
        CommandError::new_from_safe_message(format!(
            "Failed to apply ReferenceGrant {secret_namespace}/{grant_name}: {error}"
        ))
    })?;

    Ok(())
}

fn gateway_to_secret_reference_grant_manifest(
    api_version: &str,
    gateway_namespace: &str,
    secret_namespace: &str,
    secret_name: &str,
) -> Value {
    let grant_name = format!("allow-gateway-to-{secret_name}");
    json!({
        "apiVersion": format!("gateway.networking.k8s.io/{api_version}"),
        "kind": "ReferenceGrant",
        "metadata": {
            "name": grant_name,
            "namespace": secret_namespace,
            "labels": {
                GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL: GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE
            }
        },
        "spec": {
            "from": [{
                "group": "gateway.networking.k8s.io",
                "kind": "Gateway",
                "namespace": gateway_namespace
            }],
            "to": [{
                "group": "",
                "kind": "Secret",
                "name": secret_name
            }]
        }
    })
}

fn gateway_legacy_reference_grant_certificate_refs(
    kube_client: &Client,
    gateway_namespace: &str,
    fallback_ownership: &BTreeSet<(String, String)>,
) -> Result<BTreeSet<(String, String)>, CommandError> {
    // This is a bounded migration source for fallback references created before the Gateway
    // ownership annotation existed. A live router TLS Secret and its matching ReferenceGrant
    // prove that the fallback is still required even if a cluster-chart upgrade has already
    // reset the dynamically managed Gateway certificateRefs.
    let Some(reference_grant_version) = kubectl_get_reference_grant_served_version(kube_client) else {
        return Ok(BTreeSet::new());
    };
    let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", &reference_grant_version, "ReferenceGrant");
    let api: Api<kube::core::DynamicObject> = Api::all_with(kube_client.clone(), &ApiResource::from_gvk(&gvk));
    let reference_grants = block_on(api.list(&ListParams::default())).map_err(|error| {
        CommandError::new_from_safe_message(format!(
            "Failed to list ReferenceGrant resources for Gateway reconciliation: {error}"
        ))
    })?;

    let mut legacy_refs = BTreeSet::new();
    for reference_grant in &reference_grants.items {
        let Some(namespace) = reference_grant.metadata.namespace.clone() else {
            continue;
        };
        let Some(secret_name) = reference_grant_router_tls_secret_name(reference_grant, gateway_namespace) else {
            continue;
        };
        if fallback_ownership.contains(&(namespace.clone(), secret_name.clone()))
            || !is_engine_gateway_fallback_reference_grant(reference_grant, &secret_name)
        {
            continue;
        }
        let secret_api: Api<Secret> = Api::namespaced(kube_client.clone(), &namespace);
        let secret = block_on(secret_api.get_opt(&secret_name)).map_err(|error| {
            CommandError::new_from_safe_message(format!(
                "Failed to fetch legacy fallback router TLS Secret {namespace}/{secret_name}: {error}"
            ))
        })?;
        if secret.is_some() {
            legacy_refs.insert((namespace, secret_name));
        }
    }

    Ok(legacy_refs)
}

/// Returns whether a fallback ReferenceGrant is managed by the engine.
pub(crate) fn is_engine_gateway_fallback_reference_grant(
    reference_grant: &kube::core::DynamicObject,
    secret_name: &str,
) -> bool {
    if reference_grant
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL))
        .is_some_and(|value| value == GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE)
    {
        return true;
    }

    // Grants created before the label existed can be migrated once. Require both the canonical
    // resource name and the server-side-apply manager used by the engine so unrelated grants
    // cannot be adopted into the shared Gateway.
    reference_grant.metadata.name.as_deref() == Some(&format!("allow-gateway-to-{secret_name}"))
        && reference_grant
            .metadata
            .managed_fields
            .as_ref()
            .is_some_and(|managed_fields| {
                managed_fields.iter().any(|entry| {
                    entry.manager.as_deref() == Some(QOVERY_ENGINE_FIELD_MANAGER)
                        && entry.operation.as_deref() == Some("Apply")
                })
            })
}

fn reference_grant_router_tls_secret_name(
    reference_grant: &kube::core::DynamicObject,
    gateway_namespace: &str,
) -> Option<String> {
    let spec = reference_grant.data.get("spec")?;
    let allows_gateway = spec.get("from").and_then(Value::as_array).is_some_and(|sources| {
        sources.iter().any(|source| {
            source.get("group").and_then(Value::as_str) == Some("gateway.networking.k8s.io")
                && source.get("kind").and_then(Value::as_str) == Some("Gateway")
                && source.get("namespace").and_then(Value::as_str) == Some(gateway_namespace)
        })
    });
    if !allows_gateway {
        return None;
    }

    spec.get("to").and_then(Value::as_array).and_then(|targets| {
        targets.iter().find_map(|target| {
            let name = target.get("name").and_then(Value::as_str)?;
            (target.get("group").and_then(Value::as_str) == Some("")
                && target.get("kind").and_then(Value::as_str) == Some("Secret")
                && name.starts_with("router-tls-"))
            .then(|| name.to_string())
        })
    })
}

const GATEWAY_FALLBACK_CERTIFICATE_REF_ANNOTATION_PREFIX: &str = "qovery.com/gateway-fallback-router-tls-";

pub(crate) fn gateway_fallback_certificate_ref_annotation_key(secret_name: &str) -> String {
    format!(
        "{GATEWAY_FALLBACK_CERTIFICATE_REF_ANNOTATION_PREFIX}{}",
        secret_name.strip_prefix("router-tls-").unwrap_or(secret_name)
    )
}

fn gateway_fallback_certificate_ref_ownership(gateway: &kube::core::DynamicObject) -> BTreeSet<(String, String)> {
    gateway
        .metadata
        .annotations
        .iter()
        .flat_map(|annotations| annotations.iter())
        .filter_map(|(key, namespace)| {
            let router_id = key.strip_prefix(GATEWAY_FALLBACK_CERTIFICATE_REF_ANNOTATION_PREFIX)?;
            (!namespace.is_empty() && !router_id.is_empty())
                .then(|| (namespace.clone(), format!("router-tls-{router_id}")))
        })
        .collect()
}

fn live_gateway_fallback_certificate_ref_ownership(
    kube_client: &Client,
    fallback_ownership: &BTreeSet<(String, String)>,
) -> Result<BTreeSet<(String, String)>, CommandError> {
    live_router_tls_certificate_refs(kube_client, fallback_ownership)
}

/// Filters router TLS references to Secrets that still exist.
///
/// Ingresses and ListenerSets can outlive their Secrets after a partial router deletion. Keeping
/// their references would make reconciliation re-add dead certificateRefs to the shared Gateway.
fn live_router_tls_certificate_refs(
    kube_client: &Client,
    router_tls_certificate_refs: &BTreeSet<(String, String)>,
) -> Result<BTreeSet<(String, String)>, CommandError> {
    let mut live_refs = BTreeSet::new();

    for (namespace, secret_name) in router_tls_certificate_refs {
        let api: Api<Secret> = Api::namespaced(kube_client.clone(), namespace);
        let secret = block_on(api.get_opt(secret_name)).map_err(|error| {
            CommandError::new_from_safe_message(format!(
                "Failed to fetch router TLS Secret {namespace}/{secret_name} during Gateway reconciliation: {error}"
            ))
        })?;
        if secret.is_some() {
            live_refs.insert((namespace.clone(), secret_name.clone()));
        }
    }

    Ok(live_refs)
}

fn gateway_certificate_refs_reconciliation_patch(
    gateway: &kube::core::DynamicObject,
    listener_name: &str,
    desired_refs: &BTreeSet<(String, String)>,
    legacy_fallback_refs: &BTreeSet<(String, String)>,
    stale_fallback_ownership: &BTreeSet<(String, String)>,
    gateway_namespace: &str,
) -> Result<Option<json_patch::Patch>, CommandError> {
    let resource_version = gateway.metadata.resource_version.as_ref().ok_or_else(|| {
        CommandError::new_from_safe_message(format!(
            "Gateway {} has no resourceVersion",
            gateway.metadata.name.as_deref().unwrap_or("unknown")
        ))
    })?;
    let listeners = gateway
        .data
        .get("spec")
        .and_then(|spec| spec.get("listeners"))
        .and_then(Value::as_array)
        .ok_or_else(|| CommandError::new_from_safe_message("Gateway has no spec.listeners".to_string()))?;

    let listener_index = listeners
        .iter()
        .position(|listener| listener.get("name").and_then(Value::as_str) == Some(listener_name))
        .ok_or_else(|| CommandError::new_from_safe_message(format!("Gateway has no '{listener_name}' listener")))?;
    let listener = listeners
        .get(listener_index)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            CommandError::new_from_safe_message(format!("Gateway listener '{listener_name}' is not an object"))
        })?;
    let listener_path = format!("/spec/listeners/{listener_index}");

    let mut patch_operations = vec![PatchOperation::Test(TestOperation {
        path: json_pointer("/metadata/resourceVersion")?,
        value: Value::String(resource_version.clone()),
    })];
    let ownership_patch_operations =
        gateway_fallback_ownership_patch_operations(gateway, legacy_fallback_refs, stale_fallback_ownership)?;
    let mut certificate_refs = match listener.get("tls") {
        None => {
            if desired_refs.is_empty() {
                if ownership_patch_operations.is_empty() {
                    return Ok(None);
                }
                patch_operations.extend(ownership_patch_operations);
                return Ok(Some(json_patch::Patch(patch_operations)));
            }
            let mut certificate_refs = Vec::new();
            reconcile_router_tls_certificate_refs(&mut certificate_refs, desired_refs, gateway_namespace);
            patch_operations.push(PatchOperation::Add(AddOperation {
                path: json_pointer(format!("{listener_path}/tls"))?,
                value: json!({ "mode": "Terminate", "certificateRefs": certificate_refs }),
            }));
            patch_operations.extend(ownership_patch_operations);
            return Ok(Some(json_patch::Patch(patch_operations)));
        }
        Some(tls) => {
            let tls = tls.as_object().ok_or_else(|| {
                CommandError::new_from_safe_message(format!("Gateway listener '{listener_name}' tls is not an object"))
            })?;
            match tls.get("certificateRefs") {
                None => {
                    if desired_refs.is_empty() {
                        if ownership_patch_operations.is_empty() {
                            return Ok(None);
                        }
                        patch_operations.extend(ownership_patch_operations);
                        return Ok(Some(json_patch::Patch(patch_operations)));
                    }
                    let mut certificate_refs = Vec::new();
                    reconcile_router_tls_certificate_refs(&mut certificate_refs, desired_refs, gateway_namespace);
                    patch_operations.push(PatchOperation::Add(AddOperation {
                        path: json_pointer(format!("{listener_path}/tls/certificateRefs"))?,
                        value: Value::Array(certificate_refs),
                    }));
                    patch_operations.extend(ownership_patch_operations);
                    return Ok(Some(json_patch::Patch(patch_operations)));
                }
                Some(certificate_refs) => certificate_refs
                    .as_array()
                    .ok_or_else(|| {
                        CommandError::new_from_safe_message(format!(
                            "Gateway listener '{listener_name}' tls.certificateRefs is not an array"
                        ))
                    })?
                    .clone(),
            }
        }
    };

    let certificate_refs_changed =
        reconcile_router_tls_certificate_refs(&mut certificate_refs, desired_refs, gateway_namespace);
    if !certificate_refs_changed && ownership_patch_operations.is_empty() {
        return Ok(None);
    }
    if certificate_refs_changed {
        patch_operations.push(PatchOperation::Replace(ReplaceOperation {
            path: json_pointer(format!("{listener_path}/tls/certificateRefs"))?,
            value: Value::Array(certificate_refs),
        }));
    }
    patch_operations.extend(ownership_patch_operations);

    Ok(Some(json_patch::Patch(patch_operations)))
}

fn gateway_fallback_ownership_patch_operations(
    gateway: &kube::core::DynamicObject,
    legacy_fallback_refs: &BTreeSet<(String, String)>,
    stale_fallback_ownership: &BTreeSet<(String, String)>,
) -> Result<Vec<PatchOperation>, CommandError> {
    let missing_ownership: Vec<_> = legacy_fallback_refs
        .iter()
        .filter(|(namespace, secret_name)| {
            let key = gateway_fallback_certificate_ref_annotation_key(secret_name);
            gateway
                .metadata
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.get(&key))
                != Some(namespace)
        })
        .collect();
    if missing_ownership.is_empty() && stale_fallback_ownership.is_empty() {
        return Ok(Vec::new());
    }

    match gateway.metadata.annotations.as_ref() {
        Some(_) => {
            let mut patch_operations: Vec<_> = missing_ownership
                .into_iter()
                .map(|(namespace, secret_name)| {
                    let key = gateway_fallback_certificate_ref_annotation_key(secret_name);
                    Ok(PatchOperation::Add(AddOperation {
                        path: json_pointer(format!("/metadata/annotations/{}", json_pointer_segment(&key)))?,
                        value: Value::String(namespace.clone()),
                    }))
                })
                .collect::<Result<_, CommandError>>()?;
            patch_operations.extend(
                stale_fallback_ownership
                    .iter()
                    .map(|(_, secret_name)| {
                        let key = gateway_fallback_certificate_ref_annotation_key(secret_name);
                        Ok(PatchOperation::Remove(RemoveOperation {
                            path: json_pointer(format!("/metadata/annotations/{}", json_pointer_segment(&key)))?,
                        }))
                    })
                    .collect::<Result<Vec<_>, CommandError>>()?,
            );
            Ok(patch_operations)
        }
        None => Ok(vec![PatchOperation::Add(AddOperation {
            path: json_pointer("/metadata/annotations")?,
            value: Value::Object(
                missing_ownership
                    .into_iter()
                    .map(|(namespace, secret_name)| {
                        (
                            gateway_fallback_certificate_ref_annotation_key(secret_name),
                            Value::String(namespace.clone()),
                        )
                    })
                    .collect(),
            ),
        })]),
    }
}

fn json_pointer(path: impl AsRef<str>) -> Result<PointerBuf, CommandError> {
    PointerBuf::parse(path.as_ref())
        .map_err(|error| CommandError::new_from_safe_message(format!("Invalid Gateway patch path: {error}")))
}

fn json_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn is_kubernetes_optimistic_concurrency_conflict(error: &kube::Error) -> bool {
    matches!(error, kube::Error::Api(status) if status.is_conflict())
}

fn reconcile_router_tls_certificate_refs(
    certificate_refs: &mut Vec<Value>,
    desired_refs: &BTreeSet<(String, String)>,
    gateway_namespace: &str,
) -> bool {
    let existing_refs = std::mem::take(certificate_refs);
    let mut retained_refs = Vec::with_capacity(existing_refs.len() + desired_refs.len());
    let mut retained_router_refs = BTreeSet::new();
    let mut mutated = false;

    for reference in existing_refs {
        let Some(name) = reference.get("name").and_then(Value::as_str) else {
            retained_refs.push(reference);
            continue;
        };
        if !name.starts_with("router-tls-") {
            retained_refs.push(reference);
            continue;
        }

        let namespace = reference
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or(gateway_namespace)
            .to_string();
        let identity = (namespace, name.to_string());
        if desired_refs.contains(&identity) && retained_router_refs.insert(identity) {
            retained_refs.push(reference);
        } else {
            mutated = true;
        }
    }

    for (namespace, name) in desired_refs {
        if retained_router_refs.contains(&(namespace.clone(), name.clone())) {
            continue;
        }
        retained_refs.push(json!({
            "kind": "Secret",
            "name": name,
            "namespace": namespace,
        }));
        mutated = true;
    }

    *certificate_refs = retained_refs;
    mutated
}

/// kubectl_get_crash_looping_pods: gets crash looping pods.
///
/// Arguments
///
/// * `kubernetes_config`: kubernetes config file path.
/// * `namespace`: namespace to look into, if None, will look into all namespaces.
/// * `selector`: selector to look for, if None, will look for anything.
/// * `restarted_min_count`: minimum restart counts to be considered as crash looping. If None, default is 5.
/// * `envs`: environment variables to be passed to kubectl.
pub fn kubectl_get_crash_looping_pods<P>(
    kubernetes_config: P,
    namespace: Option<&str>,
    selector: Option<&str>,
    restarted_min_count: Option<usize>,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<KubernetesPod>, CommandError>
where
    P: AsRef<Path>,
{
    let restarted_min = restarted_min_count.unwrap_or(5usize);
    let pods = kubectl_exec_get_pods(kubernetes_config, namespace, selector, envs)?;

    // Pod needs to have at least one container having backoff status (check 1)
    // AND at least a container with minimum restarts (asked in inputs) (check 2)
    let crash_looping_pods = pods
        .items
        .into_iter()
        .filter(|pod| {
            pod.status.container_statuses.as_ref().is_some()
                && pod
                    .status
                    .container_statuses
                    .as_ref()
                    .expect("Cannot get container statuses")
                    .iter()
                    .any(|e| {
                        e.state.waiting.as_ref().is_some()
                        && e.state.waiting.as_ref().expect("cannot get container state").reason == KubernetesPodStatusReason::CrashLoopBackOff // check 1
                        && e.restart_count >= restarted_min // check 2
                    })
        })
        .collect::<Vec<KubernetesPod>>();

    Ok(crash_looping_pods)
}

/// kubectl_exec_delete_pod: allow to delete a k8s pod if exists.
///
/// Arguments
///
/// * `kubernetes_config`: kubernetes config file path.
/// * `pod_namespace`: pod's namespace.
/// * `pod_name`: pod's name.
/// * `envs`: environment variables to be passed to kubectl.
pub fn kubectl_exec_delete_pod<P>(
    kubernetes_config: P,
    pod_namespace: &str,
    pod_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesPod, CommandError>
where
    P: AsRef<Path>,
{
    let pod_to_be_deleted =
        kubectl_exec_get_pod_by_name(&kubernetes_config, Some(pod_namespace), pod_name, envs.clone())?;

    let mut complete_envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        complete_envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    complete_envs.extend(envs);

    match kubectl_exec_with_output(
        vec![
            "delete",
            "pod",
            pod_to_be_deleted.metadata.name.as_str(),
            "-n",
            pod_to_be_deleted.metadata.namespace.as_str(),
        ],
        complete_envs,
        &mut |_| {},
        &mut |_| {},
    ) {
        Ok(_) => Ok(pod_to_be_deleted),
        Err(e) => Err(e),
    }
}

fn kubectl_exec<P, T>(args: Vec<&str>, kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<T, CommandError>
where
    P: AsRef<Path>,
    T: DeserializeOwned,
{
    let mut extended_envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        extended_envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    extended_envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let mut err_vec = Vec::new();
    kubectl_exec_with_output(
        args.clone(),
        extended_envs.clone(),
        &mut |line| output_vec.push(line),
        &mut |line| {
            err_vec.push(line.to_string());
            error!("{}", line)
        },
    )?;

    let output_string: String = output_vec.join("");

    let result = match serde_json::from_str::<T>(output_string.as_str()) {
        Ok(x) => x,
        Err(err) => {
            return Err(CommandError::new(
                "JSON parsing error on kubectl command.".to_string(),
                Some(err.to_string()),
                Some(
                    extended_envs
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect::<Vec<(String, String)>>(),
                ),
            ));
        }
    };

    Ok(result)
}

fn kubectl_exec_raw_output<P>(
    args: Vec<&str>,
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    keep_format: bool,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    let kubernetes_config = kubernetes_config.as_ref();
    if kubernetes_config.exists() {
        _envs.push((KUBECONFIG, kubernetes_config.to_str().unwrap()));
    }
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    kubectl_exec_with_output(args.clone(), _envs.clone(), &mut |line| output_vec.push(line), &mut |line| {
        error!("{}", line)
    })?;

    match keep_format {
        true => Ok(output_vec.join("\n")),
        false => Ok(output_vec.join("")),
    }
}

pub fn kubernetes_get_all_pdbs<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
) -> Result<PDB, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["get", "pdb", "-o", "json"];

    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    kubectl_exec::<P, PDB>(cmd_args, kubernetes_config, envs)
}

pub fn kubernetes_is_metrics_server_working<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<MetricsServer, CommandError>
where
    P: AsRef<Path>,
{
    let cmd_args = vec!["get", "--raw", "/apis/metrics.k8s.io"];

    kubectl_exec::<P, MetricsServer>(cmd_args, kubernetes_config, envs)
}

pub fn kubectl_get_resource_yaml<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    resource: &str,
    namespace: Option<&str>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["get", resource, "-oyaml"];
    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    kubectl_exec_raw_output(cmd_args, kubernetes_config, envs, true)
}

pub fn kubectl_apply_with_path<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    file_path: &str,
    args: Option<Vec<&str>>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["apply"];

    if let Some(args) = args {
        for arg in args {
            cmd_args.push(arg)
        }
    }

    cmd_args.push("-f");
    cmd_args.push(file_path);

    kubectl_exec_raw_output::<P>(cmd_args, kubernetes_config, envs, false)
}

/// kubectl_apply_with_server_side_apply: apply a kubernetes manifest with server side apply.
pub fn kubectl_apply_with_server_side_apply<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    args: Option<Vec<&str>>,
    template: &str,
    force_conflicts: bool,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["apply"];

    if let Some(args) = args {
        for arg in args {
            cmd_args.push(arg)
        }
    }

    if force_conflicts {
        cmd_args.push("--force-conflicts");
    }

    cmd_args.push("--server-side");
    cmd_args.push("-f");

    // write the template in a temporary file
    let tmp_file = tempfile::NamedTempFile::new().map_err(|e| {
        CommandError::new(
            "Error while creating temporary file for kubectl apply.".to_string(),
            Some(e.to_string()),
            None,
        )
    })?;
    std::fs::write(tmp_file.path(), template).map_err(|e| {
        CommandError::new(
            "Error while writing to temporary file for kubectl apply.".to_string(),
            Some(e.to_string()),
            None,
        )
    })?;

    cmd_args.push(tmp_file.path().to_str().unwrap_or_default());

    kubectl_exec_raw_output::<P>(cmd_args, kubernetes_config, envs, false)
}

pub fn kubectl_create_secret<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
    secret_name: String,
    key: String,
    value: String,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let secret_arg = format!("--from-literal={key}=\"{value}\"");
    let mut cmd_args = vec!["create", "secret", "generic", secret_name.as_str(), secret_arg.as_str()];
    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    kubectl_exec_raw_output(cmd_args, kubernetes_config, envs, false)
}

pub fn kubectl_delete_secret<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
    secret_name: String,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["delete", "secret", secret_name.as_str()];
    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    kubectl_exec_raw_output(cmd_args, kubernetes_config, envs, false)
}

pub fn kubectl_create_secret_from_file<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
    backup_name: String,
    key: String,
    file_path: String,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut file = File::open(file_path.as_str()).unwrap();
    let mut content = String::new();
    let _ = file.read_to_string(&mut content);

    kubectl_create_secret(kubernetes_config, envs, namespace, backup_name, key, content)
}

pub fn kubectl_get_completed_jobs<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesJob>, CommandError>
where
    P: AsRef<Path>,
{
    let cmd_args = vec![
        "get",
        "jobs",
        "--all-namespaces",
        "--field-selector",
        "status.successful=1",
        "-o",
        "json",
    ];

    kubectl_exec::<P, KubernetesList<KubernetesJob>>(cmd_args, kubernetes_config, envs)
}

pub fn kubectl_delete_completed_jobs<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    ignored_namespaces: Option<Vec<&str>>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let jobs = kubectl_get_completed_jobs(&kubernetes_config, envs.clone())?;

    if jobs.items.is_empty() {
        return Ok("No completed job to delete.".to_string());
    }
    let mut field_selectors = vec!["status.successful=1".to_string()];
    if let Some(ignored_namespaces) = ignored_namespaces {
        for namespace in ignored_namespaces {
            field_selectors.push(format!(",metadata.namespace!={namespace}"));
        }
    }
    let field_selectors_arg = field_selectors.join("");
    let cmd_args = vec![
        "delete",
        "jobs",
        "--all-namespaces",
        "--field-selector",
        field_selectors_arg.as_str(),
    ];

    kubectl_exec_raw_output(cmd_args, kubernetes_config, envs, false)
}

pub fn kubectl_get_secret(kube_client: Client, fields_selector: &str) -> Result<Vec<Secret>, CommandError> {
    let secrets: Api<Secret> = Api::all(kube_client);

    match block_on(secrets.list(&ListParams::default().fields(fields_selector))) {
        Ok(secret_results) => {
            if secret_results.items.is_empty() {
                return Err(CommandError::new_from_safe_message(format!(
                    "No Secret found with fields selector `{fields_selector}`"
                )));
            }

            Ok(secret_results.items)
        }
        Err(e) => Err(CommandError::new(
            format!("Error trying to get Secret for fields selector `{fields_selector}`"),
            Some(e.to_string()),
            None,
        )),
    }
}

/// kubectl_exec_delete_job: allow to delete a k8s job if exists.
///
/// Arguments
///
/// * `kube_client`: kubernetes API client.
/// * `job_selector`: job's selector.
pub fn kubectl_exec_delete_job(
    kube_client: &Client,
    job_selector: &str,
    namespace: Option<&str>,
) -> Result<(), CommandError> {
    let jobs_api: Api<Job> = match namespace {
        Some(ns) => Api::namespaced(kube_client.clone(), ns),
        None => Api::all(kube_client.clone()),
    };

    match block_on(jobs_api.delete_collection(
        &DeleteParams {
            propagation_policy: Some(PropagationPolicy::Foreground), // deletes linked pods
            ..Default::default()
        },
        &ListParams::default().labels(job_selector),
    )) {
        Ok(_) => Ok(()),
        Err(e) => Err(CommandError::new(
            format!("Error while trying to delete job with selector`{job_selector}`"),
            Some(e.to_string()),
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL, GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE,
        KubernetesServicePortForwardTarget, gateway_certificate_refs_reconciliation_patch,
        gateway_fallback_certificate_ref_ownership, gateway_to_secret_reference_grant_manifest,
        is_engine_gateway_fallback_reference_grant, is_kubernetes_optimistic_concurrency_conflict,
        reconcile_router_tls_certificate_refs,
    };
    use kube::api::ApiResource;
    use kube::core::{DynamicObject, GroupVersionKind, Status};
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use url::Url;

    #[test]
    fn parses_kubernetes_service_url() {
        let url = Url::parse("http://prometheus-operated.prometheus.svc.cluster.local:9090").unwrap();

        assert_eq!(
            KubernetesServicePortForwardTarget::from_service_url(&url),
            Some(KubernetesServicePortForwardTarget {
                namespace: "prometheus".to_string(),
                service_name: "prometheus-operated".to_string(),
                remote_port: 9090,
            })
        );
    }

    #[test]
    fn ignores_external_url() {
        let url = Url::parse("https://thanos.example.com").unwrap();

        assert_eq!(KubernetesServicePortForwardTarget::from_service_url(&url), None);
    }

    #[test]
    fn reconciliation_removes_stale_router_tls_references_and_preserves_platform_certificates() {
        let mut certificate_refs = vec![
            json!({
                "kind": "Secret",
                "name": "letsencrypt-acme-qovery-cert",
                "namespace": "cert-manager",
            }),
            json!({
                "kind": "Secret",
                "name": "router-tls-zactive",
                "namespace": "environment-a",
            }),
            json!({
                "kind": "Secret",
                "name": "router-tls-zstale",
                "namespace": "environment-deleted",
            }),
        ];
        let desired_refs = BTreeSet::from([("environment-a".to_string(), "router-tls-zactive".to_string())]);

        assert!(reconcile_router_tls_certificate_refs(
            &mut certificate_refs,
            &desired_refs,
            "qovery"
        ));
        assert_eq!(
            certificate_refs,
            vec![
                json!({
                    "kind": "Secret",
                    "name": "letsencrypt-acme-qovery-cert",
                    "namespace": "cert-manager",
                }),
                json!({
                    "kind": "Secret",
                    "name": "router-tls-zactive",
                    "namespace": "environment-a",
                }),
            ]
        );
    }

    #[test]
    fn reconciliation_patch_replaces_only_certificate_refs_after_testing_resource_version() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "tls": {
                        "certificateRefs": [
                            { "name": "letsencrypt-acme-qovery-cert", "namespace": "qovery" },
                            { "name": "router-tls-zstale", "namespace": "environment-deleted" }
                        ]
                    }
                }]
            }
        });

        let patch = gateway_certificate_refs_reconciliation_patch(
            &gateway,
            "https",
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            "qovery",
        )
        .expect("reconciliation patch creation should succeed")
        .expect("the stale router certificateRef should produce a patch");

        assert_eq!(
            serde_json::to_value(&patch).expect("JSON Patch should serialize"),
            json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": "42" },
                {
                    "op": "replace",
                    "path": "/spec/listeners/0/tls/certificateRefs",
                    "value": [{ "name": "letsencrypt-acme-qovery-cert", "namespace": "qovery" }]
                }
            ])
        );
    }

    #[test]
    fn reconciliation_does_not_add_tls_to_a_non_tls_listener_without_router_references() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "protocol": "HTTPS",
                    "port": 443
                }]
            }
        });

        let patch = gateway_certificate_refs_reconciliation_patch(
            &gateway,
            "https",
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            "qovery",
        )
        .expect("reconciliation patch creation should succeed");

        assert!(patch.is_none());
    }

    #[test]
    fn reconciliation_derives_fallback_references_from_gateway_ownership_annotations() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.annotations = Some(BTreeMap::from([(
            "qovery.com/gateway-fallback-router-tls-z1234567".to_string(),
            "environment".to_string(),
        )]));

        assert_eq!(
            gateway_fallback_certificate_ref_ownership(&gateway),
            BTreeSet::from([("environment".to_string(), "router-tls-z1234567".to_string())])
        );
    }

    #[test]
    fn reference_grant_manifest_authorizes_the_shared_gateway_for_a_router_tls_secret() {
        assert_eq!(
            gateway_to_secret_reference_grant_manifest("v1", "qovery", "environment-a", "router-tls-z1234567",),
            json!({
                "apiVersion": "gateway.networking.k8s.io/v1",
                "kind": "ReferenceGrant",
                "metadata": {
                    "name": "allow-gateway-to-router-tls-z1234567",
                    "namespace": "environment-a",
                    "labels": {
                        "qovery.com/gateway-fallback-router-tls": "true"
                    }
                },
                "spec": {
                    "from": [{
                        "group": "gateway.networking.k8s.io",
                        "kind": "Gateway",
                        "namespace": "qovery"
                    }],
                    "to": [{
                        "group": "",
                        "kind": "Secret",
                        "name": "router-tls-z1234567"
                    }]
                }
            })
        );
    }

    #[test]
    fn only_adopts_labeled_or_engine_managed_fallback_reference_grants() {
        let api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "ReferenceGrant"));
        let mut labeled_grant = DynamicObject::new("external-grant", &api_resource);
        labeled_grant.metadata.labels = Some(BTreeMap::from([(
            GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL.to_string(),
            GATEWAY_FALLBACK_REFERENCE_GRANT_LABEL_VALUE.to_string(),
        )]));
        assert!(is_engine_gateway_fallback_reference_grant(
            &labeled_grant,
            "router-tls-z1234567",
        ));

        let mut legacy_grant = DynamicObject::new("allow-gateway-to-router-tls-z1234567", &api_resource);
        legacy_grant.metadata.managed_fields =
            Some(vec![k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry {
                manager: Some("qovery-engine".to_string()),
                operation: Some("Apply".to_string()),
                ..Default::default()
            }]);
        assert!(is_engine_gateway_fallback_reference_grant(&legacy_grant, "router-tls-z1234567",));

        let unrelated_grant = DynamicObject::new("external-grant", &api_resource);
        assert!(!is_engine_gateway_fallback_reference_grant(
            &unrelated_grant,
            "router-tls-z1234567",
        ));
    }

    #[test]
    fn retries_only_kubernetes_conflicts_during_gateway_reconciliation() {
        let conflict = kube::Error::Api(Status::failure("Gateway changed", "Conflict").with_code(409).boxed());
        let validation_error = kube::Error::Api(
            Status::failure("certificateRefs exceeds the maximum", "Invalid")
                .with_code(422)
                .boxed(),
        );

        assert!(is_kubernetes_optimistic_concurrency_conflict(&conflict));
        assert!(!is_kubernetes_optimistic_concurrency_conflict(&validation_error));
    }

    #[test]
    fn reconciliation_restores_and_migrates_a_live_legacy_fallback_reference() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "tls": {
                        "certificateRefs": []
                    }
                }]
            }
        });
        let legacy_fallback_refs = BTreeSet::from([("environment".to_string(), "router-tls-z1234567".to_string())]);

        let patch = gateway_certificate_refs_reconciliation_patch(
            &gateway,
            "https",
            &legacy_fallback_refs,
            &legacy_fallback_refs,
            &BTreeSet::new(),
            "qovery",
        )
        .expect("reconciliation patch creation should succeed")
        .expect("the legacy fallback reference should be migrated");

        assert_eq!(
            serde_json::to_value(&patch).expect("JSON Patch should serialize"),
            json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": "42" },
                {
                    "op": "replace",
                    "path": "/spec/listeners/0/tls/certificateRefs",
                    "value": [{
                        "kind": "Secret",
                        "name": "router-tls-z1234567",
                        "namespace": "environment"
                    }]
                },
                {
                    "op": "add",
                    "path": "/metadata/annotations",
                    "value": {
                        "qovery.com/gateway-fallback-router-tls-z1234567": "environment"
                    }
                }
            ])
        );
    }

    #[test]
    fn reconciliation_prunes_a_stale_fallback_reference_and_its_ownership_marker_together() {
        let gateway_api_resource =
            ApiResource::from_gvk(&GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", "Gateway"));
        let mut gateway = DynamicObject::new("qovery-cluster-public-gateway", &gateway_api_resource);
        gateway.metadata.resource_version = Some("42".to_string());
        gateway.metadata.annotations = Some(BTreeMap::from([(
            "qovery.com/gateway-fallback-router-tls-z1234567".to_string(),
            "environment".to_string(),
        )]));
        gateway.data = json!({
            "spec": {
                "listeners": [{
                    "name": "https",
                    "tls": {
                        "certificateRefs": [{
                            "kind": "Secret",
                            "name": "router-tls-z1234567",
                            "namespace": "environment"
                        }]
                    }
                }]
            }
        });
        let stale_fallback_ownership = BTreeSet::from([("environment".to_string(), "router-tls-z1234567".to_string())]);

        let patch = gateway_certificate_refs_reconciliation_patch(
            &gateway,
            "https",
            &BTreeSet::new(),
            &BTreeSet::new(),
            &stale_fallback_ownership,
            "qovery",
        )
        .expect("reconciliation patch creation should succeed")
        .expect("the stale fallback should be pruned");

        assert_eq!(
            serde_json::to_value(&patch).expect("JSON Patch should serialize"),
            json!([
                { "op": "test", "path": "/metadata/resourceVersion", "value": "42" },
                {
                    "op": "replace",
                    "path": "/spec/listeners/0/tls/certificateRefs",
                    "value": []
                },
                {
                    "op": "remove",
                    "path": "/metadata/annotations/qovery.com~1gateway-fallback-router-tls-z1234567"
                }
            ])
        );
    }
}
