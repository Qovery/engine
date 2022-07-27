use itertools::Itertools;
use std::fmt::{Display, Formatter};
use std::net::Ipv4Addr;

/// Represent a String path instead of passing a PathBuf struct
pub type StringPath = String;

pub trait ToTerraformString {
    fn to_terraform_format_string(&self) -> String;
}

pub trait ToHelmString {
    fn to_helm_format_string(&self) -> String;
}

/// Represents a domain, just plain domain, no protocol.
/// eq. `test.com`, `sub.test.com`
#[derive(Clone)]
pub struct Domain {
    raw: String,
    root_domain: String,
}

impl Domain {
    pub fn new(raw: String) -> Self {
        // TODO(benjaminch): This is very basic solution which doesn't take into account
        // some edge cases such as: "test.co.uk" domains
        let sep: &str = ".";
        let items: Vec<String> = raw.split(sep).map(|e| e.to_string()).collect();
        let items_count = raw.matches(sep).count() + 1;
        let top_domain: String = match items_count > 2 {
            true => items.iter().skip(items_count - 2).join("."),
            false => items.iter().join("."),
        };

        Domain {
            root_domain: top_domain,
            raw,
        }
    }

    pub fn new_with_subdomain(raw: String, sub_domain: String) -> Self {
        Domain::new(format!("{}.{}", sub_domain, raw))
    }

    pub fn with_sub_domain(&self, sub_domain: String) -> Domain {
        Domain::new(format!("{}.{}", sub_domain, self.raw))
    }

    pub fn root_domain(&self) -> Domain {
        Domain::new(self.root_domain.to_string())
    }

    pub fn wildcarded(&self) -> Domain {
        if self.is_wildcarded() {
            return self.clone();
        }

        match self.raw.is_empty() {
            false => Domain::new_with_subdomain(self.raw.to_string(), "*".to_string()),
            true => Domain::new("*".to_string()),
        }
    }

    fn is_wildcarded(&self) -> bool {
        self.raw.starts_with('*')
    }
}

impl Display for Domain {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw.as_str())
    }
}

impl ToTerraformString for Domain {
    fn to_terraform_format_string(&self) -> String {
        format!("{{{}}}", self.raw)
    }
}

impl ToHelmString for Domain {
    fn to_helm_format_string(&self) -> String {
        format!("{{{}}}", self.raw)
    }
}

impl ToTerraformString for Ipv4Addr {
    fn to_terraform_format_string(&self) -> String {
        format!("{{{}}}", self)
    }
}

#[cfg(test)]
mod tests {
    use crate::io_models::domain::Domain;

    #[test]
    fn test_domain_new() {
        struct TestCase<'a> {
            input: String,
            expected_root_domain_output: String,
            expected_wildcarded_output: String,
            description: &'a str,
        }

        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: "".to_string(),
                expected_root_domain_output: "".to_string(),
                expected_wildcarded_output: "*".to_string(),
                description: "empty raw domain input",
            },
            TestCase {
                input: "*".to_string(),
                expected_root_domain_output: "*".to_string(),
                expected_wildcarded_output: "*".to_string(),
                description: "wildcard domain input",
            },
            TestCase {
                input: "*.test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.test.com".to_string(),
                description: "wildcarded domain input",
            },
            TestCase {
                input: "test.co.uk".to_string(),
                expected_root_domain_output: "co.uk".to_string(), // TODO(benjamin) => Should be test.co.uk in the future
                expected_wildcarded_output: "*.co.uk".to_string(),
                description: "broken edge case domain with special tld input",
            },
            TestCase {
                input: "test".to_string(),
                expected_root_domain_output: "test".to_string(),
                expected_wildcarded_output: "*.test".to_string(),
                description: "domain without tld input",
            },
            TestCase {
                input: "test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.test.com".to_string(),
                description: "simple top domain input",
            },
            TestCase {
                input: "sub.test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.sub.test.com".to_string(),
                description: "simple sub domain input",
            },
            TestCase {
                input: "yetanother.sub.test.com".to_string(),
                expected_root_domain_output: "test.com".to_string(),
                expected_wildcarded_output: "*.yetanother.sub.test.com".to_string(),
                description: "simple sub domain input",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = Domain::new(tc.input.clone());
            tc.expected_wildcarded_output; // to avoid warning

            // verify:
            assert_eq!(
                tc.expected_root_domain_output,
                result.root_domain().to_string(),
                "case {} : '{}'",
                tc.description,
                tc.input
            );
        }
    }
}
