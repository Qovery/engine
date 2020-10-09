use std::borrow::Borrow;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use qovery_engine::build_platform::local_docker::LocalDocker;
use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::gcp::GCP;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::dns_provider::Kind::CLOUDFLARE;
use qovery_engine::engine::Engine;
use qovery_engine::models::{Context, Environment, EnvironmentAction, Listener, ProgressListener};

#[derive(Serialize, Deserialize, Clone)]
pub struct Request {
    pub id: String,
    pub organization_id: String,
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub build_platform: BuildPlatform,
    pub cloud_provider: CloudProvider,
    pub dns_provider: DnsProvider,
    #[serde(default = "default_test_cluster")]
    pub test_cluster: bool,
    pub container_registry: ContainerRegistry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_environment: Option<Environment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failover_environment: Option<Environment>,
}

fn default_test_cluster() -> bool {
    false
}

impl Request {
    pub fn engine(&self, context: &Context, progress_listener: Listener) -> Engine {
        let mut build_platform = self.build_platform.to_engine_build_platform(&context);

        build_platform.add_listener(progress_listener.clone());

        let mut cloud_provider = self
            .cloud_provider
            .to_engine_cloud_provider(&context, self.organization_id.as_str());

        cloud_provider.add_listener(progress_listener.clone());

        let mut container_registry = self
            .container_registry
            .to_engine_container_registry(&context);

        container_registry.add_listener(progress_listener.clone());

        let dns_provider = self.dns_provider.to_engine_dns_provider(context);

        Engine::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
        )
    }

    pub fn environment_action(&self) -> Option<EnvironmentAction> {
        if self.target_environment.is_none() {
            return None;
        }

        let environment = self.target_environment.as_ref().unwrap().clone();

        Some(match self.failover_environment.clone() {
            Some(fe) => EnvironmentAction::EnvironmentWithFailover(environment, fe),
            None => EnvironmentAction::Environment(environment),
        })
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Create,
    Pause,
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
    pub fn to_engine_build_platform(
        &self,
        context: &Context,
    ) -> Box<dyn qovery_engine::build_platform::BuildPlatform> {
        Box::new(match self.kind {
            qovery_engine::build_platform::Kind::LocalDocker => {
                LocalDocker::new(context.clone(), self.id.as_str(), self.name.as_str())
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
    pub fn to_engine_cloud_provider(
        &self,
        context: &Context,
        organization_id: &str,
    ) -> Box<dyn qovery_engine::cloud_provider::CloudProvider> {
        match self.kind {
            qovery_engine::cloud_provider::Kind::AWS => Box::new(AWS::new(
                context.clone(),
                self.id.as_str(),
                organization_id,
                self.name.as_str(),
                self.options.access_key_id.as_ref().unwrap().as_str(),
                self.options.secret_access_key.as_ref().unwrap().as_str(),
            )),
            qovery_engine::cloud_provider::Kind::GCP => Box::new(GCP::new(
                context.clone(),
                self.id.as_str(),
                self.name.as_str(),
                "",
            )),
            qovery_engine::cloud_provider::Kind::DO => Box::new(DO::new(
                context.clone(),
                self.id.as_str(),
                self.options.secret_access_key.as_ref().unwrap().as_str(),
            )),
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
    pub test_cluster: bool,
    pub options: Value,
    pub nodes: Vec<Node>,
}

impl Kubernetes {
    pub fn to_engine_kubernetes<'a>(
        &self,
        context: &Context,
        cloud_provider: &'a dyn qovery_engine::cloud_provider::CloudProvider,
        dns_provider: &'a dyn qovery_engine::dns_provider::DnsProvider,
        nodes: &Vec<Box<dyn qovery_engine::cloud_provider::kubernetes::KubernetesNode>>,
    ) -> Box<dyn qovery_engine::cloud_provider::kubernetes::Kubernetes + 'a> {
        match self.kind {
            qovery_engine::cloud_provider::kubernetes::Kind::EKS => Box::new(EKS::new(
                context.clone(),
                self.id.as_str(),
                self.name.as_str(),
                self.version.as_str(),
                self.region.as_str(),
                cloud_provider.as_any().downcast_ref::<AWS>().unwrap(),
                dns_provider,
                self.test_cluster,
                serde_json::from_value::<qovery_engine::cloud_provider::aws::kubernetes::Options>(
                    self.options.clone(),
                )
                .expect("What's wronnnnng -- JSON Options payload is not the expected one"),
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

    pub fn to_engine_kubernetes_nodes(
        &self,
    ) -> Vec<Box<dyn qovery_engine::cloud_provider::kubernetes::KubernetesNode>> {
        self.nodes
            .iter()
            .map(|n| n.to_engine_kubernetes_node(self))
            .collect::<Vec<_>>()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Node {
    pub cpu: u8,
    pub memory_in_gib: u16,
}

impl Node {
    pub fn to_engine_kubernetes_node(
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
    pub fn to_engine_container_registry(
        &self,
        context: &Context,
    ) -> Box<dyn qovery_engine::container_registry::ContainerRegistry> {
        match self.kind {
            qovery_engine::container_registry::Kind::DockerHub => Box::new(DockerHub::new(
                context.clone(),
                self.id.as_str(),
                self.name.as_str(),
                self.options.login.as_ref().unwrap().as_str(),
                self.options.password.as_ref().unwrap().as_str(),
            )),
            qovery_engine::container_registry::Kind::ECR => Box::new(ECR::new(
                context.clone(),
                self.id.as_str(),
                self.name.as_str(),
                self.options.access_key_id.as_ref().unwrap().as_str(),
                self.options.secret_access_key.as_ref().unwrap().as_str(),
                self.options.region.as_ref().unwrap().as_str(),
            )),
            qovery_engine::container_registry::Kind::DOCR => Box::new(DOCR::new(
                context.clone(),
                self.name.as_str(),
                self.options.secret_access_key.as_ref().unwrap().as_str(),
            )),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DnsProvider {
    pub kind: qovery_engine::dns_provider::Kind,
    pub id: String,
    pub name: String,
    pub domain: String,
    pub options: Value,
}

impl DnsProvider {
    pub fn to_engine_dns_provider(
        &self,
        context: &Context,
    ) -> Box<dyn qovery_engine::dns_provider::DnsProvider> {
        match self.kind {
            qovery_engine::dns_provider::Kind::CLOUDFLARE => {
                let token = self.options.get("cloudflare_api_token");
                let email = self.options.get("cloudflare_email");

                Box::new(Cloudflare::new(
                    context.clone(),
                    self.id.clone(),
                    self.name.clone(),
                    self.domain.clone(),
                    token.unwrap().as_str().unwrap().parse().unwrap(),
                    email.unwrap().as_str().unwrap().parse().unwrap(),
                ))
            }
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

#[derive(Serialize, Deserialize, Clone)]
pub struct CheckTask {
    pub is_running: bool,
}

impl CheckTask {
    pub fn new(is_running: bool) -> Self {
        CheckTask { is_running }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}
