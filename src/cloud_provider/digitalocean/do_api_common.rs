use std::fmt;

use reqwest::StatusCode;

use crate::errors::CommandError;
use crate::utilities::get_header_with_bearer;

pub const DIGITAL_OCEAN_API_URL: &str = "https://api.digitalocean.com";

#[derive(Clone, Copy, Debug)]
pub enum DoApiType {
    Doks,
    Vpc,
}

impl fmt::Display for DoApiType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl DoApiType {
    pub fn api_url(&self) -> String {
        match self {
            DoApiType::Doks => format!("{}/v2/kubernetes", DIGITAL_OCEAN_API_URL),
            DoApiType::Vpc => format!("{}/v2/vpcs", DIGITAL_OCEAN_API_URL),
        }
    }
}

pub fn do_get_from_api(token: &str, api_type: DoApiType, url_api: String) -> Result<String, CommandError> {
    let headers = get_header_with_bearer(token);
    let res = reqwest::blocking::Client::new().get(url_api).headers(headers).send();

    match res {
        Ok(response) => match response.status() {
            StatusCode::OK => Ok(response.text().expect("Cannot get response text")),
            StatusCode::UNAUTHORIZED => Err(CommandError::new(
                format!(
                    "Could not get {} information, ensure your DigitalOcean token is valid.",
                    api_type
                ),
                Some(format!("response: {:?}", response)),
                None,
            )),
            _ => Err(CommandError::new(
                format!(
                    "Unknown status code received from Digital Ocean Kubernetes API while retrieving {} information.",
                    api_type
                ),
                Some(format!("response: {:?}", response)),
                None,
            )),
        },
        Err(_) => Err(CommandError::new_from_safe_message(format!(
            "Unable to get a response from Digital Ocean {} API",
            api_type
        ))),
    }
}
