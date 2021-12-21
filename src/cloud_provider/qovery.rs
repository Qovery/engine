use reqwest::{header, Error};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QoveryAgent {
    pub kubernetes_id: String,
    pub version: String,
    pub object_type: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QoveryEngine {
    pub kubernetes_id: String,
    pub version: String,
    pub object_type: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QoveryShellAgent {
    pub kubernetes_id: String,
    pub version: String,
    pub object_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineLocation {
    ClientSide,
    QoverySide,
}

pub enum QoveryAppName {
    Agent,
    Engine,
    ShellAgent,
}

pub fn get_qovery_app_version<T: DeserializeOwned>(
    qovery_app_type: QoveryAppName,
    token: &str,
    api_fqdn: &str,
    cluster_id: &str,
) -> Result<T, Error> {
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("X-Qovery-Signature", token.parse().unwrap());

    let app_type = match qovery_app_type {
        QoveryAppName::Agent => "agent",
        QoveryAppName::Engine => "engine",
        QoveryAppName::ShellAgent => "shellAgent",
    };

    let url = format!(
        "https://{}/api/v1/{}-version?type=cluster&clusterId={}",
        api_fqdn, app_type, cluster_id
    );

    match reqwest::blocking::Client::new().get(&url).headers(headers).send() {
        Ok(x) => match x.json::<T>() {
            Ok(qa) => Ok(qa),
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    }
}
