use std::fs::read_to_string;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use curl::easy::Easy;
use dirs::home_dir;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::os::unix::fs::PermissionsExt;
use tracing::Level;
use tracing_subscriber;
use tracing_subscriber::util::SubscriberInitExt;

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cmd;
use qovery_engine::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use qovery_engine::error::{SimpleError, SimpleErrorKind};
use qovery_engine::models::{Context, Environment, Metadata};

use crate::aws::{aws_access_key_id, aws_secret_access_key, KUBE_CLUSTER_ID};

pub fn build_platform_local_docker(context: &Context) -> LocalDocker {
    LocalDocker::new(context.clone(), "oxqlm3r99vwcmvuj", "qovery-local-docker")
}

pub fn init() {
    let collector = tracing_subscriber::fmt()
        // filter spans/events with level TRACE or higher.
        .with_max_level(Level::INFO)
        // build but do not install the subscriber.
        .finish();

    let _ = collector.try_init();

    println!(
        "running from current directory: {}",
        std::env::current_dir().unwrap().to_str().unwrap()
    );
}

pub fn generate_id() -> String {
    // Should follow DNS naming convention https://tools.ietf.org/html/rfc1035
    let uuid;

    loop {
        let rand_string: String = thread_rng().sample_iter(Alphanumeric).take(15).collect();
        if rand_string.chars().next().unwrap().is_alphabetic() {
            uuid = rand_string.to_lowercase();
            break;
        }
    }
    uuid
}

pub fn check_all_connections(env: &Environment) -> Vec<bool> {
    let mut checking: Vec<bool> = Vec::with_capacity(env.routers.len());

    for router_to_test in &env.routers {
        let path_to_test = format!(
            "https://{}{}",
            &router_to_test.default_domain, &router_to_test.routes[0].path
        );

        checking.push(curl_path(path_to_test.as_str()));
    }
    return checking;
}

fn curl_path(path: &str) -> bool {
    let mut easy = Easy::new();
    easy.url(path).unwrap();
    let res = easy.perform();
    match res {
        Ok(_) => return true,

        Err(e) => {
            println!("TEST Error : while trying to call {}", e);
            return false;
        }
    }
}

pub fn context() -> Context {
    let execution_id = execution_id();
    let home_dir = std::env::var("WORKSPACE_ROOT_DIR")
        .unwrap_or(home_dir().unwrap().to_str().unwrap().to_string());
    let lib_root_dir = std::env::var("LIB_ROOT_DIR").expect("LIB_ROOT_DIR is mandatory");
    let metadata = Metadata {
        test: Option::from(true),
        dry_run_deploy: Option::from(false),
        resource_expiration_in_seconds: Some(2700),
    };

    Context::new(
        execution_id.as_str(),
        home_dir.as_str(),
        lib_root_dir.as_str(),
        None,
        Option::from(metadata),
    )
}

fn kubernetes_config_path(
    workspace_directory: &str,
    kubernetes_cluster_id: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<String, SimpleError> {
    let kubernetes_config_bucket_name = format!("qovery-kubeconfigs-{}", kubernetes_cluster_id);
    let kubernetes_config_object_key = format!("{}.yaml", kubernetes_cluster_id);

    let kubernetes_config_file_path = format!(
        "{}/kubernetes_config_{}",
        workspace_directory, kubernetes_cluster_id
    );

    let _ = get_kubernetes_config_file(
        access_key_id,
        secret_access_key,
        kubernetes_config_bucket_name.as_str(),
        kubernetes_config_object_key.as_str(),
        kubernetes_config_file_path.as_str(),
    )?;

    Ok(kubernetes_config_file_path)
}

fn get_kubernetes_config_file<P>(
    access_key_id: &str,
    secret_access_key: &str,
    kubernetes_config_bucket_name: &str,
    kubernetes_config_object_key: &str,
    file_path: P,
) -> Result<File, SimpleError>
where
    P: AsRef<Path>,
{
    // return the file if it already exists
    let _ = match File::open(file_path.as_ref()) {
        Ok(f) => return Ok(f),
        Err(_) => {}
    };

    let file_content_result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
        let file_content = get_object_via_aws_cli(
            access_key_id,
            secret_access_key,
            kubernetes_config_bucket_name,
            kubernetes_config_object_key,
        );

        match file_content {
            Ok(file_content) => OperationResult::Ok(file_content),
            Err(err) => OperationResult::Retry(err),
        }
    });

    let file_content = match file_content_result {
        Ok(file_content) => file_content,
        Err(_) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("file content is empty (retry failed multiple times) - which is not the expected content - what's wrong?"),
            ));
        }
    };

    let mut kubernetes_config_file = File::create(file_path.as_ref())?;
    let _ = kubernetes_config_file.write_all(file_content.as_bytes())?;
    // removes warning kubeconfig is (world/group) readable
    let metadata = kubernetes_config_file.metadata()?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o400);
    std::fs::set_permissions(file_path.as_ref(), permissions)?;
    Ok(kubernetes_config_file)
}

/// gets an aws s3 object using aws-cli
/// used as a failover when rusoto_s3 acts up
fn get_object_via_aws_cli(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
) -> Result<String, SimpleError> {
    let s3_url = format!("s3://{}/{}", bucket_name, object_key);
    let local_path = format!("/tmp/{}", object_key); // FIXME: change hardcoded /tmp/

    qovery_engine::cmd::utilities::exec_with_envs(
        "aws",
        vec!["s3", "cp", &s3_url, &local_path],
        vec![
            (AWS_ACCESS_KEY_ID, access_key_id),
            (AWS_SECRET_ACCESS_KEY, secret_access_key),
        ],
    )?;

    let s = read_to_string(&local_path)?;
    Ok(s)
}

pub fn is_pod_restarted_aws_env(
    environment_check: Environment,
    pod_to_check: &str,
) -> (bool, String) {
    let namespace_name = format!(
        "{}-{}",
        &environment_check.project_id.clone(),
        &environment_check.id.clone(),
    );

    let access_key = aws_access_key_id();
    let secret_key = aws_secret_access_key();
    let aws_credentials_envs = vec![
        ("AWS_ACCESS_KEY_ID", access_key.as_str()),
        ("AWS_SECRET_ACCESS_KEY", secret_key.as_str()),
    ];

    let kubernetes_config = kubernetes_config_path(
        "/tmp",
        KUBE_CLUSTER_ID,
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
    );

    match kubernetes_config {
        Ok(path) => {
            let restarted_database = cmd::kubectl::kubectl_exec_get_number_of_restart(
                path.as_str(),
                namespace_name.clone().as_str(),
                pod_to_check,
                aws_credentials_envs,
            );
            match restarted_database {
                Ok(count) => match count.trim().eq("0") {
                    true => return (true, "0".to_string()),
                    false => return (true, count.to_string()),
                },
                _ => return (false, "".to_string()),
            }
        }
        Err(_e) => return (false, "".to_string()),
    }
}

pub fn execution_id() -> String {
    Utc::now()
        .to_rfc3339()
        .replace(":", "-")
        .replace(".", "-")
        .replace("+", "-")
}
