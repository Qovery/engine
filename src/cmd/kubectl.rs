use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{DeleteParams, Patch, PatchParams, PropagationPolicy};
use kube::core::params::ListParams;
use kube::{Api, Client, ResourceExt};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_yaml::Deserializer;
use std::fmt::Debug;
use std::fs::{File, read_dir, read_to_string};
use std::io::Read;
use std::path::Path;
use uuid::Uuid;

use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::cmd::structs::{
    Configmap, KubernetesIngress, KubernetesIngressStatusLoadBalancerIngress, KubernetesJob, KubernetesKind,
    KubernetesList, KubernetesNode, KubernetesPod, KubernetesPodStatusReason, KubernetesVersion, MetricsServer, PDB,
    PVC, SVC, Secrets,
};
use crate::constants::KUBECONFIG;
use crate::errors::{CommandError, ErrorMessageVerbosity};
use crate::runtime::block_on;

pub enum ScalingKind {
    Deployment,
    Statefulset,
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
        let msg = format!("Error on command: kubectl {}. {:?}", args_string, &err);
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
            PodCondition::Delete => format!("{:?}", &condition).to_lowercase(),
            _ => format!("condition={:?}", &condition).to_lowercase(),
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
