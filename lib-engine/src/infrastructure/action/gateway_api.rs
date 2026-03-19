use strum_macros::EnumIter;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum GatewayApiRolloutStatus {
    #[default]
    NotDeployed,
    DualStack,
    Default,
}

impl GatewayApiRolloutStatus {
    pub fn new(deployed: bool, default: bool) -> Self {
        match (deployed, default) {
            (true, false) => GatewayApiRolloutStatus::DualStack,
            (true, true) => GatewayApiRolloutStatus::Default,
            (false, _) => GatewayApiRolloutStatus::NotDeployed,
        }
    }

    pub fn is_deployed(&self) -> bool {
        match self {
            GatewayApiRolloutStatus::NotDeployed => false,
            GatewayApiRolloutStatus::DualStack | GatewayApiRolloutStatus::Default => true,
        }
    }

    pub fn is_default(&self) -> bool {
        match self {
            GatewayApiRolloutStatus::Default => true,
            GatewayApiRolloutStatus::DualStack | GatewayApiRolloutStatus::NotDeployed => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn test_new_all_combinations() {
        // Test all input combinations for the new() constructor
        assert_eq!(
            GatewayApiRolloutStatus::new(false, false),
            GatewayApiRolloutStatus::NotDeployed,
            "new(false, false) should be NotDeployed"
        );
        assert_eq!(
            GatewayApiRolloutStatus::new(false, true),
            GatewayApiRolloutStatus::NotDeployed,
            "new(false, true) should be NotDeployed (deployed=false takes precedence)"
        );
        assert_eq!(
            GatewayApiRolloutStatus::new(true, false),
            GatewayApiRolloutStatus::DualStack,
            "new(true, false) should be DualStack"
        );
        assert_eq!(
            GatewayApiRolloutStatus::new(true, true),
            GatewayApiRolloutStatus::Default,
            "new(true, true) should be Default"
        );
    }

    #[test]
    fn test_is_deployed_for_all_variants() {
        // Test is_deployed() for all enum variants
        for status in GatewayApiRolloutStatus::iter() {
            let expected = match status {
                GatewayApiRolloutStatus::NotDeployed => false,
                GatewayApiRolloutStatus::DualStack | GatewayApiRolloutStatus::Default => true,
            };
            assert_eq!(
                status.is_deployed(),
                expected,
                "is_deployed() for {status:?} should be {expected}"
            );
        }
    }

    #[test]
    fn test_is_default_for_all_variants() {
        // Test is_default() for all enum variants
        for status in GatewayApiRolloutStatus::iter() {
            let expected = matches!(status, GatewayApiRolloutStatus::Default);
            assert_eq!(
                status.is_default(),
                expected,
                "is_default() for {status:?} should be {expected}"
            );
        }
    }

    #[test]
    fn test_default_trait() {
        assert_eq!(
            GatewayApiRolloutStatus::default(),
            GatewayApiRolloutStatus::NotDeployed,
            "Default trait should return NotDeployed"
        );
    }

    #[test]
    fn test_lifecycle_not_deployed_to_dual_stack() {
        // Simulate lifecycle: not deployed -> dual stack deployment
        let status = GatewayApiRolloutStatus::new(false, false);
        assert!(!status.is_deployed());
        assert!(!status.is_default());

        let status = GatewayApiRolloutStatus::new(true, false);
        assert!(status.is_deployed());
        assert!(!status.is_default());
    }

    #[test]
    fn test_lifecycle_dual_stack_to_default() {
        // Simulate lifecycle: dual stack -> default
        let status = GatewayApiRolloutStatus::new(true, false);
        assert!(status.is_deployed());
        assert!(!status.is_default());

        let status = GatewayApiRolloutStatus::new(true, true);
        assert!(status.is_deployed());
        assert!(status.is_default());
    }

    #[test]
    fn test_lifecycle_complete_migration() {
        // Simulate complete lifecycle: not deployed -> dual stack -> default
        let statuses = [
            GatewayApiRolloutStatus::new(false, false), // Initial state
            GatewayApiRolloutStatus::new(true, false),  // Dual stack
            GatewayApiRolloutStatus::new(true, true),   // Default
        ];

        assert!(!statuses[0].is_deployed());
        assert!(statuses[1].is_deployed());
        assert!(statuses[2].is_deployed());

        assert!(!statuses[0].is_default());
        assert!(!statuses[1].is_default());
        assert!(statuses[2].is_default());
    }
}
