use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::azure::AksQoveryTerraformOutput;
use crate::infrastructure::action::eks::AwsEksQoveryTerraformOutput;
use crate::infrastructure::action::gke::GkeQoveryTerraformOutput;
use crate::infrastructure::action::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::infrastructure::action::scaleway::ScalewayQoveryTerraformOutput;
use crate::infrastructure::models::kubernetes::{Kind, Kubernetes};

pub fn update_cluster_outputs<T: IntoClusterOutputsRequest>(
    kube: &dyn Kubernetes,
    tf_output: &T,
) -> Result<(), Box<EngineError>> {
    info!("update_cluster_outputs");
    let cluster_outputs_request: ClusterOutputsRequest = tf_output.to_cluster_outputs_request();
    debug!("update_cluster_outputs request: {:?}", cluster_outputs_request);

    // Upload cluster outputs, so we can store them in the core (it contains the kubeconfig)
    if let Err(err) = kube
        .context()
        .qovery_api
        .update_cluster_outputs(&cluster_outputs_request)
    {
        error!(
            "Cannot update cluster outputs for {}: {}",
            cluster_outputs_request.cluster_id, err
        );
    }

    write_kubeconfig_on_disk(
        &kube.kubeconfig_local_file_path(),
        &cluster_outputs_request.kubeconfig,
        kube.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
    )?;

    Ok(())
}

pub trait IntoClusterOutputsRequest {
    fn to_cluster_outputs_request(&self) -> ClusterOutputsRequest;
}

#[derive(Debug)]
pub struct ClusterOutputsRequest {
    pub kubernetes_kind: Kind,
    pub kubeconfig: String,
    pub cluster_id: String,
    pub cluster_name: String,
    pub cluster_arn: Option<String>,
    pub cluster_oidc_issuer: Option<String>,
    pub cluster_vpc_id: Option<String>,
    pub cluster_self_link: Option<String>,
    pub network: Option<String>,
    pub private_network_id: Option<String>,
}

impl IntoClusterOutputsRequest for AwsEksQoveryTerraformOutput {
    fn to_cluster_outputs_request(&self) -> ClusterOutputsRequest {
        ClusterOutputsRequest {
            kubernetes_kind: Kind::Eks,
            kubeconfig: self.kubeconfig.clone(),
            cluster_id: self.cluster_id.clone(),
            cluster_name: self.cluster_name.clone(),
            cluster_arn: Some(self.cluster_arn.clone()),
            cluster_oidc_issuer: Some(self.cluster_oidc_issuer.clone()),
            cluster_vpc_id: Some(self.cluster_vpc_id.clone()),
            cluster_self_link: None,
            network: None,
            private_network_id: None,
        }
    }
}

impl IntoClusterOutputsRequest for ScalewayQoveryTerraformOutput {
    fn to_cluster_outputs_request(&self) -> ClusterOutputsRequest {
        ClusterOutputsRequest {
            kubernetes_kind: Kind::ScwKapsule,
            kubeconfig: self.kubeconfig.clone(),
            cluster_id: self.cluster_id.clone(),
            cluster_name: self.cluster_name.clone(),
            private_network_id: Some(self.private_network_id.clone()),
            cluster_arn: None,
            cluster_oidc_issuer: None,
            cluster_vpc_id: None,
            cluster_self_link: None,
            network: None,
        }
    }
}

impl IntoClusterOutputsRequest for GkeQoveryTerraformOutput {
    fn to_cluster_outputs_request(&self) -> ClusterOutputsRequest {
        ClusterOutputsRequest {
            kubernetes_kind: Kind::Gke,
            kubeconfig: self.kubeconfig.clone(),
            cluster_id: self.cluster_id.clone(),
            cluster_name: self.cluster_name.clone(),
            cluster_self_link: Some(self.cluster_self_link.clone()),
            network: Some(self.network.clone()),
            cluster_arn: None,
            cluster_oidc_issuer: None,
            cluster_vpc_id: None,
            private_network_id: None,
        }
    }
}

impl IntoClusterOutputsRequest for AksQoveryTerraformOutput {
    fn to_cluster_outputs_request(&self) -> ClusterOutputsRequest {
        ClusterOutputsRequest {
            kubernetes_kind: Kind::Aks,
            kubeconfig: self.kubeconfig.clone(),
            cluster_id: self.cluster_id.clone(),
            cluster_name: self.cluster_name.clone(),
            cluster_oidc_issuer: Some(self.cluster_oidc_issuer.clone()),
            cluster_arn: None,
            cluster_vpc_id: None,
            cluster_self_link: None,
            network: None,
            private_network_id: None,
        }
    }
}
