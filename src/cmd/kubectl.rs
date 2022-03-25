use std::path::Path;

use retry::delay::Fibonacci;
use retry::OperationResult;
use serde::de::DeserializeOwned;

use crate::cloud_provider::digitalocean::models::svc::DoLoadBalancer;
use crate::cloud_provider::metrics::KubernetesApiMetrics;
use crate::cmd::command::QoveryCommand;
use crate::cmd::structs::{
    Configmap, Daemonset, Item, KubernetesEvent, KubernetesJob, KubernetesKind, KubernetesList, KubernetesNode,
    KubernetesPod, KubernetesPodStatusPhase, KubernetesPodStatusReason, KubernetesService, KubernetesVersion,
    LabelsContent, Namespace, Secrets, HPA, PDB, PVC, SVC,
};
use crate::constants::KUBECONFIG;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::errors::CommandError;

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
    pod_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec![
            "get",
            "po",
            pod_name,
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

pub fn do_kubectl_exec_describe_service<P>(
    kubernetes_config: P,
    namespace: &str,
    service_name: &str,
    input_envs: Vec<(&str, &str)>,
) -> Result<DoLoadBalancer, CommandError>
where
    P: AsRef<Path>,
{
    let mut envs = Vec::with_capacity(input_envs.len() + 1);
    envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    envs.extend(input_envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let mut err_output_vec: Vec<String> = Vec::with_capacity(20);
    let cmd_args = vec!["get", "svc", "-n", namespace, service_name, "-o", "json"];
    let _ = kubectl_exec_with_output(
        cmd_args.clone(),
        envs.clone(),
        &mut |line| output_vec.push(line),
        &mut |line| err_output_vec.push(line),
    )?;

    let output_string: String = output_vec.join("\n");
    let err_output_string: String = output_vec.join("\n");

    match serde_json::from_str::<DoLoadBalancer>(output_string.as_str()) {
        Ok(x) => Ok(x),
        Err(err) => Err(CommandError::new_from_command_line(
            format!("Error while executing kubectl command: {:?}", err),
            "kubectl".to_string(),
            cmd_args.into_iter().map(|a| a.to_string()).collect(),
            envs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            Some(output_string.to_string()),
            Some(err_output_string),
        )),
    }
}

// Get ip external ingress
pub fn do_kubectl_exec_get_external_ingress_ip<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<String>, CommandError>
where
    P: AsRef<Path>,
{
    match do_kubectl_exec_describe_service(kubernetes_config, namespace, selector, envs) {
        Ok(result) => {
            if result.status.load_balancer.ingress.is_empty() {
                return Ok(None);
            }

            Ok(Some(result.status.load_balancer.ingress.first().unwrap().ip.clone()))
        }
        Err(e) => Err(e),
    }
}

pub fn do_kubectl_exec_get_loadbalancer_id<P>(
    kubernetes_config: P,
    namespace: &str,
    service_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<String>, CommandError>
where
    P: AsRef<Path>,
{
    match do_kubectl_exec_describe_service(kubernetes_config, namespace, service_name, envs) {
        Ok(result) => {
            if result.status.load_balancer.ingress.is_empty() {
                return Ok(None);
            }

            Ok(Some(
                result.metadata.annotations.kubernetes_digitalocean_com_load_balancer_id,
            ))
        }
        Err(e) => Err(e),
    }
}

pub fn kubectl_exec_get_external_ingress_hostname<P>(
    kubernetes_config: P,
    namespace: &str,
    name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<String>, CommandError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec::<P, KubernetesService>(
        vec!["get", "-n", namespace, "svc", name, "-o", "json"],
        kubernetes_config,
        envs,
    )?;

    if result.status.load_balancer.ingress.is_empty() {
        return Ok(None);
    }

    Ok(Some(
        result.status.load_balancer.ingress.first().unwrap().hostname.clone(),
    ))
}

pub fn kubectl_exec_is_pod_ready_with_retry<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fibonacci::from_millis(3000).take(10), || {
        let r = crate::cmd::kubectl::kubectl_exec_is_pod_ready(
            kubernetes_config.as_ref(),
            namespace,
            selector,
            envs.clone(),
        );

        match r {
            Ok(is_ready) => match is_ready {
                Some(true) => OperationResult::Ok(true),
                _ => {
                    let t = format!("pod with selector: {} is not ready yet", selector);
                    info!("{}", t.as_str());
                    OperationResult::Retry(t)
                }
            },
            Err(err) => OperationResult::Err(format!("command error: {:?}", err)),
        }
    });

    match result {
        Err(err) => match err {
            retry::Error::Operation {
                error: _,
                total_delay: _,
                tries: _,
            } => Ok(Some(false)),
            retry::Error::Internal(err) => Err(CommandError::new_from_safe_message(err)),
        },
        Ok(_) => Ok(Some(true)),
    }
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

pub fn kubectl_exec_is_pod_ready<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, CommandError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec_get_pods(kubernetes_config, Some(namespace), Some(selector), envs)?;

    if result.items.is_empty() || result.items.first().unwrap().status.container_statuses.is_none() {
        return Ok(None);
    }

    let first_item = result.items.first().unwrap();

    let is_ready = matches!(first_item.status.phase, KubernetesPodStatusPhase::Running);
    Ok(Some(is_ready))
}

pub fn kubectl_exec_is_job_ready_with_retry<P>(
    kubernetes_config: P,
    namespace: &str,
    job_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, SimpleError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fibonacci::from_millis(3000).take(10), || {
        let r = crate::cmd::kubectl::kubectl_exec_is_job_ready(
            kubernetes_config.as_ref(),
            namespace,
            job_name,
            envs.clone(),
        );

        match r {
            Ok(is_ready) => match is_ready {
                Some(true) => OperationResult::Ok(true),
                _ => {
                    let t = format!("job {} is not ready yet", job_name);
                    info!("{}", t.as_str());
                    OperationResult::Retry(t)
                }
            },
            Err(err) => OperationResult::Err(format!("command error: {:?}", err)),
        }
    });

    match result {
        Err(err) => match err {
            retry::Error::Operation {
                error: _,
                total_delay: _,
                tries: _,
            } => Ok(Some(false)),
            retry::Error::Internal(err) => Err(SimpleError::new(SimpleErrorKind::Other, Some(err))),
        },
        Ok(_) => Ok(Some(true)),
    }
}

pub fn kubectl_exec_is_job_ready<P>(
    kubernetes_config: P,
    namespace: &str,
    job_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, CommandError>
where
    P: AsRef<Path>,
{
    let job_result = kubectl_exec::<P, KubernetesJob>(
        vec!["get", "job", "-o", "json", "-n", namespace, job_name],
        kubernetes_config,
        envs,
    )?;

    if job_result.status.succeeded > 0 {
        return Ok(Some(true));
    }

    Ok(Some(false))
}

pub fn kubectl_exec_is_namespace_present<P>(kubernetes_config: P, namespace: &str, envs: Vec<(&str, &str)>) -> bool
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let result = kubectl_exec_with_output(
        vec!["get", "namespace", namespace],
        _envs,
        &mut |out| info!("{:?}", out),
        &mut |out| warn!("{:?}", out),
    );

    result.is_ok()
}

pub fn kubectl_exec_create_namespace_without_labels(namespace: &str, kube_config: &str, envs: Vec<(&str, &str)>) {
    let _ = kubectl_exec_create_namespace(kube_config, namespace, None, envs);
}

pub fn kubectl_exec_create_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    labels: Option<Vec<LabelsContent>>,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    // don't create the namespace if already exists and not not return error in this case
    if !kubectl_exec_is_namespace_present(kubernetes_config.as_ref(), namespace, envs.clone()) {
        // create namespace
        let mut _envs = Vec::with_capacity(envs.len() + 1);
        _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
        _envs.extend(envs.clone());

        let _ = kubectl_exec_with_output(
            vec!["create", "namespace", namespace],
            _envs,
            &mut |line| info!("{}", line),
            &mut |line| error!("{}", line),
        )?;
    }

    // additional labels
    if labels.is_some() {
        match kubectl_add_labels_to_namespace(kubernetes_config, namespace, labels.unwrap(), envs) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    };

    Ok(())
}

pub fn kubectl_add_labels_to_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    labels: Vec<LabelsContent>,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    if labels.is_empty() {
        return Err(CommandError::new_from_safe_message(
            "No labels were defined, can't set them".to_string(),
        ));
    };

    if !kubectl_exec_is_namespace_present(kubernetes_config.as_ref(), namespace, envs.clone()) {
        return Err(CommandError::new_from_safe_message(format!(
            "Can't set labels on namespace {} because it doesn't exists",
            namespace
        )));
    }

    let mut command_args = Vec::new();
    let mut labels_string = Vec::new();
    command_args.extend(vec!["label", "namespace", namespace, "--overwrite"]);

    for label in labels.iter() {
        labels_string.push(format! {"{}={}", label.name, label.value});
    }
    let labels_str = labels_string.iter().map(|x| x.as_ref()).collect::<Vec<&str>>();
    command_args.extend(labels_str);

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs.clone());

    let _ = kubectl_exec_with_output(command_args, _envs, &mut |line| info!("{}", line), &mut |line| {
        error!("{}", line)
    })?;

    Ok(())
}

// used for testing the does_contain_terraform_tfstate
pub fn does_contain_terraform_tfstate<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<bool, CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.extend(envs);

    let result = kubectl_exec::<P, KubernetesList<Item>>(
        vec![
            "get",
            "secrets",
            "--namespace",
            namespace,
            "-l",
            "app.kubernetes.io/managed-by=terraform,tfstate=true",
            "-o",
            "json",
        ],
        kubernetes_config,
        _envs,
    );

    match result {
        Ok(out) => {
            if out.items.is_empty() {
                Ok(false)
            } else {
                Ok(true)
            }
        }
        Err(e) => Err(e),
    }
}

pub fn kubectl_exec_get_all_namespaces<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<String>, CommandError>
where
    P: AsRef<Path>,
{
    let result =
        kubectl_exec::<P, KubernetesList<Namespace>>(vec!["get", "namespaces", "-o", "json"], kubernetes_config, envs);

    let mut to_return: Vec<String> = Vec::new();

    match result {
        Ok(out) => {
            for item in out.items {
                to_return.push(item.metadata.name);
            }
        }
        Err(e) => return Err(e),
    };

    Ok(to_return)
}

pub fn kubectl_exec_delete_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    if does_contain_terraform_tfstate(&kubernetes_config, namespace, &envs)? {
        return Err(CommandError::new_from_safe_message(
            "Namespace contains terraform tfstates in secret, can't delete it !".to_string(),
        ));
    }

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["delete", "namespace", namespace],
        _envs,
        &mut |line| info!("{}", line),
        &mut |line| error!("{}", line),
    )?;

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
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
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
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["-n", namespace, "delete", "secret", secret],
        _envs,
        &mut |line| info!("{}", line),
        &mut |line| error!("{}", line),
    )?;

    Ok(())
}

pub fn kubectl_exec_logs<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<String>, CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        vec!["logs", "--tail", "1000", "-n", namespace, "-l", selector],
        _envs,
        &mut |line| output_vec.push(line),
        &mut |line| error!("{}", line),
    )?;

    Ok(output_vec)
}

pub fn kubectl_exec_describe_pod<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, CommandError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        vec!["describe", "pod", "-n", namespace, "-l", selector],
        _envs,
        &mut |line| output_vec.push(line),
        &mut |line| error!("{}", line),
    )?;

    Ok(output_vec.join("\n"))
}

pub fn kubectl_exec_version<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<KubernetesVersion, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, KubernetesVersion>(vec!["version", "-o", "json"], kubernetes_config, envs)
}

pub fn kubectl_exec_get_daemonset<P>(
    kubernetes_config: P,
    name: &str,
    namespace: &str,
    selectors: Option<&str>,
    envs: Vec<(&str, &str)>,
) -> Result<Daemonset, CommandError>
where
    P: AsRef<Path>,
{
    let mut args = vec!["-n", namespace, "get", "daemonset"];
    match selectors {
        Some(x) => {
            args.push("-l");
            args.push(x);
        }
        None => args.push(name),
    };
    args.push("-o");
    args.push("json");

    kubectl_exec::<P, Daemonset>(args, kubernetes_config, envs)
}

pub fn kubectl_exec_rollout_restart_deployment<P>(
    kubernetes_config: P,
    name: &str,
    namespace: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let mut environment_variables: Vec<(&str, &str)> = envs.clone();
    environment_variables.push(("KUBECONFIG", kubernetes_config.as_ref().to_str().unwrap()));
    let args = vec!["-n", namespace, "rollout", "restart", "deployment", name];

    kubectl_exec_with_output(
        args,
        environment_variables.clone(),
        &mut |line| info!("{}", line),
        &mut |line| error!("{}", line),
    )
}

pub fn kubectl_exec_get_node<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesNode>, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, KubernetesList<KubernetesNode>>(vec!["get", "node", "-o", "json"], kubernetes_config, envs)
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

pub fn kubectl_exec_get_json_events<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesEvent>, CommandError>
where
    P: AsRef<Path>,
{
    // Note: can't use app selector with kubectl get event..
    kubectl_exec::<P, KubernetesList<KubernetesEvent>>(
        vec!["get", "event", "-o", "json", "-n", namespace],
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
    environment_variables.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));

    let arg_namespace = match namespace {
        Some(n) => format!("-n {}", n),
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
    let result = kubectl_exec::<P, KubernetesList<Item>>(
        vec!["delete", object, "--all-namespaces", "--all"],
        kubernetes_config,
        envs,
    );

    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            let lower_case_message = e.message().to_lowercase();
            if lower_case_message.contains("no resources found") || lower_case_message.ends_with(" deleted") {
                return Ok(());
            }
            Err(e)
        }
    }
}

/// Get custom metrics values
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `envs` - environment variables required for kubernetes connection
/// * `namespace` - kubernetes namespace
/// * `pod_name` - add a pod name or None to specify all pods
/// * `metric_name` - metric name
pub fn kubectl_exec_api_custom_metrics<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: &str,
    specific_pod_name: Option<&str>,
    metric_name: &str,
) -> Result<KubernetesApiMetrics, CommandError>
where
    P: AsRef<Path>,
{
    let pods = specific_pod_name.unwrap_or("*");
    let api_url = format!(
        "/apis/custom.metrics.k8s.io/v1beta1/namespaces/{}/pods/{}/{}",
        namespace, pods, metric_name
    );
    kubectl_exec::<P, KubernetesApiMetrics>(vec!["get", "--raw", api_url.as_str()], kubernetes_config, envs)
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
    let kind_with_name = format!("{}/{}", kind_formatted, name);

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
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

/// scale down replicas by selector
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `envs` - environment variables required for kubernetes connection
/// * `namespace` - kubernetes namespace
/// * `kind` - kind of kubernetes resource to scale
/// * `selector` - ressources that must match the selector
/// * `replicas_count` - desired number of replicas
pub fn kubectl_exec_scale_replicas_by_selector<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: &str,
    kind: ScalingKind,
    selector: &str,
    replicas_count: u32,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let kind_formatted = match kind {
        ScalingKind::Deployment => "deployment",
        ScalingKind::Statefulset => "statefulset",
    };

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs.clone());

    kubectl_exec_with_output(
        vec![
            "-n",
            namespace,
            "scale",
            "--replicas",
            &replicas_count.to_string(),
            kind_formatted,
            "--selector",
            selector,
        ],
        _envs.clone(),
        &mut |_| {},
        &mut |_| {},
    )?;

    // deleting pdb in order to be able to upgrade kubernetes version
    kubectl_exec_with_output(
        vec!["-n", namespace, "delete", "pdb", "--selector", selector],
        _envs,
        &mut |_| {},
        &mut |_| {},
    )?;

    let condition = match replicas_count {
        0 => PodCondition::Delete,
        _ => PodCondition::Ready,
    };
    kubectl_exec_wait_for_pods_condition(kubernetes_config, envs, namespace, selector, condition)
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
    complete_envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
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
    kubectl_exec::<P, PVC>(
        vec!["get", "pvc", "-o", "json", "-n", namespace],
        kubernetes_config,
        envs,
    )
}

pub fn kubectl_get_svc<P>(kubernetes_config: P, namespace: &str, envs: Vec<(&str, &str)>) -> Result<SVC, CommandError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, SVC>(
        vec!["get", "svc", "-o", "json", "-n", namespace],
        kubernetes_config,
        envs,
    )
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
        match kubectl_get_crash_looping_pods(&kubernetes_config, namespace, selector, None, envs.clone()) {
            Ok(pods) => pods,
            Err(e) => return Err(e),
        };

    for crash_looping_pod in crash_looping_pods.iter() {
        if let Err(e) = kubectl_exec_delete_pod(
            &kubernetes_config,
            crash_looping_pod.metadata.namespace.as_str(),
            crash_looping_pod.metadata.name.as_str(),
            envs.clone(),
        ) {
            return Err(e);
        }
    }

    Ok(crash_looping_pods)
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
        match kubectl_exec_get_pod_by_name(&kubernetes_config, Some(pod_namespace), pod_name, envs.clone()) {
            Ok(pod) => pod,
            Err(e) => return Err(e),
        };

    let mut complete_envs = Vec::with_capacity(envs.len() + 1);
    complete_envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
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
        Err(e) => Err(CommandError::new(e.message(), None)),
    }
}

fn kubectl_exec<P, T>(args: Vec<&str>, kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<T, CommandError>
where
    P: AsRef<Path>,
    T: DeserializeOwned,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        args.clone(),
        _envs.clone(),
        &mut |line| output_vec.push(line),
        &mut |line| error!("{}", line),
    )?;

    let output_string: String = output_vec.join("");

    let result = match serde_json::from_str::<T>(output_string.as_str()) {
        Ok(x) => x,
        Err(err) => {
            let args_string = args.join(" ");
            let mut env_vars_in_vec = Vec::new();
            let _ = _envs.into_iter().map(|x| {
                env_vars_in_vec.push(x.0.to_string());
                env_vars_in_vec.push(x.1.to_string());
            });
            let environment_variables = env_vars_in_vec.join(" ");
            return Err(CommandError::new(
                format!(
                    "JSON parsing error on {:?} on command: {} kubectl {}, output: {}. {:?}",
                    std::any::type_name::<T>(),
                    environment_variables,
                    args_string,
                    output_string,
                    err
                ),
                Some("JSON parsing error on kubectl command.".to_string()),
            ));
        }
    };

    Ok(result)
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

pub fn kubernetes_get_all_hpas<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
) -> Result<HPA, CommandError>
where
    P: AsRef<Path>,
{
    let mut cmd_args = vec!["get", "hpa", "-o", "json"];

    match namespace {
        Some(n) => {
            cmd_args.push("-n");
            cmd_args.push(n);
        }
        None => cmd_args.push("--all-namespaces"),
    }

    kubectl_exec::<P, HPA>(cmd_args, kubernetes_config, envs)
}
