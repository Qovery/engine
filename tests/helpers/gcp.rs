use chrono::Duration;
use lazy_static::lazy_static;
use qovery_engine::cloud_provider::gcp::regions::GcpRegion;
use qovery_engine::cloud_provider::kubernetes::KubernetesVersion;

pub const GCP_REGION: GcpRegion = GcpRegion::EuropeWest9;

lazy_static! {
    pub static ref GCP_RESOURCE_TTL: Duration = Duration::hours(4);
}
