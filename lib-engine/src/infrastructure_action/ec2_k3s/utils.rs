use crate::cloud_provider::kubeconfig_helper::force_fetch_kubeconfig;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::utilities::{wait_until_port_is_open, TcpCheckSource};
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::infrastructure_action::ec2_k3s::AwsEc2QoveryTerraformOutput;
use crate::object_storage::ObjectStorage;
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

// EC2 instances push themselves the kubeconfig to S3 storage
// We need to be sure the content of the kubeconfig is the correct one (matching the EC2 instance FQDN)
pub fn get_and_check_if_kubeconfig_is_valid(
    kubernetes: &dyn Kubernetes,
    object_store: &dyn ObjectStorage,
    event_details: EventDetails,
    qovery_terraform_config: AwsEc2QoveryTerraformOutput,
) -> Result<PathBuf, Box<EngineError>> {
    let port = match qovery_terraform_config.kubernetes_port_to_u16() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!(
                "Couldn't get the kubernetes port from Terraform config (convertion issue): {}",
                &qovery_terraform_config.aws_ec2_public_hostname
            );
            kubernetes.logger().log(EngineEvent::Error(
                EngineError::new_error_on_cloud_provider_information(
                    event_details.clone(),
                    CommandError::new(msg.clone(), Some(e), None),
                ),
                None,
            ));
            return Err(Box::new(EngineError::new_error_on_cloud_provider_information(
                event_details,
                CommandError::new_from_safe_message(msg),
            )));
        }
    };

    // wait for k3s port to be open
    // retry for 10 min, a reboot will occur after 5 min if nothing happens (see EC2 Terraform user config)
    wait_until_port_is_open(
        &TcpCheckSource::DnsName(qovery_terraform_config.aws_ec2_public_hostname.as_str()),
        port,
        600,
        kubernetes.logger(),
        event_details.clone(),
    )
    .map_err(|_| EngineError::new_k8s_cannot_reach_api(event_details.clone()))?;

    // during an instance replacement, the EC2 host dns will change and will require the kubeconfig to be updated
    // we need to ensure the kubeconfig is the correct one by checking the current instance dns in the kubeconfig
    let result = retry::retry(Fixed::from_millis(5 * 1000).take(120), || {
        // force s3 kubeconfig retrieve
        match force_fetch_kubeconfig(kubernetes, object_store) {
            Ok(p) => p,
            Err(e) => return OperationResult::Retry(e),
        };
        let current_kubeconfig_path = kubernetes.kubeconfig_local_file_path();
        let mut kubeconfig_file = File::open(&current_kubeconfig_path).expect("Cannot open file");

        // ensure the kubeconfig content address match with the current instance dns
        let mut buffer = String::new();
        if let Err(e) = kubeconfig_file.read_to_string(&mut buffer) {
            warn!("Cannot read kubeconfig file, error: {e}");
        }
        match buffer.contains(&qovery_terraform_config.aws_ec2_public_hostname) {
            true => {
                kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "kubeconfig stored on s3 do correspond with the actual host {}",
                        &qovery_terraform_config.aws_ec2_public_hostname
                    )),
                ));
                OperationResult::Ok(current_kubeconfig_path)
            }
            false => {
                kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "kubeconfig stored on s3 do not yet correspond with the actual host {}, retrying in 5 sec...",
                        &qovery_terraform_config.aws_ec2_public_hostname
                    )),
                ));
                OperationResult::Retry(Box::new(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(
                    event_details.clone(),
                )))
            }
        }
    });

    match result {
        Ok(x) => Ok(x),
        Err(Error { error, .. }) => Err(error),
    }
}
