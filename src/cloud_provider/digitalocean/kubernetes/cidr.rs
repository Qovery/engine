use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::cmd::utilities;
use std::borrow::Borrow;

#[derive(Serialize, Deserialize, Debug)]
pub struct DoVpc {
    id: String,
    urn: String,
    name: String,
    ip_range: String,
    region: String,
    created_at: String,
    default: bool,
}

fn get_forbidden_cidr_per_region() -> HashMap<&'static str, &'static str, RandomState> {
    // see https://www.digitalocean.com/docs/networking/vpc/
    let mut forbidden_cidr = HashMap::new();
    forbidden_cidr.insert("AMS1", "10.11.0.0/16");
    forbidden_cidr.insert("AMS2", "10.14.0.0/16");
    forbidden_cidr.insert("AMS3", "10.18.0.0/16");
    forbidden_cidr.insert("BLR1", "10.47.0.0/16");
    forbidden_cidr.insert("FRA1", "10.19.0.0/16");
    forbidden_cidr.insert("LON1", "10.16.0.0/16");
    forbidden_cidr.insert("NYC1", "10.10.0.0/16");
    forbidden_cidr.insert("NYC2", "10.13.0.0/16");
    forbidden_cidr.insert("NYC3", "10.17.0.0/16");
    forbidden_cidr.insert("SFO1", "10.12.0.0/16");
    forbidden_cidr.insert("SFO2", "10.46.0.0/16");
    forbidden_cidr.insert("SFO3", "10.48.0.0/16");
    forbidden_cidr.insert("SGP1", "10.15.0.0/16");
    forbidden_cidr.insert("TOR1", "10.20.0.0/16");
    // for all regions
    forbidden_cidr.insert("ALL", "10.244.0.0/16");
    forbidden_cidr.insert("ALL", "10.245.0.0/16");
    // the /24 is not a typo ;)
    forbidden_cidr.insert("ALL", "10.246.0.0/24");
    forbidden_cidr
}

pub fn get_used_cidr_on_region() {
    let mut output_from_cli = String::new();
    utilities::exec_with_output(
        "doctl",
        vec![
            "vpcs",
            "list",
            "--output",
            "json",
            "-t",
            "34158dea3388309455954a9602be686de63b84ca6374db04588e818731ccf184",
        ],
        |r_out| match r_out {
            Ok(s) => output_from_cli.push_str(&s.to_owned()),
            Err(e) => error!("DOCTL Cli not respond well{}", e),
        },
        |r_err| match r_err {
            Ok(s) => error!(
                "DOCTL Cli error from cmd inserted, please check vpcs list command{}",
                s
            ),
            Err(e) => error!("DOCTL Cli not respond good {}", e),
        },
    );
    let buff = output_from_cli.borrow();
    let array: Vec<DoVpc> = serde_json::from_str(&buff).expect("JSON was not well-formatted");
    for elem in array.iter() {
        let reg = &elem.region;
        let ip = &elem.ip_range;
    }
}
