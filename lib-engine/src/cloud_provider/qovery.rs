use crate::errors::CommandError;
use reqwest::header;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QoveryAgent {
    pub kubernetes_id: String,
    pub version: String,
    pub object_type: String,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QoveryEngine {
    pub kubernetes_id: String,
    pub version: String,
    pub object_type: String,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QoveryShellAgent {
    pub kubernetes_id: String,
    pub version: String,
    pub object_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineLocation {
    ClientSide,
    QoverySide,
}

pub enum QoveryAppName {
    Agent,
    Engine,
    ShellAgent,
    ClusterAgent,
}

pub fn get_qovery_app_version<T: DeserializeOwned>(
    qovery_app_type: QoveryAppName,
    token: &str,
    api_fqdn: &str,
    cluster_id: &str,
) -> Result<T, CommandError> {
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("X-Qovery-Signature", token.parse().unwrap());

    let app_type = match qovery_app_type {
        QoveryAppName::Agent => "agent",
        QoveryAppName::Engine => "engine",
        QoveryAppName::ShellAgent => "shellAgent",
        QoveryAppName::ClusterAgent => "clusterAgent",
    };

    let url = format!(
        "https://{}/api/v1/{}-version?type=cluster&clusterId={}",
        api_fqdn, app_type, cluster_id
    );

    let message_safe = format!("Error while trying to get `{}` version.", app_type);

    match reqwest::blocking::Client::new().get(&url).headers(headers).send() {
        Ok(x) => match x.json::<T>() {
            Ok(qa) => Ok(qa),
            Err(e) => Err(CommandError::new(message_safe, Some(e.to_string()), None)),
        },
        Err(e) => Err(CommandError::new(message_safe, Some(e.to_string()), None)),
    }
}
