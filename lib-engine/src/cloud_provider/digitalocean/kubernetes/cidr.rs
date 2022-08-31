use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

use crate::cmd::command::{ExecutableCommand, QoveryCommand};

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

    let mut cmd = QoveryCommand::new("doctl", &["vpcs", "list", "--output", "json", "-t", token], &[]);
    let _ = cmd.exec_with_output(&mut |r_out| output_from_cli.push_str(&r_out), &mut |r_err| {
        error!("DOCTL CLI error from cmd inserted, please check vpcs list command{}", r_err)
    });

    let buff = output_from_cli.borrow();
    let _array: Vec<DoVpc> = serde_json::from_str(buff).expect("JSON is not well-formatted");
}
