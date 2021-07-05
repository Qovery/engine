use crate::utilities::FuncTestsSecrets;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;
use std::collections::HashMap;
use std::fmt;
use tracing::info;

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

pub async fn cloudflare_create_cname_record(
    cloudflare_provider: &Cloudflare,
    dns_record: &DnsRecord,
) -> Result<reqwest::Response, reqwest::Error> {
    info!("creating dns record {} -> {}", &dns_record.src, &dns_record.dest);

    let mut json_data = HashMap::new();
    json_data.insert("type", dns_record.dns_type.to_string());
    json_data.insert("name", dns_record.src.to_string());
    json_data.insert("content", dns_record.dest.to_string());
    json_data.insert("ttl", dns_record.ttl.to_string());
    json_data.insert("priority", dns_record.priority.to_string());
    json_data.insert("proxied", dns_record.proxied.to_string());

    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        dns_record.zone_id
    );

    let client = reqwest::Client::new();
    client
        .post(&url)
        .bearer_auth(cloudflare_provider.cloudflare_api_token.to_string())
        .json(&json_data)
        .send()
        .await

    // let cf_creds: Credentials = UserAuthToken {
    //     token: "VxdnVQA0lBh6TAdfG0MWI0ZByFnvERuGms06rlNW".to_string(),
    // };
    //
    // let token = "VxdnVQA0lBh6TAdfG0MWI0ZByFnvERuGms06rlNW";
    // let mut config = HttpApiClientConfig::default();
    // let mut headers = HeaderMap::default();
    // //.append("Authorization", format!("Bearer {}", token).as_str());
    // let cf_client = HttpApiClient::new(cf_creds, header, Environment::Production).unwrap();
    // let response = cf_client.request(&dns::CreateDnsRecord {
    //     zone_identifier: cf_zone_id,
    //     params: record,
    // });
    //
    // match response {
    //     Ok(_) => {
    //         info!("Cloudflare dns record {}, was created", &dns_src);
    //         Ok(())
    //     }
    //     Err(e) => match &e {
    //         ApiFailure::Error(status, errors) => {
    //             error!("Cloudflare error HTTP {}:", status);
    //             for err in &errors.errors {
    //                 error!("Cloudflare error {}: {}", err.code, err.message);
    //                 for (k, v) in &err.other {
    //                     error!("Cloudflare error: {}: {}", k, v);
    //                 }
    //             }
    //             for (k, v) in &errors.other {
    //                 error!("Cloudflare error {}: {}", k, v);
    //             }
    //             Err(e)
    //         }
    //         ApiFailure::Invalid(reqwest_err) => {
    //             error!("Cloudflare error: {}", reqwest_err);
    //             Err(e)
    //         }
    //     },
    // }
}
