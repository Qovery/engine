//! Bounded synchronous connectivity probes for endpoints and registries.

use super::model::{FactState, evidence};
use super::runtime::block_on_timeout;
use crate::io_models::platform_components::PlatformHelmUnit;
use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use url::Url;

const NETWORK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

const NETWORK_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn probe_http_endpoints(
    endpoints: &[crate::io_models::platform_components::PlatformPreflightEndpoint],
) -> FactState {
    if endpoints.is_empty() {
        return FactState::Unavailable(evidence([("input", "qoveryEndpoints")]));
    }
    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(NETWORK_CONNECT_TIMEOUT)
        .timeout(NETWORK_REQUEST_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(_) => return FactState::Unavailable(BTreeMap::new()),
    };
    for endpoint in endpoints {
        let Some(url) = normalize_https_url(&endpoint.url) else {
            return FactState::Unavailable(evidence([("endpoint", endpoint.key.as_str())]));
        };
        if client.head(url.clone()).send().is_err() {
            return FactState::Fail(evidence([
                ("endpoint", endpoint.key.as_str()),
                ("host", url.host_str().unwrap_or("<invalid>")),
            ]));
        }
    }
    FactState::Pass
}

pub(super) fn probe_chart_registries(units: &[PlatformHelmUnit]) -> FactState {
    if units.is_empty() {
        return FactState::Pass;
    }
    let endpoints: Vec<_> = units
        .iter()
        .map(|unit| (unit.chart.repository.as_str(), unit.key.as_str()))
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .map(
            |(repository, unit_key)| crate::io_models::platform_components::PlatformPreflightEndpoint {
                key: unit_key.to_string(),
                url: repository.to_string(),
            },
        )
        .collect();
    probe_http_endpoints(&endpoints)
}

pub(super) fn probe_container_registries(units: &[PlatformHelmUnit]) -> FactState {
    let registries: BTreeSet<String> = units
        .iter()
        .flat_map(|unit| &unit.images)
        .filter_map(|image| container_registry_address(&image.repository))
        .collect();
    if registries.is_empty() {
        return FactState::Unavailable(evidence([("input", "images")]));
    }
    for registry in registries {
        let addresses = match resolve_socket_addresses(&registry) {
            Some(addresses) => addresses,
            None => return FactState::Fail(evidence([("registry", registry.as_str())])),
        };
        if !addresses
            .into_iter()
            .take(4)
            .any(|socket| TcpStream::connect_timeout(&socket, NETWORK_CONNECT_TIMEOUT).is_ok())
        {
            return FactState::Fail(evidence([("registry", registry.as_str())]));
        }
    }
    FactState::Pass
}

pub(super) fn resolve_socket_addresses(address: &str) -> Option<Vec<SocketAddr>> {
    block_on_timeout(NETWORK_REQUEST_TIMEOUT, tokio::net::lookup_host(address))
        .ok()
        .and_then(Result::ok)
        .map(Iterator::collect)
}

fn normalize_https_url(raw: &str) -> Option<Url> {
    let normalized = if raw.starts_with("oci://") {
        raw.replacen("oci://", "https://", 1)
    } else if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    Url::parse(&normalized).ok().filter(|url| url.host_str().is_some())
}

fn container_registry_address(repository: &str) -> Option<String> {
    let first = repository.trim().trim_start_matches("oci://").split('/').next()?;
    if first.is_empty() {
        None
    } else if first.contains('.') || first.contains(':') || first == "localhost" {
        Some(if first.contains(':') {
            first.to_string()
        } else {
            format!("{first}:443")
        })
    } else {
        Some("registry-1.docker.io:443".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::container_registry_address;

    #[test]
    fn image_registry_detection_handles_docker_hub_and_explicit_hosts() {
        assert_eq!(
            container_registry_address("nginx"),
            Some("registry-1.docker.io:443".to_string())
        );
        assert_eq!(
            container_registry_address("public.ecr.aws/qovery/engine"),
            Some("public.ecr.aws:443".to_string())
        );
        assert_eq!(
            container_registry_address("registry.qovery.svc.cluster.local:5000/demo/image"),
            Some("registry.qovery.svc.cluster.local:5000".to_string())
        );
    }
}
