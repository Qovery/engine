use serde::{Deserialize, Serialize};

/// Represents the different resource profiles available for metrics components.
/// Each profile defines specific CPU and memory values adapted to different usage scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "SCREAMING_SNAKE_CASE", deserialize = "SCREAMING_SNAKE_CASE"))]
#[derive(Default)]
pub enum ResourceProfile {
    /// Low resource profile - suitable for small clusters or development environments
    Low,
    /// Standard resource profile - suitable for most production use cases
    #[default]
    Normal,
    /// High resource profile - suitable for clusters with high load or many metrics
    High,
}

impl ResourceProfile {
    /// Returns a string representation of the profile
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }
}

/// Resource configuration for a specific component
#[derive(Debug, Clone)]
pub struct ResourceConfig {
    pub cpu_request: String,
    pub cpu_limit: String,
    pub memory_request: String,
    pub memory_limit: String,
}

impl ResourceConfig {
    pub fn new(cpu_request: &str, cpu_limit: &str, memory_request: &str, memory_limit: &str) -> Self {
        Self {
            cpu_request: cpu_request.to_string(),
            cpu_limit: cpu_limit.to_string(),
            memory_request: memory_request.to_string(),
            memory_limit: memory_limit.to_string(),
        }
    }
}

/// Resource configurations for Prometheus based on the chosen profile
pub struct PrometheusResources;

impl PrometheusResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("500m", "2000m", "1Gi", "1Gi"),
            ResourceProfile::Normal => ResourceConfig::new("1000m", "4000m", "4Gi", "4Gi"),
            ResourceProfile::High => ResourceConfig::new("2000m", "4000m", "8Gi", "8Gi"),
        }
    }
}

/// Resource configurations for Prometheus based on the chosen profile
pub struct PrometheusNodeExporterResources;

impl PrometheusNodeExporterResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("10m", "20m", "32Mi", "32Mi"),
            ResourceProfile::Normal => ResourceConfig::new("10m", "20m", "32Mi", "32Mi"),
            ResourceProfile::High => ResourceConfig::new("10m", "20m", "32Mi", "32Mi"),
        }
    }
}

/// Resource configurations for Prometheus based on the chosen profile
pub struct PrometheusOperatorResources;

impl PrometheusOperatorResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("200m", "1000m", "512Mi", "1Gi"),
            ResourceProfile::Normal => ResourceConfig::new("200m", "1000m", "1Gi", "1Gi"),
            ResourceProfile::High => ResourceConfig::new("500m", "1000m", "1Gi", "1Gi"),
        }
    }
}

/// Resource configurations for kube state metrics
pub struct KubeStateMetricsResources;

impl KubeStateMetricsResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("150m", "150m", "512Mi", "512Mi"),
            ResourceProfile::Normal => ResourceConfig::new("150m", "150m", "768Mi", "768Mi"),
            ResourceProfile::High => ResourceConfig::new("500m", "500m", "1Gi", "1Gi"),
        }
    }
}

/// Resource configurations for Thanos Query based on the chosen profile
pub struct ThanosQueryResources;

impl ThanosQueryResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("500m", "500m", "512Mi", "512Mi"),
            ResourceProfile::Normal => ResourceConfig::new("1000m", "1000m", "768Mi", "768Mi"),
            ResourceProfile::High => ResourceConfig::new("2000m", "2000m", "1Gi", "1Gi"),
        }
    }
}

/// Resource configurations for Thanos Store based on the chosen profile
pub struct ThanosStoreResources;

impl ThanosStoreResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("500m", "500m", "512Mi", "512Mi"),
            ResourceProfile::Normal => ResourceConfig::new("500m", "500m", "1Gi", "1Gi"),
            ResourceProfile::High => ResourceConfig::new("1000m", "1000m", "2Gi", "2Gi"),
        }
    }
}

/// Resource configurations for Thanos Compactor based on the chosen profile
pub struct ThanosCompactorResources;

impl ThanosCompactorResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("500m", "500m", "1Gi", "1Gi"),
            ResourceProfile::Normal => ResourceConfig::new("2000m", "2000m", "4Gi", "4Gi"),
            ResourceProfile::High => ResourceConfig::new("2000m", "2000m", "6Gi", "6Gi"),
        }
    }
}

/// Resource configurations for Prometheus Adapter based on the chosen profile
pub struct PrometheusAdapterResources;

impl PrometheusAdapterResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("250m", "250m", "384Mi", "384Mi"),
            ResourceProfile::Normal => ResourceConfig::new("250m", "250m", "384Mi", "384Mi"),
            ResourceProfile::High => ResourceConfig::new("400m", "400m", "512Mi", "512Mi"),
        }
    }
}

/// Resource configurations for AlertManager based on the chosen profile
pub struct AlertManagerResources;

impl AlertManagerResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("50m", "100m", "128Mi", "256Mi"),
            ResourceProfile::Normal => ResourceConfig::new("100m", "200m", "256Mi", "512Mi"),
            ResourceProfile::High => ResourceConfig::new("200m", "500m", "512Mi", "1Gi"),
        }
    }
}

/// Resource configurations for Yet Another Cloudwatch Exporter based on the chosen profile
pub struct YaceResources;

impl YaceResources {
    pub fn get(profile: ResourceProfile) -> ResourceConfig {
        match profile {
            ResourceProfile::Low => ResourceConfig::new("150", "150", "256Mi", "256Mi"),
            ResourceProfile::Normal => ResourceConfig::new("250m", "250m", "512Mi", "512Mi"),
            ResourceProfile::High => ResourceConfig::new("500m", "500m", "768Mi", "768Mi"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_profile_default() {
        assert_eq!(ResourceProfile::default(), ResourceProfile::Normal);
    }

    #[test]
    fn test_resource_profile_as_str() {
        assert_eq!(ResourceProfile::Low.as_str(), "low");
        assert_eq!(ResourceProfile::Normal.as_str(), "normal");
        assert_eq!(ResourceProfile::High.as_str(), "high");
    }

    #[test]
    fn test_prometheus_resources_low() {
        let resources = PrometheusResources::get(ResourceProfile::Low);
        assert_eq!(resources.cpu_request, "500m");
        assert_eq!(resources.cpu_limit, "2000m");
        assert_eq!(resources.memory_request, "1Gi");
        assert_eq!(resources.memory_limit, "1Gi");
    }

    #[test]
    fn test_prometheus_resources_normal() {
        let resources = PrometheusResources::get(ResourceProfile::Normal);
        assert_eq!(resources.cpu_request, "1000m");
        assert_eq!(resources.cpu_limit, "4000m");
        assert_eq!(resources.memory_request, "4Gi");
        assert_eq!(resources.memory_limit, "4Gi");
    }

    #[test]
    fn test_prometheus_resources_high() {
        let resources = PrometheusResources::get(ResourceProfile::High);
        assert_eq!(resources.cpu_request, "2000m");
        assert_eq!(resources.cpu_limit, "4000m");
        assert_eq!(resources.memory_request, "8Gi");
        assert_eq!(resources.memory_limit, "8Gi");
    }

    #[test]
    fn test_thanos_query_resources_all_profiles() {
        let low = ThanosQueryResources::get(ResourceProfile::Low);
        assert_eq!(low.cpu_request, "500m");

        let normal = ThanosQueryResources::get(ResourceProfile::Normal);
        assert_eq!(normal.cpu_request, "1000m");

        let high = ThanosQueryResources::get(ResourceProfile::High);
        assert_eq!(high.cpu_request, "2000m");
    }

    #[test]
    fn test_prometheus_adapter_resources_all_profiles() {
        let low = PrometheusAdapterResources::get(ResourceProfile::Low);
        assert_eq!(low.memory_request, "384Mi");

        let normal = PrometheusAdapterResources::get(ResourceProfile::Normal);
        assert_eq!(normal.memory_request, "384Mi");

        let high = PrometheusAdapterResources::get(ResourceProfile::High);
        assert_eq!(high.memory_request, "512Mi");
    }
}
