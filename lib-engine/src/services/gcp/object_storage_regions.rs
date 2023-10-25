use crate::cloud_provider::gcp::regions::GcpRegion;
use crate::models::ToCloudProviderFormat;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(PartialEq, Eq, Debug, Clone, EnumIter, Hash)]
pub enum GcpStorageRegion {
    EuropeWest9,
}

impl From<GcpRegion> for GcpStorageRegion {
    fn from(value: GcpRegion) -> Self {
        match value {
            GcpRegion::EuropeWest9 => GcpStorageRegion::EuropeWest9,
        }
    }
}

impl ToCloudProviderFormat for GcpStorageRegion {
    fn to_cloud_provider_format(&self) -> String {
        self.to_string()
    }
}

impl Display for GcpStorageRegion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            GcpStorageRegion::EuropeWest9 => "EUROPE-WEST9",
        })
    }
}

impl FromStr for GcpStorageRegion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_uppercase().as_str() {
            "EUROPE-WEST9" => Ok(GcpStorageRegion::EuropeWest9),
            _ => Err(format!("Unknown storage region: `{}`.", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::gcp::regions::GcpRegion;
    use crate::models::ToCloudProviderFormat;
    use crate::services::gcp::object_storage_regions::GcpStorageRegion;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_gcp_storage_region_to_gcp_format() {
        for region in GcpStorageRegion::iter() {
            assert_eq!(
                match region {
                    GcpStorageRegion::EuropeWest9 => "EUROPE-WEST9",
                },
                region.to_cloud_provider_format()
            );
        }
    }

    #[test]
    fn test_gcp_storage_region_to_string() {
        for region in GcpStorageRegion::iter() {
            assert_eq!(
                match region {
                    GcpStorageRegion::EuropeWest9 => "EUROPE-WEST9",
                },
                region.to_string()
            );
        }
    }

    #[test]
    fn test_gcp_storage_region_from_gcp_region() {
        for region in GcpRegion::iter() {
            assert_eq!(
                match region {
                    GcpRegion::EuropeWest9 => GcpStorageRegion::EuropeWest9,
                },
                GcpStorageRegion::from(region)
            );
        }
    }

    #[test]
    fn test_gcp_storage_region_from_str_success() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            expected: GcpStorageRegion,
        }
        let test_cases = vec![
            TestCase {
                input: "EUROPE-WEST9",
                expected: GcpStorageRegion::EuropeWest9,
            },
            TestCase {
                input: " EUROPE-WEST9  ",
                expected: GcpStorageRegion::EuropeWest9,
            },
            TestCase {
                input: "europe-west9",
                expected: GcpStorageRegion::EuropeWest9,
            },
            TestCase {
                input: " europe-west9",
                expected: GcpStorageRegion::EuropeWest9,
            },
        ];

        for tc in test_cases {
            // execution:
            let res = GcpStorageRegion::from_str(tc.input);

            // validate:
            assert!(res.is_ok());
            assert_eq!(tc.expected, res.unwrap());
        }
    }
}
