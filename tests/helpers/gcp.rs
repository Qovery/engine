use chrono::Duration;
use once_cell::sync::Lazy;
use qovery_engine::cloud_provider::gcp::regions::GcpRegion;

pub const GCP_REGION: GcpRegion = GcpRegion::EuropeWest9;

pub static GCP_RESOURCE_TTL: Lazy<Duration> = Lazy::new(|| Duration::hours(4));
