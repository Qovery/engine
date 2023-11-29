use crate::models::ToCloudProviderFormat;
use serde_derive::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter)]
// Sync with Qovery Core team if you update this content
pub enum GcpRegion {
    EuropeWest9, // Paris
}

impl ToCloudProviderFormat for GcpRegion {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            GcpRegion::EuropeWest9 => "europe-west9",
        }
    }
}

impl Display for GcpRegion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_cloud_provider_format())
    }
}

impl FromStr for GcpRegion {
    type Err = ();

    fn from_str(s: &str) -> Result<GcpRegion, ()> {
        let v: &str = &s.to_lowercase();
        match v {
            "europe-west9" => Ok(GcpRegion::EuropeWest9),
            // TODO(benjaminch): Add all regions GCP integration
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::gcp::regions::GcpRegion;
    use crate::models::ToCloudProviderFormat;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_gcp_region_to_gcp_format() {
        for region in GcpRegion::iter() {
            assert_eq!(
                match region {
                    GcpRegion::EuropeWest9 => "europe-west9",
                },
                region.to_cloud_provider_format()
            );
        }
    }

    #[test]
    fn test_gcp_region_to_string() {
        for region in GcpRegion::iter() {
            assert_eq!(
                match region {
                    GcpRegion::EuropeWest9 => "europe-west9",
                },
                region.to_string()
            );
        }
    }

    #[test]
    fn test_gcp_region_from_str() {
        // test all supported regions
        for region in GcpRegion::iter() {
            assert_eq!(region, GcpRegion::from_str(&region.to_cloud_provider_format()).unwrap());
        }

        // test unsupported region
        assert!(GcpRegion::from_str("an-unsupported-region").is_err());
    }
}
