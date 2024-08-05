use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::Action;
use crate::models;
use crate::models::aws::AwsRouterExtraSettings;
use crate::models::aws_ec2::AwsEc2RouterExtraSettings;
use crate::models::gcp::GcpRouterExtraSettings;
use crate::models::router::{RouterAdvancedSettings, RouterError, RouterService};
use crate::models::scaleway::ScwRouterExtraSettings;
use crate::models::selfmanaged::OnPremiseRouterExtraSettings;
use crate::models::types::{AWSEc2, OnPremise, AWS, GCP, SCW};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn default_generate_certificate() -> bool {
    true
}

fn default_use_cdn() -> bool {
    false
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Router {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub default_domain: String,
    pub public_port: u16,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
    #[serde(default = "default_generate_certificate")]
    pub generate_certificate: bool,
    #[serde(default = "default_use_cdn")]
    pub use_cdn: bool,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Route {
    pub path: String,
    pub service_long_id: Uuid,
}

impl Router {
    pub fn to_router_domain(
        &self,
        context: &Context,
        advanced_settings: RouterAdvancedSettings,
        cloud_provider: &dyn CloudProvider,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
    ) -> Result<Box<dyn RouterService>, RouterError> {
        let custom_domains = self
            .custom_domains
            .iter()
            .map(|it| crate::cloud_provider::models::CustomDomain {
                domain: it.domain.clone(),
                target_domain: it.target_domain.clone(),
                generate_certificate: it.generate_certificate,
                use_cdn: it.use_cdn,
            })
            .collect::<Vec<_>>();

        let routes = self
            .routes
            .iter()
            .map(|x| crate::cloud_provider::models::Route {
                path: x.path.clone(),
                service_long_id: x.service_long_id,
            })
            .collect::<Vec<_>>();

        match cloud_provider.kind() {
            CPKind::Aws => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::router::Router::<AWS>::new(
                        context,
                        self.long_id,
                        self.name.as_str(),
                        self.kube_name.to_string(),
                        self.action.to_service_action(),
                        self.default_domain.as_str(),
                        custom_domains,
                        routes,
                        AwsRouterExtraSettings {},
                        advanced_settings,
                        |transmitter| context.get_event_details(transmitter),
                        annotations_groups,
                        labels_groups,
                    )?))
                } else {
                    Ok(Box::new(models::router::Router::<AWSEc2>::new(
                        context,
                        self.long_id,
                        self.name.as_str(),
                        self.kube_name.to_string(),
                        self.action.to_service_action(),
                        self.default_domain.as_str(),
                        custom_domains,
                        routes,
                        AwsEc2RouterExtraSettings {},
                        advanced_settings,
                        |transmitter| context.get_event_details(transmitter),
                        annotations_groups,
                        labels_groups,
                    )?))
                }
            }
            CPKind::Scw => {
                let router = Box::new(models::router::Router::<SCW>::new(
                    context,
                    self.long_id,
                    self.name.as_str(),
                    self.kube_name.to_string(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    ScwRouterExtraSettings {},
                    advanced_settings,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    labels_groups,
                )?);
                Ok(router)
            }
            CPKind::Gcp => Ok(Box::new(models::router::Router::<GCP>::new(
                context,
                self.long_id,
                self.name.as_str(),
                self.kube_name.to_string(),
                self.action.to_service_action(),
                self.default_domain.as_str(),
                custom_domains,
                routes,
                GcpRouterExtraSettings {},
                advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
            )?)),
            CPKind::OnPremise => {
                let router = Box::new(models::router::Router::<OnPremise>::new(
                    context,
                    self.long_id,
                    self.name.as_str(),
                    self.kube_name.to_string(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    OnPremiseRouterExtraSettings {},
                    advanced_settings,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    labels_groups,
                )?);
                Ok(router)
            }
        }
    }
}
