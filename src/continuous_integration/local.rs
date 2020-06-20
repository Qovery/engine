use crate::continuous_integration::ContinuousIntegration;

pub struct Local {}

impl ContinuousIntegration for Local {
    fn is_valid(&self) -> bool {
        true
    }
}
