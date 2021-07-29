use crate::utilities::FuncTestsSecrets;
use qovery_engine::cloud_provider::utilities::cloudflare_dns_resolver;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;
use serde_json::json;
use std::fmt;
use tracing::info;
use trust_dns_resolver::error::ResolveError;

#[derive(Debug)]
pub enum DnsRecordType {
    A,
    CNAME,
}

impl fmt::Display for DnsRecordType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub struct DnsRecord {
    pub dns_type: DnsRecordType,
    pub src: String,
    pub dest: String,
    pub ttl: u32,
    pub priority: u32,
    pub proxied: bool,
    pub zone_id: String,
}

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    let secrets = FuncTestsSecrets::new();
    Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo",
        "Qovery Test Cloudflare",
        secrets.CLOUDFLARE_DOMAIN.unwrap().as_str(),
        secrets.CLOUDFLARE_TOKEN.unwrap().as_str(), // Cloudflare name: Qovery test
        secrets.CLOUDFLARE_ID.unwrap().as_str(),
    )
}

pub fn cloudflare_create_record(
    cloudflare_provider: &Cloudflare,
    dns_record: &DnsRecord,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    info!("creating dns record {} -> {}", &dns_record.src, &dns_record.dest);

    let json_data = json!({
      "type": dns_record.dns_type.to_string(),
      "name": dns_record.src.to_string(),
      "content": dns_record.dest.to_string(),
      "ttl": dns_record.ttl,
      "priority": dns_record.priority,
      "proxied": dns_record.proxied
    });

    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        dns_record.zone_id
    );

    let client = reqwest::blocking::Client::new();
    client
        .post(url.as_str())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .bearer_auth(cloudflare_provider.cloudflare_api_token.to_string())
        .json(&json_data)
        .send()
}

pub fn cloudflare_delete_record(
    cloudflare_provider: &Cloudflare,
    dns_record: &DnsRecord,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    info!("deleting dns record {} -> {}", &dns_record.src, &dns_record.dest);

    let json_data = json!({
      "type": dns_record.dns_type.to_string(),
      "name": dns_record.src.to_string(),
      "content": dns_record.dest.to_string(),
      "ttl": dns_record.ttl,
      "priority": dns_record.priority,
      "proxied": dns_record.proxied
    });

    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        dns_record.zone_id
    );

    let client = reqwest::blocking::Client::new();
    client
        .post(url.as_str())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .bearer_auth(cloudflare_provider.cloudflare_api_token.to_string())
        .json(&json_data)
        .send()
}

pub fn basic_dns_record_check_exists(domain: String) -> Result<(), ResolveError> {
    let resolver = cloudflare_dns_resolver();
    match resolver.lookup_ip(domain) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}
