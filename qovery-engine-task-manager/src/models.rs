use std::borrow::Borrow;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::gcp::GCP;
use qovery_engine::config::Config;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::models::Environment;

#[derive(Serialize, Deserialize, Clone)]
pub struct Request {
    pub id: String,
    pub organization_id: String,
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub build_platform: BuildPlatform,
    pub cloud_provider: CloudProvider,
    pub container_registry: ContainerRegistry,
    pub environment: Option<Environment>,
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
    pub options: Options,
}

impl BuildPlatform {
    pub fn as_engine_build_platform(
        &self,
        request_id: &str,
    ) -> Box<dyn qovery_engine::build_platform::BuildPlatform> {
        Box::new(match self.kind {
            qovery_engine::build_platform::Kind::LocalDocker => {
                LocalDocker::new(request_id, self.id.as_str(), self.name.as_str())
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CloudProvider {
    pub kind: qovery_engine::cloud_provider::Kind,
    pub id: String,
    pub name: String,
    pub options: Options,
    pub kubernetes: Kubernetes,
}

impl CloudProvider {
    pub fn as_engine_cloud_provider(
        &self,
        request_id: &str,
        organization_id: &str,
    ) -> Box<dyn qovery_engine::cloud_provider::CloudProvider> {
        match self.kind {
            qovery_engine::cloud_provider::Kind::AWS => {
                // FIXME
                Box::new(AWS::new(
                    request_id,
                    self.id.as_str(),
                    organization_id,
                    self.name.as_str(),
                    self.options.access_key_id.as_ref().unwrap().as_str(),
                    self.options.secret_access_key.as_ref().unwrap().as_str(),
                ))
            }
            qovery_engine::cloud_provider::Kind::GCP => {
                // FIXME
                Box::new(GCP::new(
                    request_id,
                    self.id.as_str(),
                    self.name.as_str(),
                    "",
                ))
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Kubernetes {
    pub kind: qovery_engine::cloud_provider::kubernetes::Kind,
    pub id: String,
    pub name: String,
    pub version: String,
    pub region: String,
    pub nodes: Vec<Node>,
}

impl Kubernetes {
    pub fn as_engine_kubernetes<'a>(
        &self,
        request_id: &str,
        cloud_provider: &'a Box<dyn qovery_engine::cloud_provider::CloudProvider>,
        nodes: &Vec<Box<dyn qovery_engine::cloud_provider::kubernetes::KubernetesNode>>,
    ) -> Box<dyn qovery_engine::cloud_provider::kubernetes::Kubernetes + 'a> {
        match self.kind {
            qovery_engine::cloud_provider::kubernetes::Kind::EKS => Box::new(EKS::new(
                request_id,
                self.id.as_str(),
                self.name.as_str(),
                self.version.as_str(),
                self.region.as_str(),
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
    pub options: Options,
}

impl ContainerRegistry {
    pub fn as_engine_container_registry<'a>(
        &'a self,
        request_id: &str,
    ) -> Box<dyn qovery_engine::container_registry::ContainerRegistry + 'a> {
        match self.kind {
            qovery_engine::container_registry::Kind::DockerHub => Box::new(DockerHub::new(
                request_id,
                self.id.as_str(),
                self.name.as_str(),
                self.options.login.as_ref().unwrap().as_str(),
                self.options.password.as_ref().unwrap().as_str(),
            )),
            qovery_engine::container_registry::Kind::ECR => Box::new(ECR::new(
                request_id,
                self.id.as_str(),
                self.name.as_str(),
                self.options.access_key_id.as_ref().unwrap().as_str(),
                self.options.secret_access_key.as_ref().unwrap().as_str(),
                self.options.region.as_ref().unwrap().as_str(),
            )),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Options {
    login: Option<String>,
    password: Option<String>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    region: Option<String>,
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
