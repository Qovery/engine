use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::router::Router;
use crate::models::types::{ToTeraContext, DO};
use tera::Context as TeraContext;

impl ToTeraContext for Router<DO> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let mut context = self.default_tera_context(target)?;
        context.insert("doks_cluster_id", target.kubernetes.id());
        if let Some(domain) = self.custom_domains.first() {
            // https://github.com/digitalocean/digitalocean-cloud-controller-manager/issues/291
            // Can only manage 1 host at a time on an DO load balancer
            context.insert("custom_domain_name", domain.domain.as_str());
        }

        Ok(context)
    }
}
