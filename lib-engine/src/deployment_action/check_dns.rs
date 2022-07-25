use crate::cloud_provider::models::CustomDomain;
use crate::cloud_provider::utilities::{await_domain_resolve_cname, await_domain_resolve_ip};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use std::net::IpAddr;
use std::time::Duration;

pub struct CheckDnsForDomains {
    pub resolve_to_ip: Vec<String>,
    pub resolve_to_cname: Vec<CustomDomain>,
    pub log: Box<dyn Fn(String)>,
}

fn check_domain_resolve_ip(domain: &str, log: &impl Fn(String)) {
    // We use send_success because if on_check is called it means the DB is already correctly deployed
    (log)(format!(
        "🌍 Checking DNS Ip resolution for domain {}. Please wait, it can take some time...",
        domain
    ));

    let get_domain = || {
        (log)(format!("🌍 Waiting domain {} resolve to an Ip address...", domain));
        domain
    };

    let does_resolve = await_domain_resolve_ip(get_domain, Duration::from_secs(60), Duration::from_secs(60 * 5));

    match does_resolve {
        Ok(ip) => {
            (log)(format!(
                "✨ Domain {} resolved to ip {}",
                domain,
                ip.iter().next().unwrap_or_else(|| IpAddr::from([0_u8, 0, 0, 0]))
            ));
        }
        Err(_) => {
            let message = format!(
                "💥 Unable to check domain availability for '{}'. It can be due to a \
                        too long domain propagation. Note: this is not critical.",
                &domain
            );
            (log)(message);
        }
    }
}

fn check_domain_resolve_cname(custom_domain: &CustomDomain, log: &impl Fn(String)) {
    // We use send_success because if on_check is called it means the DB is already correctly deployed
    (log)(format!(
        "🌍 Checking DNS CNAME resolution for domain {}. Please wait, it can take some time...",
        &custom_domain.domain,
    ));

    let get_domain = || {
        (log)(format!(
            "🌍 Waiting domain {} to resolve to DNS CNAME {}",
            &custom_domain.domain, &custom_domain.target_domain
        ));
        custom_domain.domain.as_str()
    };

    let does_resolve = await_domain_resolve_cname(get_domain, Duration::from_secs(60), Duration::from_secs(60 * 5));

    match does_resolve {
        Ok(cname) => {
            (log)(format!(
                "✨ Domain {} resolved to CNAME {}",
                custom_domain.domain,
                cname.to_utf8()
            ));
        }
        Err(_) => {
            let message = format!(
                "💥 Resolution of CNAME for domain {} failed. Please check that you have correctly configured your CNAME. If you are using a CDN you can forget this message",
                &custom_domain.domain
            );
            (log)(message);
        }
    }
}

impl DeploymentAction for CheckDnsForDomains {
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        for domain in &self.resolve_to_ip {
            check_domain_resolve_ip(domain, &self.log);
        }

        for domain in &self.resolve_to_cname {
            check_domain_resolve_cname(domain, &self.log);
        }

        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }
}
