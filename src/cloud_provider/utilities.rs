#![allow(clippy::field_reassign_with_default)]

use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::io_models::{Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope};
use crate::logger::Logger;
use chrono::Duration;
use core::option::Option::{None, Some};
use core::result::Result;
use core::result::Result::{Err, Ok};
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use std::fmt;
use std::net::ToSocketAddrs;
use std::net::{SocketAddr, TcpStream as NetTcpStream};
use trust_dns_resolver::config::*;
use trust_dns_resolver::proto::rr::{RData, RecordType};
use trust_dns_resolver::Resolver;

fn dns_resolvers() -> Vec<Resolver> {
    let mut resolver_options = ResolverOpts::default();

    //  We want to avoid cache and using host file of the host, as some provider force caching
    //  which lead to stale response
    resolver_options.cache_size = 0;
    resolver_options.use_hosts_file = true;
    //resolver_options.ip_strategy = LookupIpStrategy::Ipv4Only;
    //let dns = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 254));
    //let resolver = ResolverConfig::from_parts(
    //    None,
    //    vec![],
    //    NameServerConfigGroup::from_ips_clear(&vec![dns], 53, true),
    //);

    //Resolver::new(resolver, resolver_options).unwrap()
    vec![
        Resolver::new(ResolverConfig::google(), resolver_options).expect("Invalid google DNS resolver configuration"),
        Resolver::new(ResolverConfig::cloudflare(), resolver_options)
            .expect("Invalid cloudflare DNS resolver configuration"),
        Resolver::new(ResolverConfig::quad9(), resolver_options).expect("Invalid quad9 DNS resolver configuration"),
        Resolver::from_system_conf().expect("Invalid system DNS resolver configuration"),
    ]
}

fn get_cname_record_value(resolver: &Resolver, cname: &str) -> Option<String> {
    resolver
        .lookup(cname, RecordType::CNAME)
        .iter()
        .flat_map(|lookup| lookup.record_iter())
        .filter_map(|record| {
            if let RData::CNAME(cname) = record.rdata() {
                Some(cname.to_utf8())
            } else {
                None
            }
        })
        .next() // Can only have one domain behind a CNAME
}

pub fn check_cname_for(
    scope: ProgressScope,
    listeners: &Listeners,
    cname_to_check: &str,
    execution_id: &str,
) -> Result<String, String> {
    let resolvers = dns_resolvers();
    let listener_helper = ListenersHelper::new(listeners);

    let send_deployment_progress = |msg: &str| {
        listener_helper.deployment_in_progress(ProgressInfo::new(
            scope.clone(),
            ProgressLevel::Info,
            Some(msg.to_string()),
            execution_id,
        ));
    };

    let send_deployment_progress_warn = |msg: &str| {
        listener_helper.deployment_in_progress(ProgressInfo::new(
            scope.clone(),
            ProgressLevel::Warn,
            Some(msg.to_string()),
            execution_id,
        ));
    };

    send_deployment_progress(
        format!(
            "Checking CNAME resolution of '{}'. Please wait, it can take some time...",
            cname_to_check
        )
        .as_str(),
    );

    // Trying for 5 min to resolve CNAME
    let mut ix: usize = 0;
    let mut next_resolver = || {
        let resolver = &resolvers[ix % resolvers.len()];
        ix += 1;
        resolver
    };
    let fixed_iterable = Fixed::from_millis(Duration::seconds(5).num_milliseconds() as u64).take(6 * 5);
    let check_result = retry::retry(fixed_iterable, || {
        match get_cname_record_value(next_resolver(), cname_to_check) {
            Some(domain) => OperationResult::Ok(domain),
            None => {
                let msg = format!("Cannot find domain under CNAME {}. Retrying in 5 seconds...", cname_to_check);
                send_deployment_progress(msg.as_str());
                OperationResult::Retry(msg)
            }
        }
    });

    match check_result {
        Ok(domain) => {
            send_deployment_progress(format!("Resolution of CNAME {} found to {}", cname_to_check, domain).as_str());
        }
        Err(_) => {
            let msg = format!(
                "Resolution of CNAME {} failed. Please check that you have correctly configured your CNAME. If you are using a CDN you can forget this message",
                cname_to_check
            );
            send_deployment_progress_warn(msg.as_str());
        }
    }

    // do not exit / rollback if domain is not ready, simply warn the user about it
    Ok(cname_to_check.to_string())
}

pub fn check_domain_for(
    listener_helper: ListenersHelper,
    domains_to_check: Vec<&str>,
    execution_id: &str,
    context_id: &str,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), EngineError> {
    let resolvers = dns_resolvers();

    for domain in domains_to_check {
        let message = format!(
            "Let's check domain resolution for '{}'. Please wait, it can take some time...",
            domain
        );

        listener_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Environment {
                id: execution_id.to_string(),
            },
            ProgressLevel::Info,
            Some(message.to_string()),
            execution_id,
        ));

        let mut ix: usize = 0;
        let mut next_resolver = || {
            let resolver = &resolvers[ix % resolvers.len()];
            ix += 1;
            resolver
        };

        logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(message.to_string()),
        ));

        let fixed_iterable = Fixed::from_millis(3000).take(100);
        let check_result = retry::retry(fixed_iterable, || match next_resolver().lookup_ip(domain) {
            Ok(lookup_ip) => OperationResult::Ok(lookup_ip),
            Err(err) => {
                let x = format!("Domain resolution check for '{}' is still in progress...", domain);

                logger.log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(x.to_string()),
                ));

                listener_helper.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Environment {
                        id: execution_id.to_string(),
                    },
                    ProgressLevel::Info,
                    Some(x),
                    execution_id.to_string(),
                ));

                OperationResult::Retry(err)
            }
        });

        match check_result {
            Ok(_) => {
                let x = format!("Domain {} is ready! ⚡️", domain);

                logger.log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(message.to_string()),
                ));

                listener_helper.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Environment {
                        id: execution_id.to_string(),
                    },
                    ProgressLevel::Info,
                    Some(x),
                    context_id,
                ));
            }
            Err(_) => {
                let message = format!(
                    "Unable to check domain availability for '{}'. It can be due to a \
                        too long domain propagation. Note: this is not critical.",
                    domain
                );

                logger.log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_safe(message.to_string()),
                ));

                listener_helper.deployment_in_progress(ProgressInfo::new(
                    ProgressScope::Environment {
                        id: execution_id.to_string(),
                    },
                    ProgressLevel::Warn,
                    Some(message),
                    context_id,
                ));
            }
        }
    }

    Ok(())
}

pub fn sanitize_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name).replace('_', "-")
}

pub fn managed_db_name_sanitizer(max_size: usize, prefix: &str, name: &str) -> String {
    let max_size = max_size - prefix.len();
    let mut new_name = format!("{}{}", prefix, name.replace('_', "").replace('-', ""));
    if new_name.chars().count() > max_size {
        new_name = new_name[..max_size].to_string();
    }
    new_name
}

#[derive(PartialEq, Debug)]
pub enum TcpCheckErrors {
    DomainNotResolvable,
    PortNotOpen,
    UnknownError,
}

pub enum TcpCheckSource<'a> {
    SocketAddr(SocketAddr),
    DnsName(&'a str),
}

impl fmt::Display for TcpCheckSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TcpCheckSource::SocketAddr(x) => write!(f, "{}", x),
            TcpCheckSource::DnsName(x) => write!(f, "{}", x),
        }
    }
}

pub fn check_tcp_port_is_open(address: &TcpCheckSource, port: u16) -> Result<(), TcpCheckErrors> {
    let timeout = core::time::Duration::from_secs(1);

    let ip = match address {
        TcpCheckSource::SocketAddr(x) => *x,
        TcpCheckSource::DnsName(x) => {
            let address = format!("{}:{}", x, port);
            match address.to_socket_addrs() {
                Ok(x) => {
                    let ips: Vec<SocketAddr> = x.collect();
                    ips[0]
                }
                Err(_) => return Err(TcpCheckErrors::DomainNotResolvable),
            }
        }
    };

    match NetTcpStream::connect_timeout(&ip, timeout) {
        Ok(_) => Ok(()),
        Err(_) => Err(TcpCheckErrors::PortNotOpen),
    }
}

pub fn wait_until_port_is_open(
    address: &TcpCheckSource,
    port: u16,
    max_timeout: usize,
    logger: &dyn Logger,
    event_details: EventDetails,
) -> Result<(), TcpCheckErrors> {
    let fixed_iterable = Fixed::from(core::time::Duration::from_secs(1)).take(max_timeout);
    let check_result = retry::retry(fixed_iterable, || match check_tcp_port_is_open(address, port) {
        Ok(_) => OperationResult::Ok(()),
        Err(e) => {
            logger.log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!("{}:{} is still not ready: {:?}. retrying...", address, port, e)),
            ));
            OperationResult::Retry(e)
        }
    });

    match check_result {
        Ok(_) => Ok(()),
        Err(e) => match e {
            Error::Operation { error, .. } => Err(error),
            Error::Internal(_) => Err(TcpCheckErrors::UnknownError),
        },
    }
}

pub fn print_action(
    cloud_provider_name: &str,
    struct_name: &str,
    fn_name: &str,
    item_name: &str,
    event_details: EventDetails,
    logger: &dyn Logger,
) {
    let msg = format!("{}.{}.{} called for {}", cloud_provider_name, struct_name, fn_name, item_name);
    match fn_name.contains("error") {
        true => logger.log(EngineEvent::Warning(event_details, EventMessage::new_from_safe(msg))),
        false => logger.log(EngineEvent::Info(event_details, EventMessage::new_from_safe(msg))),
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::utilities::{
        check_tcp_port_is_open, dns_resolvers, get_cname_record_value, TcpCheckErrors, TcpCheckSource,
    };
    use crate::errors::CommandError;
    use crate::models::types::VersionsNumber;
    use std::str::FromStr;

    #[test]
    pub fn test_port_open() {
        let address_ok = "www.qovery.com";
        let port_ok: u16 = 443;
        let address_nok = "www.abcdefghijklmnopqrstuvwxyz.com";
        let port_nok: u16 = 4430;

        assert!(check_tcp_port_is_open(&TcpCheckSource::DnsName(address_ok), port_ok).is_ok());
        assert_eq!(
            check_tcp_port_is_open(&TcpCheckSource::DnsName(address_nok), port_ok).unwrap_err(),
            TcpCheckErrors::DomainNotResolvable
        );
        assert_eq!(
            check_tcp_port_is_open(&TcpCheckSource::DnsName(address_ok), port_nok).unwrap_err(),
            TcpCheckErrors::PortNotOpen
        );
    }

    #[test]
    pub fn test_cname_resolution() {
        let resolvers = dns_resolvers();
        let cname = get_cname_record_value(&resolvers[0], "ci-test-no-delete.qovery.io");

        assert_eq!(cname, Some(String::from("qovery.io.")));
    }

    #[test]
    pub fn test_versions_number() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            expected_output: Result<VersionsNumber, CommandError>,
            description: &'a str,
        }

        let test_cases = vec![
            TestCase {
                input: "",
                expected_output: Err(CommandError::new_from_safe_message("version cannot be empty".to_string())),
                description: "empty version str",
            },
            TestCase {
                input: "    ",
                expected_output: Err(CommandError::new_from_safe_message("version cannot be empty".to_string())),
                description: "version a tab str",
            },
            TestCase {
                input: " ",
                expected_output: Err(CommandError::new_from_safe_message("version cannot be empty".to_string())),
                description: "version as a space str",
            },
            TestCase {
                input: "-", // TODO(benjaminch): better handle this case, should trigger an error
                expected_output: Ok(VersionsNumber::new("-".to_string(), None, None, None)),
                description: "suffix separator only",
            },
            TestCase {
                input: "test",
                expected_output: Ok(VersionsNumber::new("test".to_string(), None, None, None)),
                description: "bad string",
            },
            TestCase {
                input: "1,2,3,4", // TODO(benjaminch): better handle this case, should trigger an error
                expected_output: Ok(VersionsNumber::new("1,2,3,4".to_string(), None, None, None)),
                description: "bad versions separator",
            },
            TestCase {
                input: "1",
                expected_output: Ok(VersionsNumber::new("1".to_string(), None, None, None)),
                description: "major only",
            },
            TestCase {
                input: "1.1",
                expected_output: Ok(VersionsNumber::new("1".to_string(), Some("1".to_string()), None, None)),
                description: "major.minor only",
            },
            TestCase {
                input: "1.1.1",
                expected_output: Ok(VersionsNumber::new(
                    "1".to_string(),
                    Some("1".to_string()),
                    Some("1".to_string()),
                    None,
                )),
                description: "major.minor.update only",
            },
            TestCase {
                input: "1.1.1.suffix",
                expected_output: Ok(VersionsNumber::new(
                    "1".to_string(),
                    Some("1".to_string()),
                    Some("1".to_string()),
                    Some("suffix".to_string()),
                )),
                description: "major.minor.patch-suffix",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = VersionsNumber::from_str(tc.input);

            // verify:
            assert_eq!(tc.expected_output, result, "case {} : '{}'", tc.description, tc.input);
        }
    }
}
