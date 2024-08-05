use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

#[derive(Debug, PartialEq)]
pub struct ServiceRequestsAndLimits {
    pub cpu_request_in_milli: KubernetesCpuResourceUnit,
    pub cpu_limit_in_milli: KubernetesCpuResourceUnit,
    pub ram_request_in_mib: KubernetesMemoryResourceUnit,
    pub ram_limit_in_mib: KubernetesMemoryResourceUnit,
}

pub fn compute_service_requests_and_limits(
    cpu_request_in_milli: u32,
    cpu_limit_in_milli: u32,
    ram_request_in_mib: u32,
    ram_limit_in_mib: u32,
    overridden_cpu_limit_in_milli: Option<u32>,
    overridden_ram_limit_in_mib: Option<u32>,
    allow_service_cpu_overcommit: bool,
    allow_service_ram_overcommit: bool,
) -> Result<ServiceRequestsAndLimits, String> {
    if cpu_request_in_milli == 0 {
        return Err("cpu_request_in_milli must be greater than 0".to_string());
    }

    if cpu_limit_in_milli == 0 {
        return Err("cpu_limit_in_milli must be greater than 0".to_string());
    }

    if cpu_request_in_milli > cpu_limit_in_milli {
        return Err("cpu_request_in_milli must be less or equal to cpu_limit_in_milli".to_string());
    }

    if ram_request_in_mib == 0 {
        return Err("ram_request_in_mib must be greater than 0".to_string());
    }

    if ram_limit_in_mib == 0 {
        return Err("ram_limit_in_mib must be greater than 0".to_string());
    }

    if ram_request_in_mib > ram_limit_in_mib {
        return Err("ram_request_in_mib must be less or equal to ram_limit_in_mib".to_string());
    }

    // Return early if limit overrides are disabled
    if !allow_service_cpu_overcommit && !allow_service_ram_overcommit {
        return Ok(ServiceRequestsAndLimits {
            cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_request_in_milli),
            cpu_limit_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_limit_in_milli),
            ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_request_in_mib),
            ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_limit_in_mib),
        });
    }

    if !allow_service_cpu_overcommit && overridden_cpu_limit_in_milli.is_some() {
        return Err(
            "This is forbidden to override cpu limit, update your cluster advanced settings to enable it".to_string(),
        );
    }

    if !allow_service_ram_overcommit && overridden_ram_limit_in_mib.is_some() {
        return Err(
            "This is forbidden to override ram limit, update your cluster advanced settings to enable it".to_string(),
        );
    }

    // Compute cpu & ram limits according to service overridden limits in advanced settings
    let new_cpu_limit_in_milli = overridden_cpu_limit_in_milli.unwrap_or(cpu_limit_in_milli);
    let new_ram_limit_in_mib = overridden_ram_limit_in_mib.unwrap_or(ram_limit_in_mib);

    // Re-check coherence between new overridden limit and request
    if new_cpu_limit_in_milli == 0 {
        return Err("overridden_cpu_limit_in_milli must be greater than 0".to_string());
    }

    if new_ram_limit_in_mib == 0 {
        return Err("overridden_ram_limit_in_mib must be greater than 0".to_string());
    }

    if cpu_request_in_milli > new_cpu_limit_in_milli {
        return Err("cpu_request_in_milli must be less or equal to overridden_cpu_limit_in_milli".to_string());
    }

    if ram_request_in_mib > new_ram_limit_in_mib {
        return Err("ram_request_in_mib must be less or equal to overridden_ram_limit_in_mib".to_string());
    }

    Ok(ServiceRequestsAndLimits {
        cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_request_in_milli),
        cpu_limit_in_milli: KubernetesCpuResourceUnit::MilliCpu(new_cpu_limit_in_milli),
        ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_request_in_mib),
        ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(new_ram_limit_in_mib),
    })
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
    use crate::models::service_resource::{compute_service_requests_and_limits, ServiceRequestsAndLimits};

    //
    // Tests with overridden disabled

    #[test]
    fn should_reject_when_cpu_request_is_zero() {
        // given
        let cpu_request_in_milli = 0;

        // when
        let result = compute_service_requests_and_limits(cpu_request_in_milli, 200, 256, 256, None, None, false, false);

        // then
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "cpu_request_in_milli must be greater than 0");
    }

    #[test]
    fn should_reject_when_cpu_limit_is_zero() {
        // given
        let cpu_limit_in_milli = 0;

        // when
        let result = compute_service_requests_and_limits(200, cpu_limit_in_milli, 256, 256, None, None, false, false);

        // then
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "cpu_limit_in_milli must be greater than 0");
    }

    #[test]
    fn should_reject_when_cpu_request_is_greater_than_cpu_limit() {
        // given
        let cpu_request_in_milli = 200;
        let cpu_limit_in_milli = 100;

        // when
        let result = compute_service_requests_and_limits(
            cpu_request_in_milli,
            cpu_limit_in_milli,
            256,
            256,
            None,
            None,
            false,
            false,
        );

        // then
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            "cpu_request_in_milli must be less or equal to cpu_limit_in_milli"
        );
    }

    #[test]
    fn should_reject_when_ram_request_is_zero() {
        // given
        let ram_request_in_mib = 0;

        // when
        let result = compute_service_requests_and_limits(200, 200, ram_request_in_mib, 256, None, None, false, false);

        // then
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "ram_request_in_mib must be greater than 0");
    }

    #[test]
    fn should_reject_when_ram_limit_is_zero() {
        // given
        let _ram_limit_in_mib = 0;

        // when
        let result = compute_service_requests_and_limits(200, 200, 256, 0, None, None, false, false);

        // then
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "ram_limit_in_mib must be greater than 0");
    }

    #[test]
    fn should_reject_when_ram_request_is_greater_than_ram_limit() {
        // given
        let ram_request_in_mib = 256;
        let ram_limit_in_mib = 128;

        // when
        let result = compute_service_requests_and_limits(
            200,
            200,
            ram_request_in_mib,
            ram_limit_in_mib,
            None,
            None,
            false,
            false,
        );

        // then
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            "ram_request_in_mib must be less or equal to ram_limit_in_mib"
        );
    }

    #[test]
    fn should_get_service_resource_settings_when_no_override_is_disabled() {
        // given
        let cpu_request_in_milli = 200;
        let cpu_limit_in_milli = 200;
        let ram_request_in_mib = 256;
        let ram_limit_in_mib = 256;

        // when
        let result = compute_service_requests_and_limits(
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            None,
            None,
            false,
            false,
        );

        // then
        assert!(result.is_ok());
        let resource_settings = result.unwrap();
        assert_eq!(
            resource_settings,
            ServiceRequestsAndLimits {
                cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_request_in_milli),
                cpu_limit_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_limit_in_milli),
                ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_request_in_mib),
                ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_limit_in_mib),
            }
        );
    }

    //
    // Tests with overridden enabled

    #[test]
    fn should_reject_when_overridden_cpu_limit_is_defined_but_forbidden_at_cluster_level() {
        // given
        let overridden_cpu_limit_in_milli = Some(200);

        // when
        let result =
            compute_service_requests_and_limits(100, 100, 256, 256, overridden_cpu_limit_in_milli, None, false, true);

        // then
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            "This is forbidden to override cpu limit, update your cluster advanced settings to enable it"
        );
    }

    #[test]
    fn should_reject_when_overridden_ram_limit_is_defined_but_forbidden_at_cluster_level() {
        // given
        let overriden_ram_limit_in_mib = Some(1024);

        // when
        let result =
            compute_service_requests_and_limits(100, 100, 256, 256, None, overriden_ram_limit_in_mib, true, false);

        // then
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            "This is forbidden to override ram limit, update your cluster advanced settings to enable it"
        );
    }

    #[test]
    fn should_reject_when_overridden_cpu_limit_is_zero() {
        // given
        let overridden_cpu_limit_in_milli = Some(0);

        // when
        let result =
            compute_service_requests_and_limits(100, 100, 256, 256, overridden_cpu_limit_in_milli, None, true, false);

        // then
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "overridden_cpu_limit_in_milli must be greater than 0");
    }

    #[test]
    fn should_reject_when_cpu_request_is_greater_than_overridden_cpu_limit() {
        // given
        let cpu_request_in_milli = 200;
        let cpu_limit_in_milli = 200;
        let overridden_cpu_limit_in_milli = Some(100);

        // when
        let result = compute_service_requests_and_limits(
            cpu_request_in_milli,
            cpu_limit_in_milli,
            256,
            256,
            overridden_cpu_limit_in_milli,
            None,
            true,
            false,
        );

        // then
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            "cpu_request_in_milli must be less or equal to overridden_cpu_limit_in_milli"
        );
    }

    #[test]
    fn should_reject_when_overridden_ram_limit_is_zero() {
        // given
        let overridden_ram_limit_in_mib = Some(0);

        // when
        let result =
            compute_service_requests_and_limits(200, 200, 256, 256, None, overridden_ram_limit_in_mib, false, true);

        // then
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "overridden_ram_limit_in_mib must be greater than 0");
    }

    #[test]
    fn should_reject_when_ram_request_is_greater_than_overridden_ram_limit() {
        // given
        let ram_request_in_mib = 256;
        let ram_limit_in_mib = 256;
        let overridden_ram_limit_in_mib = Some(128);

        // when
        let result = compute_service_requests_and_limits(
            200,
            200,
            ram_request_in_mib,
            ram_limit_in_mib,
            None,
            overridden_ram_limit_in_mib,
            false,
            true,
        );

        // then
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            "ram_request_in_mib must be less or equal to overridden_ram_limit_in_mib"
        );
    }

    #[test]
    fn should_get_service_resource_settings_when_override_is_enabled_and_overridden_values_are_set() {
        // given
        let cpu_request_in_milli = 200;
        let cpu_limit_in_milli = 250;
        let ram_request_in_mib = 256;
        let ram_limit_in_mib = 512;
        let overridden_cpu_limit_in_milli = 300;
        let overridden_ram_limit_in_mib = 1024;

        // when
        let result = compute_service_requests_and_limits(
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            Some(overridden_cpu_limit_in_milli),
            Some(overridden_ram_limit_in_mib),
            true,
            true,
        );

        // then
        assert!(result.is_ok());
        let resource_settings = result.unwrap();
        assert_eq!(
            resource_settings,
            ServiceRequestsAndLimits {
                cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_request_in_milli),
                cpu_limit_in_milli: KubernetesCpuResourceUnit::MilliCpu(overridden_cpu_limit_in_milli),
                ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_request_in_mib),
                ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(overridden_ram_limit_in_mib),
            }
        );
    }

    #[test]
    fn should_get_service_resource_settings_when_override_is_enabled_and_overridden_values_are_not_set() {
        // given
        let cpu_request_in_milli = 200;
        let cpu_limit_in_milli = 300;
        let ram_request_in_mib = 256;
        let ram_limit_in_mib = 512;
        let overridden_cpu_limit_in_milli = None;
        let overridden_ram_limit_in_mib = None;

        // when
        let result = compute_service_requests_and_limits(
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            overridden_cpu_limit_in_milli,
            overridden_ram_limit_in_mib,
            false,
            true,
        );

        // then
        assert!(result.is_ok());
        let resource_settings = result.unwrap();
        assert_eq!(
            resource_settings,
            ServiceRequestsAndLimits {
                cpu_request_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_request_in_milli),
                cpu_limit_in_milli: KubernetesCpuResourceUnit::MilliCpu(cpu_limit_in_milli),
                ram_request_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_request_in_mib),
                ram_limit_in_mib: KubernetesMemoryResourceUnit::MebiByte(ram_limit_in_mib),
            }
        );
    }
}
