use chrono::{DateTime, Utc};
use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::gcp::GCP;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Request {
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub build_platform: BuildPlatform,
    pub cloud_provider: CloudProvider,
    pub container_registry: ContainerRegistry,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Action {
    Create,
    Delete,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BuildPlatform {
    pub kind: qovery_engine::build_platform::Kind,
    pub id: String,
    pub name: String,
}

impl BuildPlatform {
    pub fn as_engine_build_platform(
        &self,
    ) -> Box<dyn qovery_engine::build_platform::BuildPlatform> {
        Box::new(match self.kind {
            qovery_engine::build_platform::Kind::LocalDocker => {
                LocalDocker::new(self.id.as_str(), self.name.as_str())
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CloudProvider {
    pub kind: qovery_engine::cloud_provider::Kind,
    pub id: String,
    pub name: String,
    pub kubernetes: Kubernetes,
}

impl CloudProvider {
    pub fn as_engine_cloud_provider(
        &self,
    ) -> Box<dyn qovery_engine::cloud_provider::CloudProvider> {
        match self.kind {
            qovery_engine::cloud_provider::Kind::AWS => {
                Box::new(AWS::new(self.id.as_str(), self.name.as_str(), "", ""))
            }
            qovery_engine::cloud_provider::Kind::GCP => {
                Box::new(GCP::new(self.id.as_str(), self.name.as_str(), ""))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Kubernetes {
    pub kind: qovery_engine::cloud_provider::kubernetes::Kind,
    pub id: String,
    pub name: String,
    pub nodes: Vec<Node>,
}

impl Kubernetes {
    pub fn as_engine_kubernetes<'a>(
        &self,
        cloud_provider: &'a Box<dyn qovery_engine::cloud_provider::CloudProvider>,
        nodes: &Vec<Box<dyn qovery_engine::cloud_provider::kubernetes::KubernetesNode>>,
    ) -> Box<dyn qovery_engine::cloud_provider::kubernetes::Kubernetes + 'a> {
        match self.kind {
            qovery_engine::cloud_provider::kubernetes::Kind::EKS => Box::new(EKS::new(
                self.id.as_str(),
                self.name.as_str(),
                "",
                "",
                cloud_provider.as_any().downcast_ref::<AWS>().unwrap(),
                nodes
                    .into_iter()
                    .map(|x| {
                        qovery_engine::cloud_provider::aws::kubernetes::node::Node::new(
                            x.total_cpu(),
                            x.total_memory_in_gib(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )),
        }
    }

    pub fn as_engine_kubernetes_nodes(
        &self,
    ) -> Vec<Box<dyn qovery_engine::cloud_provider::kubernetes::KubernetesNode>> {
        self.nodes
            .iter()
            .map(|n| n.as_engine_kubernetes_node(self))
            .collect::<Vec<_>>()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Node {
    pub cpu: u8,
    pub memory_in_gib: u16,
}

impl Node {
    pub fn as_engine_kubernetes_node(
        &self,
        kubernetes: &Kubernetes,
    ) -> Box<dyn qovery_engine::cloud_provider::kubernetes::KubernetesNode> {
        match kubernetes.kind {
            qovery_engine::cloud_provider::kubernetes::Kind::EKS => Box::new(
                qovery_engine::cloud_provider::aws::kubernetes::node::Node::new(
                    self.cpu,
                    self.memory_in_gib,
                ),
            ),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ContainerRegistry {
    pub kind: qovery_engine::container_registry::Kind,
    pub id: String,
    pub name: String,
}

impl ContainerRegistry {
    pub fn as_engine_container_registry<'a>(
        &'a self,
    ) -> Box<dyn qovery_engine::container_registry::ContainerRegistry + 'a> {
        match self.kind {
            qovery_engine::container_registry::Kind::DockerHub => {
                Box::new(DockerHub::new(self.id.as_str(), self.name.as_str(), "", ""))
            }
            qovery_engine::container_registry::Kind::ECR => {
                Box::new(ECR::new(self.id.as_str(), self.name.as_str(), "", "", ""))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Response {
    pub created_at: DateTime<Utc>,
    pub message: Option<String>,
}

impl Response {
    pub fn new(message: Option<String>) -> Self {
        Response {
            created_at: Utc::now(),
            message,
        }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}
