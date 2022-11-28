use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::models;
use crate::models::aws::AwsRouterExtraSettings;
use crate::models::aws_ec2::AwsEc2RouterExtraSettings;
use crate::models::router::{RouterAdvancedSettings, RouterError, RouterService};
use crate::models::scaleway::ScwRouterExtraSettings;
use crate::models::types::{AWSEc2, AWS, SCW};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Router {
    pub long_id: Uuid,
    pub name: String,
    pub action: Action,
    pub default_domain: String,
    pub public_port: u16,
    #[serde(default)]
    /// sticky_sessions_enabled: enables sticky session for the request to come to the same
    /// pod replica that was responding to the request before
    pub sticky_sessions_enabled: bool,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
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
        custom_domain_check_enabled: bool,
        whitelist_source_range: String,
        cloud_provider: &dyn CloudProvider,
    ) -> Result<Box<dyn RouterService>, RouterError> {
        let custom_domains = self
            .custom_domains
            .iter()
            .map(|x| crate::cloud_provider::models::CustomDomain {
                domain: x.domain.clone(),
                target_domain: x.target_domain.clone(),
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

        let advanced_settings = RouterAdvancedSettings {
            custom_domain_check_enabled,
            whitelist_source_range,
        };

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
                        self.action.to_service_action(),
                        self.default_domain.as_str(),
                        custom_domains,
                        routes,
                        self.sticky_sessions_enabled,
                        AwsRouterExtraSettings {},
                        advanced_settings,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                } else {
                    Ok(Box::new(models::router::Router::<AWSEc2>::new(
                        context,
                        self.long_id,
                        self.name.as_str(),
                        self.action.to_service_action(),
                        self.default_domain.as_str(),
                        custom_domains,
                        routes,
                        self.sticky_sessions_enabled,
                        AwsEc2RouterExtraSettings {},
                        advanced_settings,
                        |transmitter| context.get_event_details(transmitter),
                    )?))
                }
            }
            CPKind::Scw => {
                let router = Box::new(models::router::Router::<SCW>::new(
                    context,
                    self.long_id,
                    self.name.as_str(),
                    self.action.to_service_action(),
                    self.default_domain.as_str(),
                    custom_domains,
                    routes,
                    self.sticky_sessions_enabled,
                    ScwRouterExtraSettings {},
                    advanced_settings,
                    |transmitter| context.get_event_details(transmitter),
                )?);
                Ok(router)
            }
        }
    }
}
