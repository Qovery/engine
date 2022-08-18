use serde::{Deserialize, Serialize};

use crate::cloud_provider::digitalocean::do_api_common::{do_get_from_api, DoApiType};
use crate::cloud_provider::digitalocean::models::vpc::{Vpc, Vpcs};
use crate::errors::CommandError;
use crate::models::digital_ocean::DoRegion;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VpcInitKind {
    Autodetect,
    Manual,
}

impl ToString for VpcInitKind {
    fn to_string(&self) -> String {
        match self {
            VpcInitKind::Autodetect => "autodetect".to_string(),
            VpcInitKind::Manual => "manual".to_string(),
        }
    }
}

impl Default for VpcInitKind {
    fn default() -> Self {
        VpcInitKind::Autodetect
    }
}

pub fn get_do_subnet_available_from_api(
    token: &str,
    desired_subnet: String,
    region: DoRegion,
) -> Result<Option<Vpc>, CommandError> {
    // get subnets from the API
    let vpcs = match do_get_from_api(token, DoApiType::Vpc, DoApiType::Vpc.api_url()) {
        Ok(x) => do_get_vpcs_from_api_output(x.as_str())?,
        Err(e) => return Err(e),
    };

    // ensure it's available
    get_do_vpc_from_subnet(desired_subnet, vpcs, region)
}

pub fn get_do_vpc_name_available_from_api(token: &str, desired_name: String) -> Result<Option<Vpc>, CommandError> {
    // get names from the API
    let vpcs = match do_get_from_api(token, DoApiType::Vpc, DoApiType::Vpc.api_url()) {
        Ok(x) => do_get_vpcs_from_api_output(x.as_str())?,
        Err(e) => return Err(e),
    };

    // ensure it's available
    Ok(get_do_vpc_from_name(desired_name, vpcs))
}

pub fn get_do_random_available_subnet_from_api(token: &str, region: DoRegion) -> Result<String, CommandError> {
    let json_content = do_get_from_api(token, DoApiType::Vpc, DoApiType::Vpc.api_url())?;
    let existing_vpcs = do_get_vpcs_from_api_output(&json_content)?;
    get_random_available_subnet(existing_vpcs, region)
}

fn get_random_available_subnet(existing_vpcs: Vec<Vpc>, region: DoRegion) -> Result<String, CommandError> {
    let subnet_start = 0;
    let subnet_end = 254;

    for looping_subnet in subnet_start..subnet_end {
        let current_subnet = format!("10.{}.0.0/16", looping_subnet);

        match get_do_vpc_from_subnet(current_subnet.clone(), existing_vpcs.clone(), region) {
            Ok(vpc) => match vpc {
                // available
                None => return Ok(current_subnet),
                // already used
                Some(_) => continue,
            },
            // reserved ip
            Err(_) => continue,
        }
    }

    Err(CommandError::new_from_safe_message(
        "no available subnet found on this Digital Ocean account.".to_string(),
    ))
}

fn get_do_vpc_from_name(desired_name: String, existing_vpcs: Vec<Vpc>) -> Option<Vpc> {
    let mut exists = None;

    for vpc in existing_vpcs {
        if vpc.name == desired_name {
            exists = Some(vpc);
            break;
        }
    }

    exists
}

fn get_do_vpc_from_subnet(
    desired_subnet: String,
    existing_vpcs: Vec<Vpc>,
    region: DoRegion,
) -> Result<Option<Vpc>, CommandError> {
    let mut exists = None;

    match is_do_reserved_vpc_subnets(region, desired_subnet.as_str()) {
        true => Err(CommandError::new_from_safe_message(format!(
            "subnet {} can't be used because it's a DigitalOcean dedicated subnet",
            desired_subnet
        ))),
        false => {
            for vpc in existing_vpcs {
                if vpc.ip_range == desired_subnet {
                    exists = Some(vpc);
                    break;
                }
            }
            Ok(exists)
        }
    }
}

fn do_get_vpcs_from_api_output(json_content: &str) -> Result<Vec<Vpc>, CommandError> {
    // better to use lib when VPC will be supported https://github.com/LoganDark/digitalocean/issues/3
    let res_vpcs = serde_json::from_str::<Vpcs>(json_content);

    match res_vpcs {
        Ok(vpcs) => Ok(vpcs.vpcs),
        Err(e) => Err(CommandError::new(
            "Error while trying to deserialize json received from Digital Ocean VPC API".to_string(),
            Some(e.to_string()),
            None,
        )),
    }
}

// https://docs.digitalocean.com/products/networking/vpc/
fn is_do_reserved_vpc_subnets(region: DoRegion, subnet: &str) -> bool {
    // reserved DigitalOcean IPs
    let mut do_all_regions_reserved_ips = vec!["10.244.0.0/16", "10.245.0.0/16", "10.246.0.0/24"];

    let region_ip = match region {
        DoRegion::NewYorkCity1 => "10.10.0.0/16",
        DoRegion::NewYorkCity2 => "10.13.0.0/16",
        DoRegion::NewYorkCity3 => "10.17.0.0/16",
        DoRegion::Amsterdam2 => "10.14.0.0/16",
        DoRegion::Amsterdam3 => "10.18.0.0/16",
        DoRegion::SanFrancisco1 => "10.12.0.0/16",
        DoRegion::SanFrancisco2 => "10.46.0.0/16",
        DoRegion::SanFrancisco3 => "10.48.0.0/16",
        DoRegion::Singapore => "10.15.0.0/16",
        DoRegion::London => "10.16.0.0/16",
        DoRegion::Frankfurt => "10.19.0.0/16",
        DoRegion::Toronto => "10.20.0.0/16",
        DoRegion::Bangalore => "10.47.0.0/16",
    };
    do_all_regions_reserved_ips.push(region_ip);

    // ensure the subnet is not reserved
    for reserved_ip in do_all_regions_reserved_ips {
        if reserved_ip == subnet {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests_do_vpcs {
    use crate::cloud_provider::digitalocean::network::vpc::{
        do_get_vpcs_from_api_output, get_do_vpc_from_name, get_do_vpc_from_subnet, get_random_available_subnet,
        is_do_reserved_vpc_subnets, VpcInitKind,
    };
    use crate::models::digital_ocean::DoRegion;

    fn do_get_vpc_json() -> String {
        // https://developers.digitalocean.com/documentation/v2/#retrieve-an-existing-load-balancer
        let content = r#"
{
  "vpcs": [
    {
      "id": "b1efe641-5115-4a06-87bf-4e0b0a7bb50f",
      "urn": "do:vpc:b1efe641-5115-4a06-87bf-4e0b0a7bb50f",
      "name": "iEqZuC1zi3GHP8yn",
      "description": "",
      "region": "nyc3",
      "ip_range": "10.2.0.0/16",
      "created_at": "2021-02-16T10:52:12Z",
      "default": false
    },
    {
      "id": "aeb265f0-813d-4387-80c7-c96910b64597",
      "urn": "do:vpc:aeb265f0-813d-4387-80c7-c96910b64597",
      "name": "default-ams3",
      "description": "",
      "region": "ams3",
      "ip_range": "10.110.0.0/20",
      "created_at": "2021-01-04T14:23:20Z",
      "default": true
    },
    {
      "id": "849041b2-049c-43a5-ae93-4266d440fec3",
      "urn": "do:vpc:849041b2-049c-43a5-ae93-4266d440fec3",
      "name": "default-nyc1",
      "description": "",
      "region": "nyc1",
      "ip_range": "10.116.0.0/20",
      "created_at": "2020-12-29T23:33:42Z",
      "default": true
    },
    {
      "id": "4d986a19-c26a-413b-ae4b-b8413126b24b",
      "urn": "do:vpc:4d986a19-c26a-413b-ae4b-b8413126b24b",
      "name": "qovery-community-nyc",
      "description": "",
      "region": "nyc3",
      "ip_range": "10.1.0.0/16",
      "created_at": "2020-12-26T14:41:21Z",
      "default": true
    },
    {
      "id": "c669c237-62b8-48f1-97e5-2648e7d7e21f",
      "urn": "do:vpc:c669c237-62b8-48f1-97e5-2648e7d7e21f",
      "name": "qovery-test",
      "description": "",
      "region": "fra1",
      "ip_range": "10.0.0.0/16",
      "created_at": "2020-09-07T16:53:29Z",
      "default": true
    }
  ],
  "links": {},
  "meta": {
    "total": 5
  }
}
        "#;

        content.to_string()
    }

    #[test]
    fn check_reserved_subnets() {
        // if not reserved
        assert!(!is_do_reserved_vpc_subnets(DoRegion::Frankfurt, "192.168.0.0/24"));
        // if region reserved
        assert!(is_do_reserved_vpc_subnets(DoRegion::Frankfurt, "10.19.0.0/16"));
        // if world wide reserved
        assert!(is_do_reserved_vpc_subnets(DoRegion::Frankfurt, "10.244.0.0/16"));
    }

    #[test]
    fn do_get_subnets_from_api_calls() {
        let json_content = do_get_vpc_json();
        let vpcs = do_get_vpcs_from_api_output(&json_content).unwrap();
        let vpc_subnets: Vec<String> = vpcs.into_iter().map(|x| x.ip_range).collect();

        let joined_subnets = vpc_subnets.join(",");
        assert_eq!(
            joined_subnets,
            "10.2.0.0/16,10.110.0.0/20,10.116.0.0/20,10.1.0.0/16,10.0.0.0/16"
        );
    }

    #[test]
    fn do_ensure_subnet_availability() {
        let json_content = do_get_vpc_json();
        let vpcs = do_get_vpcs_from_api_output(&json_content).unwrap();

        // available
        assert!(
            get_do_vpc_from_subnet("10.3.0.0/16".to_string(), vpcs.clone(), DoRegion::Frankfurt)
                .unwrap()
                .is_none()
        );
        // already used
        assert_eq!(
            get_do_vpc_from_subnet("10.2.0.0/16".to_string(), vpcs.clone(), DoRegion::Frankfurt)
                .unwrap()
                .unwrap()
                .ip_range,
            "10.2.0.0/16".to_string()
        );
        // DO reserved subnet in the same region
        assert!(get_do_vpc_from_subnet("10.19.0.0/16".to_string(), vpcs.clone(), DoRegion::Frankfurt).is_err());
        // DO reserved subnet in another region
        assert!(get_do_vpc_from_subnet("10.19.0.0/16".to_string(), vpcs, DoRegion::London)
            .unwrap()
            .is_none());
    }

    #[test]
    fn do_ensure_vpc_name_exists() {
        let json_content = do_get_vpc_json();
        let existing_vpcs = do_get_vpcs_from_api_output(&json_content).unwrap();

        assert!(get_do_vpc_from_name("qovery-community-nyc".to_string(), existing_vpcs.clone()).is_some());
        assert!(get_do_vpc_from_name("non_existing_name".to_string(), existing_vpcs).is_none());
    }

    #[test]
    fn do_check_get_random_available_subnet() {
        let json_content = do_get_vpc_json();
        let existing_vpcs = do_get_vpcs_from_api_output(&json_content).unwrap();

        assert!(get_random_available_subnet(existing_vpcs, DoRegion::Frankfurt).is_ok());
    }

    #[test]
    fn test_do_vpc_set_to_string() {
        // setup:
        let variants = vec![VpcInitKind::Autodetect, VpcInitKind::Manual];

        for variant in variants.iter() {
            // execute:
            let result = variant.to_string();

            // verify:
            assert_eq!(
                match variant {
                    VpcInitKind::Autodetect => "autodetect".to_string(),
                    VpcInitKind::Manual => "manual".to_string(),
                },
                result
            );
        }
    }
}
