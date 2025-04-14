use crate::cmd::command::CommandKiller;
use crate::environment::action::DeploymentAction;
use crate::environment::models::abort::Abort;
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::io_models::models::CustomDomain;
use std::net::IpAddr;
use std::thread;
use std::time::Duration;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::error::ResolveError;
use trust_dns_resolver::lookup_ip::LookupIp;
use trust_dns_resolver::proto::rr::{RData, RecordType};
use trust_dns_resolver::{Name, Resolver};

pub struct CheckDnsForDomains<'a> {
    pub resolve_to_ip: Vec<String>,
    pub resolve_to_cname: Vec<CustomDomain>,
    pub log: Box<dyn Fn(String) + 'a + Send + Sync>,
}

const DEFAULT_CHECK_FREQUENCY: Duration = Duration::from_secs(30);

fn dns_resolvers() -> Vec<Resolver> {
    let mut resolver_options = ResolverOpts::default();

    //  We want to avoid cache and using host file of the host, as some provider force caching
    //  which lead to stale response
    resolver_options.cache_size = 0;
    resolver_options.use_hosts_file = true;
    //resolver_options.ip_strategy = LookupIpStrategy::Ipv4Only;

    vec![
        Resolver::new(ResolverConfig::google(), resolver_options).expect("Invalid google DNS resolver configuration"),
        Resolver::new(ResolverConfig::cloudflare(), resolver_options)
            .expect("Invalid cloudflare DNS resolver configuration"),
        Resolver::new(ResolverConfig::quad9(), resolver_options).expect("Invalid quad9 DNS resolver configuration"),
        Resolver::from_system_conf().expect("Invalid system DNS resolver configuration"),
    ]
}

fn await_resolve<R>(
    with_resolver: &impl Fn(&Resolver) -> Result<R, ResolveError>,
    check_frequency: Duration,
    should_abort: &CommandKiller,
) -> Result<R, ResolveError> {
    let resolvers = dns_resolvers();

    let mut ix: usize = 0;
    let mut next_resolver = || {
        let resolver = &resolvers[ix % resolvers.len()];
        ix += 1;
        resolver
    };

    loop {
        match with_resolver(next_resolver()) {
            Ok(ip) => break Ok(ip),
            Err(err) => {
                if should_abort.should_abort().is_some() {
                    break Err(err);
                }

                thread::sleep(check_frequency)
            }
        }
    }
}

fn await_domain_resolve_cname<'a>(
    domain_to_check: impl Fn() -> &'a str,
    check_frequency: Duration,
    should_abort: CommandKiller,
) -> Result<Name, ResolveError> {
    await_resolve(
        &|resolver| {
            resolver
                .lookup(domain_to_check(), RecordType::CNAME)
                .into_iter()
                .flat_map(|lookup| lookup.into_iter())
                .filter_map(|rdata| {
                    if let RData::CNAME(cname) = rdata {
                        Some(cname.0)
                    } else {
                        None
                    }
                })
                .next()
                .ok_or_else(|| ResolveError::from("no CNAME record available for this domain"))
        },
        check_frequency,
        &should_abort,
    )
}

fn await_domain_resolve_ip<'a>(
    domain_to_check: impl Fn() -> &'a str,
    check_frequency: Duration,
    should_abort: CommandKiller,
) -> Result<LookupIp, ResolveError> {
    await_resolve(
        &|resolver| resolver.lookup_ip(domain_to_check()),
        check_frequency,
        &should_abort,
    )
}

fn check_domain_resolve_ip(domain: &str, log: &impl Fn(String), abort: &dyn Abort) {
    // We use send_success because if on_check is called it means the DB is already correctly deployed
    (log)(format!(
        "🌍 Checking DNS Ip resolution for domain {domain}. Please wait, it can take some time..."
    ));

    let get_domain = || {
        (log)(format!("🌍 Waiting domain {domain} resolve to an Ip address..."));
        domain
    };

    let should_abort = CommandKiller::from(Duration::from_secs(60 * 5), abort);
    let does_resolve = await_domain_resolve_ip(get_domain, DEFAULT_CHECK_FREQUENCY, should_abort);

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

fn check_domain_resolve_cname(custom_domain: &CustomDomain, log: &impl Fn(String), abort_status: &dyn Abort) {
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

    let should_abort = CommandKiller::from(Duration::from_secs(60 * 5), abort_status);
    let does_resolve = await_domain_resolve_cname(get_domain, DEFAULT_CHECK_FREQUENCY, should_abort);

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

impl DeploymentAction for CheckDnsForDomains<'_> {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        for domain in &self.resolve_to_ip {
            check_domain_resolve_ip(domain, &self.log, target.abort);
        }

        for domain in &self.resolve_to_cname {
            check_domain_resolve_cname(domain, &self.log, target.abort);
        }

        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_cname_resolution() {
        let cname = await_domain_resolve_cname(
            || "ci-test-no-delete.qovery.io",
            Duration::from_secs(10),
            CommandKiller::from_timeout(Duration::from_secs(30)),
        );

        assert_eq!(cname.unwrap().to_utf8(), String::from("qovery.io."));
    }
}
