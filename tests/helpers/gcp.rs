use once_cell::sync::Lazy;
use qovery_engine::cloud_provider::gcp::regions::GcpRegion;
use std::time::Duration;

pub const GCP_REGION: GcpRegion = GcpRegion::EuropeWest9;

pub static GCP_RESOURCE_TTL: Lazy<Duration> = Lazy::new(|| Duration::from_secs(4 * 60 * 60)); // 4 hours
