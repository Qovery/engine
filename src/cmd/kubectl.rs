use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use dirs::home_dir;
use retry::delay::Fibonacci;
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cmd::structs::{
    Item, KubernetesJob, KubernetesList, KubernetesNode, KubernetesPod, KubernetesPodStatusPhase,
    KubernetesService,
};
use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::{KUBECONFIG, TF_PLUGIN_CACHE_DIR};
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

/*#[derive(DeserializeQuery)]
struct PodDescribe {
    #[query(".status.containerStatuses[0]..restartCount")]
    pub restart_count: u32,
}*/

pub fn kubectl_exec_get_number_of_restart<P>(
    kubernetes_config: P,
    namespace: &str,
    podname: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, SimpleError>
where
    P: AsRef<Path>,
{
    let mut output_vec: Vec<String> = Vec::new();
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec![
            "get",
            "po",
            podname,
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

pub fn kubectl_exec_get_external_ingress_hostname<P>(
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

    let result =
        match serde_json::from_str::<KubernetesList<KubernetesService>>(output_string.as_str()) {
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
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec!["get", "pod", "-o", "json", "-n", namespace, "-l", selector],
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

    let result = match serde_json::from_str::<KubernetesList<KubernetesPod>>(output_string.as_str())
    {
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
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec!["get", "job", "-o", "json", "-n", namespace, job_name],
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

    let result = match serde_json::from_str::<KubernetesJob>(output_string.as_str()) {
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

    if result.status.succeeded > 0 {
        return Ok(Some(true));
    }

    Ok(Some(false))
}

pub fn kubectl_exec_create_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

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

    Ok(())
}

// used for testing the does_contain_terraform_tfstate

pub fn create_sample_secret_terraform_in_namespace<P>(
    kubernetes_config: P,
    namespace_to_override: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<String, SimpleError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    let mut output_vec: Vec<String> = Vec::new();
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);
    let _ = kubectl_exec_with_output(
        vec![
            "create",
            "secret",
            "tfstate-default-state",
            "--from-literal=blablablabla",
            "--namespace",
            namespace_to_override,
        ],
        _envs,
        |out| match out {
            Ok(_line) => output_vec.push(_line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(_line) => {}
            Err(err) => error!("{:?}", err),
        },
    );
    Ok(output_vec.join(""))
}

pub fn does_contain_terraform_tfstate<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<bool, SimpleError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);
    let mut exist = true;
    let _ = kubectl_exec_with_output(
        vec![
            "describe",
            "secrets/tfstate-default-state",
            "--namespace",
            namespace,
        ],
        _envs,
        |out| match out {
            Ok(_line) => exist = true,
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(_line) => {}
            Err(err) => error!("{:?}", err),
        },
    )?;
    Ok(exist)
}

pub fn kubectl_exec_get_all_namespaces<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<String>, SimpleError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::new();
    let _ = kubectl_exec_with_output(
        vec!["get", "namespaces", "-o", "json"],
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

    let mut output_string: String = output_vec.join("");
    let mut to_return: Vec<String> = Vec::new();
    let result = serde_json::from_str::<KubernetesList<Item>>(output_string.as_str());
    match result {
        Ok(out) => {
            for item in out.items {
                to_return.push(item.metadata.name);
            }
        }
        Err(e) => {
            error!(
                "Error while deserializing Kubernetes namespaces names {}",
                e
            );

            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(output_string),
            ));
        }
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
        Err(e) => warn!(
            "Unable to execute describe on secrets: {}. it may not exist anymore?",
            e.message.unwrap_or("".into())
        ),
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
) -> Result<String, SimpleError>
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

    Ok(output_vec.join("\n"))
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
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        vec!["get", "node", "-o", "json"],
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

    let result =
        match serde_json::from_str::<KubernetesList<KubernetesNode>>(output_string.as_str()) {
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
