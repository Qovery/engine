use crate::helm::HelmChartNamespaces;
use crate::infrastructure::helm_charts::HelmChartResources;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LokiDeploymentMode {
    #[default]
    SingleBinary,
    SimpleScalable,
}

impl LokiDeploymentMode {
    pub fn service_name(&self) -> &'static str {
        match self {
            LokiDeploymentMode::SingleBinary => "loki",
            LokiDeploymentMode::SimpleScalable => "loki-write",
        }
    }

    pub fn kube_dns_name(&self, namespace: &HelmChartNamespaces) -> String {
        format!("{}.{namespace}.svc:3100", self.service_name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LokiParameters {
    pub deployment_mode: LokiDeploymentMode,
    pub log_retention_in_week: u32,
    pub write_resources: HelmChartResources,
    pub read_resources: HelmChartResources,
    pub backend_resources: HelmChartResources,
    pub single_binary_resources: HelmChartResources,
}

const DEFAULT_CPU_REQUEST_M: u32 = 300;
const DEFAULT_CPU_LIMIT_M: u32 = 8000;
const DEFAULT_MEMORY_REQUEST_MIB: u32 = 1024;
const DEFAULT_MEMORY_LIMIT_MIB: u32 = 2048;

fn component_resources(
    cpu_request_m: u32,
    cpu_limit_m: u32,
    memory_request_mib: u32,
    memory_limit_mib: u32,
) -> HelmChartResources {
    HelmChartResources {
        request_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(cpu_request_m)),
        limit_cpu: Some(KubernetesCpuResourceUnit::MilliCpu(cpu_limit_m)),
        request_memory: Some(KubernetesMemoryResourceUnit::MebiByte(memory_request_mib)),
        limit_memory: Some(KubernetesMemoryResourceUnit::MebiByte(memory_limit_mib)),
    }
}

fn default_component_resources() -> HelmChartResources {
    component_resources(
        DEFAULT_CPU_REQUEST_M,
        DEFAULT_CPU_LIMIT_M,
        DEFAULT_MEMORY_REQUEST_MIB,
        DEFAULT_MEMORY_LIMIT_MIB,
    )
}

impl Default for LokiParameters {
    fn default() -> Self {
        Self {
            deployment_mode: LokiDeploymentMode::default(),
            log_retention_in_week: 12,
            write_resources: default_component_resources(),
            read_resources: default_component_resources(),
            backend_resources: default_component_resources(),
            single_binary_resources: default_component_resources(),
        }
    }
}

impl LokiParameters {
    pub fn from_advanced_settings(s: &ClusterAdvancedSettings) -> Self {
        Self {
            deployment_mode: s.loki_deployment_mode.clone(),
            log_retention_in_week: s.loki_log_retention_in_week,
            write_resources: component_resources(
                s.loki_write_cpu_request_m.unwrap_or(DEFAULT_CPU_REQUEST_M),
                s.loki_write_cpu_limit_m.unwrap_or(DEFAULT_CPU_LIMIT_M),
                s.loki_write_memory_request_mib.unwrap_or(DEFAULT_MEMORY_REQUEST_MIB),
                s.loki_write_memory_limit_mib.unwrap_or(DEFAULT_MEMORY_LIMIT_MIB),
            ),
            read_resources: component_resources(
                s.loki_read_cpu_request_m.unwrap_or(DEFAULT_CPU_REQUEST_M),
                s.loki_read_cpu_limit_m.unwrap_or(DEFAULT_CPU_LIMIT_M),
                s.loki_read_memory_request_mib.unwrap_or(DEFAULT_MEMORY_REQUEST_MIB),
                s.loki_read_memory_limit_mib.unwrap_or(DEFAULT_MEMORY_LIMIT_MIB),
            ),
            backend_resources: component_resources(
                s.loki_backend_cpu_request_m.unwrap_or(DEFAULT_CPU_REQUEST_M),
                s.loki_backend_cpu_limit_m.unwrap_or(DEFAULT_CPU_LIMIT_M),
                s.loki_backend_memory_request_mib.unwrap_or(DEFAULT_MEMORY_REQUEST_MIB),
                s.loki_backend_memory_limit_mib.unwrap_or(DEFAULT_MEMORY_LIMIT_MIB),
            ),
            single_binary_resources: component_resources(
                s.loki_single_binary_cpu_request_m.unwrap_or(DEFAULT_CPU_REQUEST_M),
                s.loki_single_binary_cpu_limit_m.unwrap_or(DEFAULT_CPU_LIMIT_M),
                s.loki_single_binary_memory_request_mib
                    .unwrap_or(DEFAULT_MEMORY_REQUEST_MIB),
                s.loki_single_binary_memory_limit_mib
                    .unwrap_or(DEFAULT_MEMORY_LIMIT_MIB),
            ),
        }
    }

    /// Raises per-component CPU request/limit to the given floors (in milli-CPU), leaving higher values untouched.
    ///
    /// Use `0` to skip a given floor. GKE Autopilot requires 500m request + 1000m limit
    /// for pods using `podAntiAffinity`.
    pub fn with_cpu_floors_m(mut self, request_floor_m: u32, limit_floor_m: u32) -> Self {
        for r in [
            &mut self.write_resources,
            &mut self.read_resources,
            &mut self.backend_resources,
            &mut self.single_binary_resources,
        ] {
            if let Some(KubernetesCpuResourceUnit::MilliCpu(v)) = r.request_cpu {
                r.request_cpu = Some(KubernetesCpuResourceUnit::MilliCpu(v.max(request_floor_m)));
            }
            if let Some(KubernetesCpuResourceUnit::MilliCpu(v)) = r.limit_cpu {
                r.limit_cpu = Some(KubernetesCpuResourceUnit::MilliCpu(v.max(limit_floor_m)));
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advanced_settings_with_loki(
        mode: LokiDeploymentMode,
        retention_weeks: u32,
        write_cpu_req: u32,
        read_cpu_req: u32,
        backend_cpu_req: u32,
        single_binary_cpu_req: u32,
    ) -> ClusterAdvancedSettings {
        ClusterAdvancedSettings {
            loki_deployment_mode: mode,
            loki_log_retention_in_week: retention_weeks,
            loki_write_cpu_request_m: Some(write_cpu_req),
            loki_read_cpu_request_m: Some(read_cpu_req),
            loki_backend_cpu_request_m: Some(backend_cpu_req),
            loki_single_binary_cpu_request_m: Some(single_binary_cpu_req),
            ..Default::default()
        }
    }

    fn cpu_m(r: &HelmChartResources, requested: bool) -> u32 {
        let cpu = if requested { &r.request_cpu } else { &r.limit_cpu };
        match cpu {
            Some(KubernetesCpuResourceUnit::MilliCpu(v)) => *v,
            _ => panic!("expected MilliCpu"),
        }
    }

    fn memory_mib(r: &HelmChartResources, requested: bool) -> u32 {
        let mem = if requested { &r.request_memory } else { &r.limit_memory };
        match mem {
            Some(KubernetesMemoryResourceUnit::MebiByte(v)) => *v,
            _ => panic!("expected MebiByte"),
        }
    }

    #[test]
    fn from_advanced_settings_maps_all_fields() {
        let settings = advanced_settings_with_loki(LokiDeploymentMode::SimpleScalable, 4, 600, 700, 400, 350);
        let params = LokiParameters::from_advanced_settings(&settings);

        assert_eq!(params.deployment_mode, LokiDeploymentMode::SimpleScalable);
        assert_eq!(params.log_retention_in_week, 4);
        assert_eq!(cpu_m(&params.write_resources, true), 600);
        assert_eq!(cpu_m(&params.read_resources, true), 700);
        assert_eq!(cpu_m(&params.backend_resources, true), 400);
        assert_eq!(cpu_m(&params.single_binary_resources, true), 350);
        // other resource fields come through unchanged (defaults in this fixture)
        assert_eq!(cpu_m(&params.write_resources, false), 8000);
        assert_eq!(memory_mib(&params.write_resources, true), 1024);
        assert_eq!(memory_mib(&params.write_resources, false), 2048);
    }

    #[test]
    fn from_advanced_settings_default_mode_is_single_binary() {
        let settings = ClusterAdvancedSettings::default();
        let params = LokiParameters::from_advanced_settings(&settings);
        assert_eq!(params.deployment_mode, LokiDeploymentMode::SingleBinary);
        assert_eq!(params.log_retention_in_week, 12);
        assert_eq!(cpu_m(&params.write_resources, true), 300);
        assert_eq!(cpu_m(&params.single_binary_resources, true), 300);
    }

    #[test]
    fn with_cpu_floors_m_raises_request_below_floor_and_keeps_above() {
        let settings = advanced_settings_with_loki(LokiDeploymentMode::SimpleScalable, 12, 200, 750, 300, 300);
        let floored = LokiParameters::from_advanced_settings(&settings).with_cpu_floors_m(500, 0);

        assert_eq!(cpu_m(&floored.write_resources, true), 500, "below-floor write raised");
        assert_eq!(cpu_m(&floored.read_resources, true), 750, "above-floor read kept");
        assert_eq!(cpu_m(&floored.backend_resources, true), 500, "at-default backend raised");
        assert_eq!(cpu_m(&floored.single_binary_resources, true), 500);
        // other fields preserved
        assert_eq!(cpu_m(&floored.write_resources, false), 8000);
        assert_eq!(memory_mib(&floored.write_resources, true), 1024);
    }

    #[test]
    fn with_cpu_floors_m_raises_limit_below_floor_and_keeps_above() {
        let settings = ClusterAdvancedSettings {
            loki_write_cpu_limit_m: Some(500),
            loki_read_cpu_limit_m: Some(4000),
            ..Default::default()
        };

        let floored = LokiParameters::from_advanced_settings(&settings).with_cpu_floors_m(0, 1000);

        assert_eq!(cpu_m(&floored.write_resources, false), 1000, "below-floor write limit raised");
        assert_eq!(cpu_m(&floored.read_resources, false), 4000, "above-floor read limit kept");
        assert_eq!(
            cpu_m(&floored.backend_resources, false),
            8000,
            "default 8000m kept (above floor)"
        );
        assert_eq!(cpu_m(&floored.write_resources, true), 300, "request untouched by limit floor");
    }
}
