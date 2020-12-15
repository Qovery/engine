use std::io::Error;
use std::path::Path;

use retry::delay::Fibonacci;
use retry::OperationResult;
use serde::de::DeserializeOwned;

use crate::cloud_provider::digitalocean::api_structs::svc::DOKubernetesList;
use crate::cmd::structs::{
    Item, KubernetesEvent, KubernetesJob, KubernetesList, KubernetesNode, KubernetesPod,
    KubernetesPodStatusPhase, KubernetesService, LabelsContent,
};
use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::KUBECONFIG;
use crate::error::{SimpleError, SimpleErrorKind};

pub fn kubectl_exec_with_output<F, X>(
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), SimpleError>
where
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    match exec_with_envs_and_output("kubectl", args, envs, stdout_output, stderr_output) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

pub fn kubectl_exec_get_number_of_restart<P>(
    kubernetes_config: P,
    namespace: &str,
    pod_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, SimpleError>
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
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    let output_string: String = output_vec.join("");
    Ok(output_string)
}

// Get ip external ingress
// CAUTION: use it only with DigitalOcean
pub fn do_kubectl_exec_get_external_ingress_ip<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<String>, SimpleError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec![
            "get", "svc", "-o", "json", "-n", namespace, "-l", // selector
            selector,
        ],
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    let output_string: String = output_vec.join("");

    let result = match serde_json::from_str::<DOKubernetesList>(output_string.as_str()) {
        Ok(x) => x,
        Err(err) => {
            error!("{:?}", err);
            error!("{}", output_string.as_str());
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(output_string),
            ));
        }
    };

    if result.items.is_empty()
        || result
            .items
            .first()
            .unwrap()
            .status
            .load_balancer
            .ingress
            .is_empty()
    {
        return Ok(None);
    }

    // FIXME unsafe unwrap here?
    Ok(Some(
        result
            .items
            .first()
            .unwrap()
            .status
            .load_balancer
            .ingress
            .first()
            .unwrap()
            .ip
            .clone(),
    ))
}

pub fn kubectl_exec_get_external_ingress_hostname<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<String>, SimpleError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec::<P, KubernetesList<KubernetesService>>(
        vec![
            "get", "svc", "-o", "json", "-n", namespace, "-l", // selector
            selector,
        ],
        kubernetes_config,
        envs,
    )?;

    if result.items.is_empty()
        || result
            .items
            .first()
            .unwrap()
            .status
            .load_balancer
            .ingress
            .is_empty()
    {
        return Ok(None);
    }

    // FIXME unsafe unwrap here?
    Ok(Some(
        result
            .items
            .first()
            .unwrap()
            .status
            .load_balancer
            .ingress
            .first()
            .unwrap()
            .hostname
            .clone(),
    ))
}

pub fn kubectl_exec_is_pod_ready_with_retry<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, SimpleError>
where
    P: AsRef<Path>,
{
    // TODO check this
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
            retry::Error::Internal(err) => Err(SimpleError::new(SimpleErrorKind::Other, Some(err))),
        },
        Ok(_) => Ok(Some(true)),
    }
}

pub fn kubectl_exec_is_pod_ready<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, SimpleError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec_get_pod(kubernetes_config, namespace, selector, envs)?;

    if result.items.is_empty()
        || result
            .items
            .first()
            .unwrap()
            .status
            .container_statuses
            .is_empty()
    {
        return Ok(None);
    }

    let first_item = result.items.first().unwrap();

    let is_ready = match first_item.status.phase {
        KubernetesPodStatusPhase::Running => true,
        _ => false,
    };

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
    // TODO check this
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
) -> Result<Option<bool>, SimpleError>
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

pub fn kubectl_exec_is_namespace_present<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: Vec<(&str, &str)>,
) -> bool
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::new();
    let result = kubectl_exec_with_output(
        vec!["get", "namespace", namespace],
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => {
                if line.contains("Error from server (NotFound): namespaces") {
                    info!("{}", line)
                } else {
                    error!("{}", line)
                }
            }
            Err(err) => error!("{:?}", err),
        },
    );

    match result {
        Ok(_) => true,
        Err(_) => false,
    }
}

pub fn kubectl_exec_create_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    labels: Option<Vec<LabelsContent>>,
    envs: Vec<(&str, &str)>,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    // don't create the namespace if already exists and not not return error in this case
    if !kubectl_exec_is_namespace_present(
        kubernetes_config.as_ref(),
        namespace.clone(),
        envs.clone(),
    ) {
        // create namespace
        let mut _envs = Vec::with_capacity(envs.len() + 1);
        _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
        _envs.extend(envs.clone());

        let _ = kubectl_exec_with_output(
            vec!["create", "namespace", namespace],
            _envs,
            |out| match out {
                Ok(line) => info!("{}", line),
                Err(err) => error!("{:?}", err),
            },
            |out| match out {
                Ok(line) => error!("{}", line),
                Err(err) => error!("{:?}", err),
            },
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
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    if labels.iter().count() == 0 {
        return Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some("No labels were defined, can't set them"),
        ));
    };

    if !kubectl_exec_is_namespace_present(
        kubernetes_config.as_ref(),
        namespace.clone(),
        envs.clone(),
    ) {
        return Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(format! {"Can't set labels on namespace {} because it doesn't exists", namespace}),
        ));
    }

    let mut command_args = Vec::new();
    let mut labels_string = Vec::new();
    command_args.extend(vec!["label", "namespace", namespace, "--overwrite"]);

    for label in labels.iter() {
        labels_string.push(format! {"{}={}", label.name, label.value});
    }
    let labels_str = labels_string
        .iter()
        .map(|x| x.as_ref())
        .collect::<Vec<&str>>();
    command_args.extend(labels_str);

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs.clone());

    let _ = kubectl_exec_with_output(
        command_args,
        _envs,
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(())
}

// used for testing the does_contain_terraform_tfstate
pub fn does_contain_terraform_tfstate<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<bool, SimpleError>
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
            if out.items.len() == 0 {
                Ok(false)
            } else {
                Ok(true)
            }
        }
        Err(e) => return Err(e),
    }
}

pub fn kubectl_exec_get_all_namespaces<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<String>, SimpleError>
where
    P: AsRef<Path>,
{
    let result = kubectl_exec::<P, KubernetesList<Item>>(
        vec!["get", "namespaces", "-o", "json"],
        kubernetes_config,
        envs,
    );

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
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    match does_contain_terraform_tfstate(&kubernetes_config, &namespace, &envs) {
        Ok(exist) => match exist {
            true => {
                return Err(SimpleError::new(
                    SimpleErrorKind::Other,
                    Some("Namespace contains terraform tfstates in secret, can't delete it !"),
                ));
            }
            false => info!(
                "Namespace {} doesn't contain any tfstates, able to delete it",
                namespace
            ),
        },
        Err(_) => debug!("Unable to execute describe on secrets. it may not exist anymore"),
    };

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["delete", "namespace", namespace],
        _envs,
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(())
}

pub fn kubectl_exec_delete_secret<P>(
    kubernetes_config: P,
    secret: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["delete", "secret", secret],
        _envs,
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(())
}

pub fn kubectl_exec_logs<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<String>, SimpleError>
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
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(output_vec)
}

pub fn kubectl_exec_describe_pod<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, SimpleError>
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
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(output_vec.join("\n"))
}

pub fn kubectl_exec_get_node<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesNode>, SimpleError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, KubernetesList<KubernetesNode>>(
        vec!["get", "node", "-o", "json"],
        kubernetes_config,
        envs,
    )
}

pub fn kubectl_exec_get_pod<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesPod>, SimpleError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, KubernetesList<KubernetesPod>>(
        vec!["get", "pod", "-o", "json", "-n", namespace, "-l", selector],
        kubernetes_config,
        envs,
    )
}

pub fn kubectl_exec_get_event<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<KubernetesList<KubernetesEvent>, SimpleError>
where
    P: AsRef<Path>,
{
    kubectl_exec::<P, KubernetesList<KubernetesEvent>>(
        vec![
            "get", "event", "-o", "json", "-n", namespace, "-l", selector,
        ],
        kubernetes_config,
        envs,
    )
}

fn kubectl_exec<P, T>(
    args: Vec<&str>,
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<T, SimpleError>
where
    P: AsRef<Path>,
    T: DeserializeOwned,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        args,
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    let output_string: String = output_vec.join("");

    let result = match serde_json::from_str::<T>(output_string.as_str()) {
        Ok(x) => x,
        Err(err) => {
            error!("{:?}", err);
            error!("{}", output_string.as_str());
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(output_string),
            ));
        }
    };

    Ok(result)
}
