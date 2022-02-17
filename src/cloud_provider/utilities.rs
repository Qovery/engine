use std::collections::HashMap;

use crate::cloud_provider::models::CpuLimits;
use crate::error::{EngineError, StringError};
use crate::models::{Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope};
use chrono::Duration;
use core::option::Option::{None, Some};
use core::result::Result;
use core::result::Result::{Err, Ok};
use retry::delay::Fixed;
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::num::ParseFloatError;
use std::str::FromStr;
use trust_dns_resolver::config::*;
use trust_dns_resolver::proto::rr::{RData, RecordType};
use trust_dns_resolver::Resolver;

pub fn get_self_hosted_postgres_version(requested_version: String) -> Result<String, StringError> {
    let mut supported_postgres_versions = HashMap::new();

    // https://hub.docker.com/r/bitnami/postgresql/tags?page=1&ordering=last_updated

    // v10
    let v10 = generate_supported_version(10, 1, 16, Some(0), Some(0), None);
    supported_postgres_versions.extend(v10);

    // v11
    let v11 = generate_supported_version(11, 1, 11, Some(0), Some(0), None);
    supported_postgres_versions.extend(v11);

    // v12
    let v12 = generate_supported_version(12, 2, 8, Some(0), Some(0), None);
    supported_postgres_versions.extend(v12);

    // v13
    let v13 = generate_supported_version(13, 1, 4, Some(0), Some(0), None);
    supported_postgres_versions.extend(v13);

    get_supported_version_to_use("Postgresql", supported_postgres_versions, requested_version)
}

pub fn get_self_hosted_mysql_version(requested_version: String) -> Result<String, StringError> {
    let mut supported_mysql_versions = HashMap::new();
    // https://hub.docker.com/r/bitnami/mysql/tags?page=1&ordering=last_updated

    // v5.7
    let v57 = generate_supported_version(5, 7, 7, Some(16), Some(34), None);
    supported_mysql_versions.extend(v57);

    // v8
    let v8 = generate_supported_version(8, 0, 0, Some(11), Some(24), None);
    supported_mysql_versions.extend(v8);

    get_supported_version_to_use("MySQL", supported_mysql_versions, requested_version)
}

pub fn get_self_hosted_mongodb_version(requested_version: String) -> Result<String, StringError> {
    let mut supported_mongodb_versions = HashMap::new();

    // https://hub.docker.com/r/bitnami/mongodb/tags?page=1&ordering=last_updated

    // v3.6
    let mongo_version = generate_supported_version(3, 6, 6, Some(0), Some(22), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.0
    let mongo_version = generate_supported_version(4, 0, 0, Some(0), Some(23), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.2
    let mongo_version = generate_supported_version(4, 2, 2, Some(0), Some(12), None);
    supported_mongodb_versions.extend(mongo_version);

    // v4.4
    let mongo_version = generate_supported_version(4, 4, 4, Some(0), Some(4), None);
    supported_mongodb_versions.extend(mongo_version);

    get_supported_version_to_use("MongoDB", supported_mongodb_versions, requested_version)
}

pub fn get_self_hosted_redis_version(requested_version: String) -> Result<String, StringError> {
    let mut supported_redis_versions = HashMap::with_capacity(4);
    // https://hub.docker.com/r/bitnami/redis/tags?page=1&ordering=last_updated

    supported_redis_versions.insert("6".to_string(), "6.0.9".to_string());
    supported_redis_versions.insert("6.0".to_string(), "6.0.9".to_string());
    supported_redis_versions.insert("5".to_string(), "5.0.10".to_string());
    supported_redis_versions.insert("5.0".to_string(), "5.0.10".to_string());

    get_supported_version_to_use("Redis", supported_redis_versions, requested_version)
}

pub fn get_supported_version_to_use(
    database_name: &str,
    all_supported_versions: HashMap<String, String>,
    version_to_check: String,
) -> Result<String, StringError> {
    let version = match VersionsNumber::from_str(version_to_check.as_str()) {
        Ok(version) => version,
        Err(e) => return Err(e),
    };

    // if a patch version is required
    if version.patch.is_some() {
        return match all_supported_versions.get(&format!(
            "{}.{}.{}",
            version.major,
            version.minor.unwrap(),
            version.patch.unwrap()
        )) {
            Some(version) => Ok(version.to_string()),
            None => {
                return Err(format!(
                    "{} {} version is not supported",
                    database_name, version_to_check
                ));
            }
        };
    }

    // if a minor version is required
    if version.minor.is_some() {
        return match all_supported_versions.get(&format!("{}.{}", version.major, version.minor.unwrap())) {
            Some(version) => Ok(version.to_string()),
            None => {
                return Err(format!(
                    "{} {} version is not supported",
                    database_name, version_to_check
                ));
            }
        };
    };

    // if only a major version is required
    match all_supported_versions.get(&version.major) {
        Some(version) => Ok(version.to_string()),
        None => {
            return Err(format!(
                "{} {} version is not supported",
                database_name, version_to_check
            ));
        }
    }
}

// Ease the support of multiple versions by range
pub fn generate_supported_version(
    major: i32,
    minor_min: i32,
    minor_max: i32,
    update_min: Option<i32>,
    update_max: Option<i32>,
    suffix_version: Option<String>,
) -> HashMap<String, String> {
    let mut supported_versions = HashMap::new();
    let latest_major_version;

    // blank suffix if not requested
    let suffix = match suffix_version {
        Some(suffix) => suffix,
        None => "".to_string(),
    };

    let _ = match update_min {
        // manage minor with updates
        Some(_) => {
            latest_major_version = format!("{}.{}.{}{}", major, minor_max, update_max.unwrap(), suffix);

            if minor_min == minor_max {
                // add short minor format targeting latest version
                supported_versions.insert(
                    format!("{}.{}", major.to_string(), minor_max.to_string()),
                    latest_major_version.clone(),
                );
                if update_min.unwrap() == update_max.unwrap() {
                    let version = format!("{}.{}.{}", major, minor_min, update_min.unwrap());
                    supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                } else {
                    for update in update_min.unwrap()..update_max.unwrap() + 1 {
                        let version = format!("{}.{}.{}", major, minor_min, update);
                        supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                    }
                }
            } else {
                for minor in minor_min..minor_max + 1 {
                    // add short minor format targeting latest version
                    supported_versions.insert(
                        format!("{}.{}", major.to_string(), minor.to_string()),
                        format!(
                            "{}.{}.{}",
                            major.to_string(),
                            minor.to_string(),
                            update_max.unwrap().to_string()
                        ),
                    );
                    if update_min.unwrap() == update_max.unwrap() {
                        let version = format!("{}.{}.{}", major, minor, update_min.unwrap());
                        supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                    } else {
                        for update in update_min.unwrap()..update_max.unwrap() + 1 {
                            let version = format!("{}.{}.{}", major, minor, update);
                            supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
                        }
                    }
                }
            }
        }
        // manage minor without updates
        None => {
            latest_major_version = format!("{}.{}{}", major, minor_max, suffix);
            for minor in minor_min..minor_max + 1 {
                let version = format!("{}.{}", major, minor);
                supported_versions.insert(version.clone(), format!("{}{}", version, suffix));
            }
        }
    };

    // default major + major.minor supported version
    supported_versions.insert(major.to_string(), latest_major_version);

    supported_versions
}

// unfortunately some proposed versions are not SemVer like Elasticache (6.x)
// this is why we need ot have our own structure
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct VersionsNumber {
    pub(crate) major: String,
    pub(crate) minor: Option<String>,
    pub(crate) patch: Option<String>,
    pub(crate) suffix: Option<String>,
}

impl VersionsNumber {
    pub fn new(major: String, minor: Option<String>, patch: Option<String>, suffix: Option<String>) -> Self {
        VersionsNumber {
            major,
            minor,
            patch,
            suffix,
        }
    }

    pub fn to_string(&self) -> String {
        let mut version = vec![self.major.to_string()];

        if self.minor.is_some() {
            version.push(self.minor.clone().unwrap())
        }
        if self.patch.is_some() {
            version.push(self.patch.clone().unwrap())
        }
        if self.suffix.is_some() {
            version.push(self.suffix.clone().unwrap())
        }

        version.join(".")
    }

    pub fn to_major_version_string(&self) -> String {
        self.major.clone()
    }

    pub fn to_major_minor_version_string(&self, default_minor: &str) -> String {
        let test = format!(
            "{}.{}",
            self.major.clone(),
            self.minor.as_ref().unwrap_or(&default_minor.to_string())
        );

        test
    }
}

impl FromStr for VersionsNumber {
    type Err = StringError;

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        if version.trim() == "" {
            return Err(StringError::from("version cannot be empty"));
        }

        let mut version_split = version.splitn(4, '.').map(|v| v.trim());

        let major = match version_split.next() {
            Some(major) => {
                let major = major.to_string();
                major.replace("v", "")
            }
            None => {
                return Err(format!(
                    "please check the version you've sent ({}), it can't be checked",
                    version
                ))
            }
        };

        let minor = version_split.next().map(|minor| {
            let minor = minor.to_string();
            minor.replace("+", "")
        });

        let patch = version_split.next().map(|patch| patch.to_string());

        let suffix = version_split.next().map(|suffix| suffix.to_string());

        // TODO(benjaminch): Handle properly the case where versions are empty
        // eq. 1..2

        Ok(VersionsNumber::new(major, minor, patch, suffix))
    }
}

impl fmt::Display for VersionsNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

fn cloudflare_dns_resolver() -> Resolver {
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
    Resolver::new(ResolverConfig::cloudflare(), resolver_options)
        .expect("Invalid cloudflare DNS resolver configuration")
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
    let resolver = cloudflare_dns_resolver();
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
    let fixed_iterable = Fixed::from_millis(Duration::seconds(5).num_milliseconds() as u64).take(6 * 5);
    let check_result = retry::retry(fixed_iterable, || {
        match get_cname_record_value(&resolver, cname_to_check) {
            Some(domain) => OperationResult::Ok(domain),
            None => {
                let msg = format!(
                    "Cannot find domain under CNAME {}. Retrying in 5 seconds...",
                    cname_to_check
                );
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
) -> Result<(), EngineError> {
    let resolver = cloudflare_dns_resolver();

    for domain in domains_to_check {
        listener_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Environment {
                id: execution_id.to_string(),
            },
            ProgressLevel::Info,
            Some(format!(
                "Let's check domain resolution for '{}'. Please wait, it can take some time...",
                domain
            )),
            execution_id,
        ));

        let fixed_iterable = Fixed::from_millis(3000).take(100);
        let check_result = retry::retry(fixed_iterable, || match resolver.lookup_ip(domain) {
            Ok(lookup_ip) => OperationResult::Ok(lookup_ip),
            Err(err) => {
                let x = format!("Domain resolution check for '{}' is still in progress...", domain);

                info!("{}", x);

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

                info!("{}", x);

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

                warn!("{}", message);

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
    format!("{}-{}", prefix, name).replace("_", "-")
}

pub fn managed_db_name_sanitizer(max_size: usize, prefix: &str, name: &str) -> String {
    let max_size = max_size - prefix.len();
    let mut new_name = format!("{}{}", prefix, name.replace("_", "").replace("-", ""));
    if new_name.chars().count() > max_size {
        new_name = new_name[..max_size].to_string();
    }
    new_name
}

pub fn convert_k8s_cpu_value_to_f32(value: String) -> Result<f32, ParseFloatError> {
    if value.ends_with('m') {
        let mut value_number_string = value;
        value_number_string.pop();
        return match value_number_string.parse::<f32>() {
            Ok(n) => {
                Ok(n * 0.001) // return in milli cpu the value
            }
            Err(e) => Err(e),
        };
    }

    match value.parse::<f32>() {
        Ok(n) => Ok(n),
        Err(e) => Err(e),
    }
}

pub fn validate_k8s_required_cpu_and_burstable(
    listener_helper: &ListenersHelper,
    execution_id: &str,
    context_id: &str,
    total_cpu: String,
    cpu_burst: String,
) -> Result<CpuLimits, ParseFloatError> {
    let total_cpu_float = convert_k8s_cpu_value_to_f32(total_cpu.clone())?;
    let cpu_burst_float = convert_k8s_cpu_value_to_f32(cpu_burst.clone())?;
    let mut set_cpu_burst = cpu_burst.clone();

    if cpu_burst_float < total_cpu_float {
        let message = format!(
            "CPU burst value '{}' was lower than the desired total of CPUs {}, using burstable value. Please ensure your configuration is valid",
            cpu_burst,
            total_cpu,
        );

        warn!("{}", message);

        listener_helper.error(ProgressInfo::new(
            ProgressScope::Environment {
                id: execution_id.to_string(),
            },
            ProgressLevel::Warn,
            Some(message),
            context_id,
        ));
        set_cpu_burst = total_cpu.clone();
    }

    Ok(CpuLimits {
        cpu_limit: set_cpu_burst,
        cpu_request: total_cpu,
    })
}

pub fn print_action(cloud_provider_name: &str, struct_name: &str, fn_name: &str, item_name: &str) {
    let msg = format!(
        "{}.{}.{} called for {}",
        cloud_provider_name, struct_name, fn_name, item_name
    );
    match fn_name.contains("error") {
        true => warn!("{}", msg),
        false => info!("{}", msg),
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::models::CpuLimits;
    use crate::cloud_provider::utilities::{
        cloudflare_dns_resolver, convert_k8s_cpu_value_to_f32, get_cname_record_value,
        validate_k8s_required_cpu_and_burstable, VersionsNumber,
    };
    use crate::error::StringError;
    use crate::models::ListenersHelper;
    use std::str::FromStr;

    #[test]
    pub fn test_k8s_milli_cpu_convert() {
        let milli_cpu = "250m".to_string();
        let int_cpu = "2".to_string();

        assert_eq!(convert_k8s_cpu_value_to_f32(milli_cpu).unwrap(), 0.25 as f32);
        assert_eq!(convert_k8s_cpu_value_to_f32(int_cpu).unwrap(), 2 as f32);
    }

    #[test]
    pub fn test_cpu_set() {
        let v = vec![];
        let listener_helper = ListenersHelper::new(&v);
        let execution_id = "execution_id";
        let context_id = "context_id";

        let mut total_cpu = "0.25".to_string();
        let mut cpu_burst = "1".to_string();
        assert_eq!(
            validate_k8s_required_cpu_and_burstable(&listener_helper, execution_id, context_id, total_cpu, cpu_burst)
                .unwrap(),
            CpuLimits {
                cpu_request: "0.25".to_string(),
                cpu_limit: "1".to_string()
            }
        );

        total_cpu = "1".to_string();
        cpu_burst = "0.5".to_string();
        assert_eq!(
            validate_k8s_required_cpu_and_burstable(&listener_helper, execution_id, context_id, total_cpu, cpu_burst)
                .unwrap(),
            CpuLimits {
                cpu_request: "1".to_string(),
                cpu_limit: "1".to_string()
            }
        );
    }

    #[test]
    pub fn test_cname_resolution() {
        let resolver = cloudflare_dns_resolver();
        let cname = get_cname_record_value(&resolver, "ci-test-no-delete.qovery.io");

        assert_eq!(cname, Some(String::from("qovery.io.")));
    }

    #[test]
    pub fn test_versions_number() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            expected_output: Result<VersionsNumber, StringError>,
            description: &'a str,
        }

        let test_cases = vec![
            TestCase {
                input: "",
                expected_output: Err(StringError::from("version cannot be empty")),
                description: "empty version str",
            },
            TestCase {
                input: "    ",
                expected_output: Err(StringError::from("version cannot be empty")),
                description: "version a tab str",
            },
            TestCase {
                input: " ",
                expected_output: Err(StringError::from("version cannot be empty")),
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
