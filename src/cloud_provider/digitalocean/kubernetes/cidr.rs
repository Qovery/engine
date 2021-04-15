use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

use crate::cmd::utilities;
use chrono::Duration;

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

pub fn get_used_cidr_on_region(token: &str) {
    let mut output_from_cli = String::new();
    let _ = utilities::exec_with_output(
        "doctl",
        vec!["vpcs", "list", "--output", "json", "-t", token],
        &vec![],
        |r_out| match r_out {
            Ok(s) => output_from_cli.push_str(&s.to_owned()),
            Err(e) => error!("DOCTL CLI does not respond correctly {}", e),
        },
        |r_err| match r_err {
            Ok(s) => error!("DOCTL CLI error from cmd inserted, please check vpcs list command{}", s),
            Err(e) => error!("DOCTL CLI does not respond correctly {}", e),
        },
        Duration::seconds(30),
    );

    let buff = output_from_cli.borrow();
    let _array: Vec<DoVpc> = serde_json::from_str(&buff).expect("JSON is not well-formatted");
}
