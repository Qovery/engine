extern crate serde_json;

use reqwest::StatusCode;

use crate::cloud_provider::digitalocean::models::load_balancers::LoadBalancer;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::utilities::get_header_with_bearer;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub const DO_LOAD_BALANCER_API_PATH: &str = "https://api.digitalocean.com/v2/load_balancers";

pub fn get_ip_from_do_load_balancer_api_output(json_content: &str) -> Result<Ipv4Addr, SimpleError> {
    let res_load_balancer = serde_json::from_str::<LoadBalancer>(json_content);

    match res_load_balancer {
        Ok(lb) => match Ipv4Addr::from_str(&lb.load_balancer.ip) {
            Ok(ip) => Ok(ip),
            Err(e) => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(format!(
                    "Info returned from DO API is not a valid IP, received '{:?}' instead. {:?}",
                    &lb.load_balancer.ip, e
                )),
            )),
        },
        Err(_) => Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some("Error While trying to deserialize json received from Digital Ocean Load Balancer API".to_string()),
        )),
    }
}

pub fn do_get_load_balancer_ip(token: &str, load_balancer_id: &str) -> Result<Ipv4Addr, SimpleError> {
    let headers = get_header_with_bearer(token);
    let url = format!("{}/{}", DO_LOAD_BALANCER_API_PATH, load_balancer_id);
    let res = reqwest::blocking::Client::new().get(&url).headers(headers).send();

    return match res {
        Ok(response) => match response.status() {
            StatusCode::OK => {
                let content = response.text().unwrap();
                get_ip_from_do_load_balancer_api_output(content.as_str())
            }
            _ => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(
                    format!("Unknown status code received from Digital Ocean Kubernetes API while retrieving load balancer information. {:?}", response),
                ),
            )),
        },
        Err(_) => {
            Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("Unable to get a response from Digital Ocean Load Balancer API"),
            ))
        }
    };
}

#[cfg(test)]
mod tests_do_api_output {
    use crate::cloud_provider::digitalocean::network::load_balancer::get_ip_from_do_load_balancer_api_output;

    #[test]
    fn check_load_balancer_ip() {
        // https://developers.digitalocean.com/documentation/v2/#retrieve-an-existing-load-balancer
        let json_content = r#"
{
  "load_balancer": {
    "id": "4de7ac8b-495b-4884-9a69-1050c6793cd6",
    "name": "example-lb-01",
    "ip": "104.131.186.241",
    "size": "lb-small",
    "algorithm": "round_robin",
    "status": "new",
    "created_at": "2017-02-01T22:22:58Z",
    "forwarding_rules": [
      {
        "entry_protocol": "http",
        "entry_port": 80,
        "target_protocol": "http",
        "target_port": 80,
        "certificate_id": "",
        "tls_passthrough": false
      },
      {
        "entry_protocol": "https",
        "entry_port": 444,
        "target_protocol": "https",
        "target_port": 443,
        "certificate_id": "",
        "tls_passthrough": true
      }
    ],
    "health_check": {
      "protocol": "http",
      "port": 80,
      "path": "/",
      "check_interval_seconds": 10,
      "response_timeout_seconds": 5,
      "healthy_threshold": 5,
      "unhealthy_threshold": 3
    },
    "sticky_sessions": {
      "type": "none"
    },
    "region": {
      "name": "New York 3",
      "slug": "nyc3",
      "sizes": [
        "s-1vcpu-1gb",
        "s-1vcpu-2gb",
        "s-1vcpu-3gb",
        "s-2vcpu-2gb",
        "s-3vcpu-1gb",
        "s-2vcpu-4gb",
        "s-4vcpu-8gb",
        "s-6vcpu-16gb",
        "s-8vcpu-32gb",
        "s-12vcpu-48gb",
        "s-16vcpu-64gb",
        "s-20vcpu-96gb",
        "s-24vcpu-128gb",
        "s-32vcpu-192gb"
      ],
      "features": [
        "private_networking",
        "backups",
        "ipv6",
        "metadata",
        "install_agent"
      ],
      "available": true
    },
    "tag": "",
    "droplet_ids": [
      3164444,
      3164445
    ],
    "redirect_http_to_https": false,
    "enable_proxy_protocol": false,
    "enable_backend_keepalive": false,
    "vpc_uuid": "c33931f2-a26a-4e61-b85c-4e95a2ec431b"
  }
}
        "#;
        let ip_returned_from_api = get_ip_from_do_load_balancer_api_output(json_content);

        assert_eq!(ip_returned_from_api.unwrap().to_string(), "104.131.186.241");
    }
}
