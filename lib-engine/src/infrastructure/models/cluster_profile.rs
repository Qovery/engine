/// Defines the cluster profile types for infrastructure configurations.
#[derive(PartialEq, Eq)]
pub enum ClusterProfile {
    Small,
    Medium,
    Large,
    ExtraLarge,
}
