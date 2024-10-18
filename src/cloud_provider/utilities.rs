use crate::cloud_provider::models::InvalidStatefulsetStorage;
use crate::cloud_provider::service::{increase_storage_size, Service};
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::logger::Logger;
use crate::runtime::block_on;
use core::option::Option::Some;
use core::result::Result;
use core::result::Result::{Err, Ok};
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use retry::delay::Fixed;
use retry::{Error, OperationResult};
use serde::de::DeserializeOwned;
use std::fmt;
use std::io;
use std::net::{SocketAddr, TcpStream as NetTcpStream};
use std::net::{ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub fn managed_db_name_sanitizer(max_size: usize, prefix: &str, name: &str) -> String {
    let max_size = max_size - prefix.len();
    let mut new_name = format!("{}{}", prefix, name.replace(['_', '-'], ""));
    if new_name.chars().count() > max_size {
        new_name = new_name[..max_size].to_string();
    }
    new_name
}

#[derive(PartialEq, Eq, Debug)]
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
            TcpCheckSource::SocketAddr(x) => write!(f, "{x}"),
            TcpCheckSource::DnsName(x) => write!(f, "{x}"),
        }
    }
}

pub fn check_tcp_port_is_open(address: &TcpCheckSource, port: u16) -> Result<(), TcpCheckErrors> {
    let timeout = Duration::from_secs(1);

    let ip = match address {
        TcpCheckSource::SocketAddr(x) => *x,
        TcpCheckSource::DnsName(x) => {
            let address = format!("{x}:{port}");
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

pub fn check_udp_port_is_open(address: &str, port: u16) -> io::Result<bool> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let full_address = format!("{}:{}", address, port);
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;

    socket.send_to(b"qovery", full_address)?;

    // Attempt to receive a response
    let mut buf = [0; 512];
    match socket.recv_from(&mut buf) {
        Ok(_) => Ok(true), // A response was received, port is open
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false), // Timeout, port is closed
        Err(e) => Err(e),  // An actual error occurred
    }
}

pub fn wait_until_port_is_open(
    address: &TcpCheckSource,
    port: u16,
    max_timeout: usize,
    logger: &dyn Logger,
    event_details: EventDetails,
) -> Result<(), TcpCheckErrors> {
    let fixed_iterable = Fixed::from(Duration::from_secs(1)).take(max_timeout);
    let check_result = retry::retry(fixed_iterable, || match check_tcp_port_is_open(address, port) {
        Ok(_) => OperationResult::Ok(()),
        Err(e) => {
            logger.log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!("{address}:{port} is still not ready: {e:?}. retrying...")),
            ));
            OperationResult::Retry(e)
        }
    });

    match check_result {
        Ok(_) => Ok(()),
        Err(Error { error, .. }) => Err(error),
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
    let msg = format!("{cloud_provider_name}.{struct_name}.{fn_name} called for {item_name}");
    match fn_name.contains("error") {
        true => logger.log(EngineEvent::Warning(event_details, EventMessage::new_from_safe(msg))),
        false => logger.log(EngineEvent::Info(event_details, EventMessage::new_from_safe(msg))),
    }
}

pub fn are_pvcs_bound(
    service: &dyn Service,
    namespace: &str,
    event_details: &EventDetails,
    kube_client: &kube::Client,
) -> Result<(), Box<EngineError>> {
    let selector = service.kube_label_selector();
    match block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
        kube_client,
        namespace,
        &selector,
    )) {
        Ok(pvcs) => {
            for pvc in pvcs.items {
                if let (Some(status), Some(name)) = (pvc.status, pvc.metadata.name) {
                    if let Some(phase) = status.phase {
                        if phase.to_lowercase().as_str() != "bound" {
                            return Err(Box::new(EngineError::new_k8s_cannot_bound_pvc(
                                event_details.clone(),
                                CommandError::new_from_safe_message(format!("Can't bound PVC {name}")),
                                service.name(),
                            )));
                        };
                    }
                }
            }

            Ok(())
        }
        Err(e) => Err(Box::new(EngineError::new_k8s_enable_to_get_pvc(event_details.clone(), e))),
    }
}

pub fn update_pvcs(
    service: &dyn Service,
    invalid_statefulset: &InvalidStatefulsetStorage,
    namespace: &str,
    event_details: &EventDetails,
    client: &kube::Client,
) -> Result<(), Box<EngineError>> {
    block_on(increase_storage_size(namespace, invalid_statefulset, event_details, client))?;

    are_pvcs_bound(service, namespace, event_details, client)?;

    Ok(())
}

pub fn from_terraform_value<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::de::Deserializer<'de>,
    T: DeserializeOwned,
{
    use serde::Deserialize;

    #[derive(serde_derive::Deserialize)]
    struct TerraformJsonValue<T> {
        value: T,
    }

    TerraformJsonValue::deserialize(deserializer).map(|o: TerraformJsonValue<T>| o.value)
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::utilities::from_terraform_value;
    use crate::cloud_provider::utilities::{
        check_tcp_port_is_open, check_udp_port_is_open, TcpCheckErrors, TcpCheckSource,
    };
    use crate::errors::CommandError;
    use crate::models::types::VersionsNumber;
    use std::str::FromStr;

    #[test]
    pub fn test_terraform_value_parsing() {
        let json = r#"
{
  "aws_account_id": {
    "sensitive": false,
    "type": "string",
    "value": "843237546537"
  },
  "aws_iam_alb_controller_arn": {
    "sensitive": false,
    "type": "string",
    "value": "arn:aws:iam::843237546537:role/qovery-eks-alb-controller-z00000019"
  },
  "aws_iam_cloudwatch_role_arn": {
    "sensitive": false,
    "type": "string",
    "value": "arn:aws:iam::843237546537:role/qovery-cloudwatch-z00000019"
  },
  "aws_number": {
    "sensitive": false,
    "type": "number",
    "value": 12
  },
  "aws_float": {
    "sensitive": false,
    "type": "number",
    "value": 12.64
  },
  "aws_list": {
    "sensitive": false,
    "type": "list",
    "value": [
      "a",
      "b",
      "c"
    ]
  }
}
        "#;

        #[derive(serde_derive::Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "from_terraform_value")]
            aws_account_id: String,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_iam_alb_controller_arn: String,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_iam_cloudwatch_role_arn: String,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_number: u32,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_float: f32,
            #[serde(deserialize_with = "from_terraform_value")]
            aws_list: Vec<String>,
        }

        let value: TestStruct = serde_json::from_str(json).unwrap();
        assert_eq!(value.aws_account_id, "843237546537");
        assert_eq!(
            value.aws_iam_alb_controller_arn,
            "arn:aws:iam::843237546537:role/qovery-eks-alb-controller-z00000019"
        );
        assert_eq!(
            value.aws_iam_cloudwatch_role_arn,
            "arn:aws:iam::843237546537:role/qovery-cloudwatch-z00000019"
        );
        assert_eq!(value.aws_number, 12);
        assert_eq!(value.aws_float, 12.64);
        assert!(!value.aws_list.is_empty());
    }

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

    #[test]
    #[ignore] // comment if you locally want to perform tests
    fn test_udp_port_closed() {
        // random port that should be closed
        let result = check_udp_port_is_open("127.0.0.1", 65535);
        assert!(result.is_err(), "port 65535 is opened while it was expected to be closed");
    }

    #[test]
    #[ignore] // comment if you locally want to perform tests
    fn test_udp_port_open() {
        let result = check_udp_port_is_open("127.0.0.1", 8080);
        assert!(
            result.is_ok(),
            "Expected the udp port to be open: 8080. Got an error: {:?}",
            result
        );
        assert!(result.unwrap());
    }
}
