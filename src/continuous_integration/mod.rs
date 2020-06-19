mod local;

pub trait ContinuousIntegration {
    fn is_valid(&self) -> bool;
}
