use crate::environment::models::ToCloudProviderFormat;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::object_storage::StorageRegion;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(PartialEq, Eq, Debug, Clone, EnumIter, Hash)]
pub enum GcpStorageRegion {
    AsiaEast1,
    AsiaEast2,
    AsiaNortheast1,
    AsiaNortheast2,
    AsiaNortheast3,
    AsiaSouth1,
    AsiaSouth2,
    AsiaSoutheast1,
    AsiaSoutheast2,
    AustraliaSoutheast1,
    AustraliaSoutheast2,
    EuropeCentral2,
    EuropeNorth1,
    EuropeSouthwest1,
    EuropeWest1,
    EuropeWest10,
    EuropeWest12,
    EuropeWest2,
    EuropeWest3,
    EuropeWest4,
    EuropeWest6,
    EuropeWest8,
    EuropeWest9,
    MeCentral1,
    MeCentral2,
    MeWest1,
    NorthAmericaNortheast1,
    NorthAmericaNortheast2,
    SouthAmericaEast1,
    SouthAmericaWest1,
    UsCentral1,
    UsEast1,
    UsEast4,
    UsEast5,
    UsSouth1,
    UsWest1,
    UsWest2,
    UsWest3,
    UsWest4,
}

impl From<GcpRegion> for GcpStorageRegion {
    fn from(value: GcpRegion) -> Self {
        match value {
            GcpRegion::AsiaEast1 => GcpStorageRegion::AsiaEast1,
            GcpRegion::AsiaEast2 => GcpStorageRegion::AsiaEast2,
            GcpRegion::AsiaNortheast1 => GcpStorageRegion::AsiaNortheast1,
            GcpRegion::AsiaNortheast2 => GcpStorageRegion::AsiaNortheast2,
            GcpRegion::AsiaNortheast3 => GcpStorageRegion::AsiaNortheast3,
            GcpRegion::AsiaSouth1 => GcpStorageRegion::AsiaSouth1,
            GcpRegion::AsiaSouth2 => GcpStorageRegion::AsiaSouth2,
            GcpRegion::AsiaSoutheast1 => GcpStorageRegion::AsiaSoutheast1,
            GcpRegion::AsiaSoutheast2 => GcpStorageRegion::AsiaSoutheast2,
            GcpRegion::AustraliaSoutheast1 => GcpStorageRegion::AustraliaSoutheast1,
            GcpRegion::AustraliaSoutheast2 => GcpStorageRegion::AustraliaSoutheast2,
            GcpRegion::EuropeCentral2 => GcpStorageRegion::EuropeCentral2,
            GcpRegion::EuropeNorth1 => GcpStorageRegion::EuropeNorth1,
            GcpRegion::EuropeSouthwest1 => GcpStorageRegion::EuropeSouthwest1,
            GcpRegion::EuropeWest1 => GcpStorageRegion::EuropeWest1,
            GcpRegion::EuropeWest10 => GcpStorageRegion::EuropeWest10,
            GcpRegion::EuropeWest12 => GcpStorageRegion::EuropeWest12,
            GcpRegion::EuropeWest2 => GcpStorageRegion::EuropeWest2,
            GcpRegion::EuropeWest3 => GcpStorageRegion::EuropeWest3,
            GcpRegion::EuropeWest4 => GcpStorageRegion::EuropeWest4,
            GcpRegion::EuropeWest6 => GcpStorageRegion::EuropeWest6,
            GcpRegion::EuropeWest8 => GcpStorageRegion::EuropeWest8,
            GcpRegion::EuropeWest9 => GcpStorageRegion::EuropeWest9,
            GcpRegion::MeCentral1 => GcpStorageRegion::MeCentral1,
            GcpRegion::MeCentral2 => GcpStorageRegion::MeCentral2,
            GcpRegion::MeWest1 => GcpStorageRegion::MeWest1,
            GcpRegion::NorthAmericaNortheast1 => GcpStorageRegion::NorthAmericaNortheast1,
            GcpRegion::NorthAmericaNortheast2 => GcpStorageRegion::NorthAmericaNortheast2,
            GcpRegion::SouthAmericaEast1 => GcpStorageRegion::SouthAmericaEast1,
            GcpRegion::SouthAmericaWest1 => GcpStorageRegion::SouthAmericaWest1,
            GcpRegion::UsCentral1 => GcpStorageRegion::UsCentral1,
            GcpRegion::UsEast1 => GcpStorageRegion::UsEast1,
            GcpRegion::UsEast4 => GcpStorageRegion::UsEast4,
            GcpRegion::UsEast5 => GcpStorageRegion::UsEast5,
            GcpRegion::UsSouth1 => GcpStorageRegion::UsSouth1,
            GcpRegion::UsWest1 => GcpStorageRegion::UsWest1,
            GcpRegion::UsWest2 => GcpStorageRegion::UsWest2,
            GcpRegion::UsWest3 => GcpStorageRegion::UsWest3,
            GcpRegion::UsWest4 => GcpStorageRegion::UsWest4,
        }
    }
}

impl StorageRegion for GcpStorageRegion {}

impl ToCloudProviderFormat for GcpStorageRegion {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            GcpStorageRegion::AsiaEast1 => "ASIA-EAST1",
            GcpStorageRegion::AsiaEast2 => "ASIA-EAST2",
            GcpStorageRegion::AsiaNortheast1 => "ASIA-NORTHEAST1",
            GcpStorageRegion::AsiaNortheast2 => "ASIA-NORTHEAST2",
            GcpStorageRegion::AsiaNortheast3 => "ASIA-NORTHEAST3",
            GcpStorageRegion::AsiaSouth1 => "ASIA-SOUTH1",
            GcpStorageRegion::AsiaSouth2 => "ASIA-SOUTH2",
            GcpStorageRegion::AsiaSoutheast1 => "ASIA-SOUTHEAST1",
            GcpStorageRegion::AsiaSoutheast2 => "ASIA-SOUTHEAST2",
            GcpStorageRegion::AustraliaSoutheast1 => "AUSTRALIA-SOUTHEAST1",
            GcpStorageRegion::AustraliaSoutheast2 => "AUSTRALIA-SOUTHEAST2",
            GcpStorageRegion::EuropeCentral2 => "EUROPE-CENTRAL2",
            GcpStorageRegion::EuropeNorth1 => "EUROPE-NORTH1",
            GcpStorageRegion::EuropeSouthwest1 => "EUROPE-SOUTHWEST1",
            GcpStorageRegion::EuropeWest1 => "EUROPE-WEST1",
            GcpStorageRegion::EuropeWest10 => "EUROPE-WEST10",
            GcpStorageRegion::EuropeWest12 => "EUROPE-WEST12",
            GcpStorageRegion::EuropeWest2 => "EUROPE-WEST2",
            GcpStorageRegion::EuropeWest3 => "EUROPE-WEST3",
            GcpStorageRegion::EuropeWest4 => "EUROPE-WEST4",
            GcpStorageRegion::EuropeWest6 => "EUROPE-WEST6",
            GcpStorageRegion::EuropeWest8 => "EUROPE-WEST8",
            GcpStorageRegion::EuropeWest9 => "EUROPE-WEST9",
            GcpStorageRegion::MeCentral1 => "ME-CENTRAL1",
            GcpStorageRegion::MeCentral2 => "ME-CENTRAL2",
            GcpStorageRegion::MeWest1 => "ME-WEST1",
            GcpStorageRegion::NorthAmericaNortheast1 => "NORTHAMERICA-NORTHEAST1",
            GcpStorageRegion::NorthAmericaNortheast2 => "NORTHAMERICA-NORTHEAST2",
            GcpStorageRegion::SouthAmericaEast1 => "SOUTHAMERICA-EAST1",
            GcpStorageRegion::SouthAmericaWest1 => "SOUTHAMERICA-WEST1",
            GcpStorageRegion::UsCentral1 => "US-CENTRAL1",
            GcpStorageRegion::UsEast1 => "US-EAST1",
            GcpStorageRegion::UsEast4 => "US-EAST4",
            GcpStorageRegion::UsEast5 => "US-EAST5",
            GcpStorageRegion::UsSouth1 => "US-SOUTH1",
            GcpStorageRegion::UsWest1 => "US-WEST1",
            GcpStorageRegion::UsWest2 => "US-WEST2",
            GcpStorageRegion::UsWest3 => "US-WEST3",
            GcpStorageRegion::UsWest4 => "US-WEST4",
        }
    }
}

impl Display for GcpStorageRegion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_cloud_provider_format())
    }
}

impl FromStr for GcpStorageRegion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_uppercase().as_str() {
            "ASIA-EAST1" => Ok(GcpStorageRegion::AsiaEast1),
            "ASIA-EAST2" => Ok(GcpStorageRegion::AsiaEast2),
            "ASIA-NORTHEAST1" => Ok(GcpStorageRegion::AsiaNortheast1),
            "ASIA-NORTHEAST2" => Ok(GcpStorageRegion::AsiaNortheast2),
            "ASIA-NORTHEAST3" => Ok(GcpStorageRegion::AsiaNortheast3),
            "ASIA-SOUTH1" => Ok(GcpStorageRegion::AsiaSouth1),
            "ASIA-SOUTH2" => Ok(GcpStorageRegion::AsiaSouth2),
            "ASIA-SOUTHEAST1" => Ok(GcpStorageRegion::AsiaSoutheast1),
            "ASIA-SOUTHEAST2" => Ok(GcpStorageRegion::AsiaSoutheast2),
            "AUSTRALIA-SOUTHEAST1" => Ok(GcpStorageRegion::AustraliaSoutheast1),
            "AUSTRALIA-SOUTHEAST2" => Ok(GcpStorageRegion::AustraliaSoutheast2),
            "EUROPE-CENTRAL2" => Ok(GcpStorageRegion::EuropeCentral2),
            "EUROPE-NORTH1" => Ok(GcpStorageRegion::EuropeNorth1),
            "EUROPE-SOUTHWEST1" => Ok(GcpStorageRegion::EuropeSouthwest1),
            "EUROPE-WEST1" => Ok(GcpStorageRegion::EuropeWest1),
            "EUROPE-WEST10" => Ok(GcpStorageRegion::EuropeWest10),
            "EUROPE-WEST12" => Ok(GcpStorageRegion::EuropeWest12),
            "EUROPE-WEST2" => Ok(GcpStorageRegion::EuropeWest2),
            "EUROPE-WEST3" => Ok(GcpStorageRegion::EuropeWest3),
            "EUROPE-WEST4" => Ok(GcpStorageRegion::EuropeWest4),
            "EUROPE-WEST6" => Ok(GcpStorageRegion::EuropeWest6),
            "EUROPE-WEST8" => Ok(GcpStorageRegion::EuropeWest8),
            "EUROPE-WEST9" => Ok(GcpStorageRegion::EuropeWest9),
            "ME-CENTRAL1" => Ok(GcpStorageRegion::MeCentral1),
            "ME-CENTRAL2" => Ok(GcpStorageRegion::MeCentral2),
            "ME-WEST1" => Ok(GcpStorageRegion::MeWest1),
            "NORTHAMERICA-NORTHEAST1" => Ok(GcpStorageRegion::NorthAmericaNortheast1),
            "NORTHAMERICA-NORTHEAST2" => Ok(GcpStorageRegion::NorthAmericaNortheast2),
            "SOUTHAMERICA-EAST1" => Ok(GcpStorageRegion::SouthAmericaEast1),
            "SOUTHAMERICA-WEST1" => Ok(GcpStorageRegion::SouthAmericaWest1),
            "US-CENTRAL1" => Ok(GcpStorageRegion::UsCentral1),
            "US-EAST1" => Ok(GcpStorageRegion::UsEast1),
            "US-EAST4" => Ok(GcpStorageRegion::UsEast4),
            "US-EAST5" => Ok(GcpStorageRegion::UsEast5),
            "US-SOUTH1" => Ok(GcpStorageRegion::UsSouth1),
            "US-WEST1" => Ok(GcpStorageRegion::UsWest1),
            "US-WEST2" => Ok(GcpStorageRegion::UsWest2),
            "US-WEST3" => Ok(GcpStorageRegion::UsWest3),
            "US-WEST4" => Ok(GcpStorageRegion::UsWest4),
            _ => Err(format!("Unknown storage region: `{}`.", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::ToCloudProviderFormat;
    use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
    use crate::services::gcp::object_storage_regions::GcpStorageRegion;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_gcp_storage_region_to_gcp_format() {
        for region in GcpStorageRegion::iter() {
            assert_eq!(
                match region {
                    GcpStorageRegion::AsiaEast1 => "ASIA-EAST1",
                    GcpStorageRegion::AsiaEast2 => "ASIA-EAST2",
                    GcpStorageRegion::AsiaNortheast1 => "ASIA-NORTHEAST1",
                    GcpStorageRegion::AsiaNortheast2 => "ASIA-NORTHEAST2",
                    GcpStorageRegion::AsiaNortheast3 => "ASIA-NORTHEAST3",
                    GcpStorageRegion::AsiaSouth1 => "ASIA-SOUTH1",
                    GcpStorageRegion::AsiaSouth2 => "ASIA-SOUTH2",
                    GcpStorageRegion::AsiaSoutheast1 => "ASIA-SOUTHEAST1",
                    GcpStorageRegion::AsiaSoutheast2 => "ASIA-SOUTHEAST2",
                    GcpStorageRegion::AustraliaSoutheast1 => "AUSTRALIA-SOUTHEAST1",
                    GcpStorageRegion::AustraliaSoutheast2 => "AUSTRALIA-SOUTHEAST2",
                    GcpStorageRegion::EuropeCentral2 => "EUROPE-CENTRAL2",
                    GcpStorageRegion::EuropeNorth1 => "EUROPE-NORTH1",
                    GcpStorageRegion::EuropeSouthwest1 => "EUROPE-SOUTHWEST1",
                    GcpStorageRegion::EuropeWest1 => "EUROPE-WEST1",
                    GcpStorageRegion::EuropeWest10 => "EUROPE-WEST10",
                    GcpStorageRegion::EuropeWest12 => "EUROPE-WEST12",
                    GcpStorageRegion::EuropeWest2 => "EUROPE-WEST2",
                    GcpStorageRegion::EuropeWest3 => "EUROPE-WEST3",
                    GcpStorageRegion::EuropeWest4 => "EUROPE-WEST4",
                    GcpStorageRegion::EuropeWest6 => "EUROPE-WEST6",
                    GcpStorageRegion::EuropeWest8 => "EUROPE-WEST8",
                    GcpStorageRegion::EuropeWest9 => "EUROPE-WEST9",
                    GcpStorageRegion::MeCentral1 => "ME-CENTRAL1",
                    GcpStorageRegion::MeCentral2 => "ME-CENTRAL2",
                    GcpStorageRegion::MeWest1 => "ME-WEST1",
                    GcpStorageRegion::NorthAmericaNortheast1 => "NORTHAMERICA-NORTHEAST1",
                    GcpStorageRegion::NorthAmericaNortheast2 => "NORTHAMERICA-NORTHEAST2",
                    GcpStorageRegion::SouthAmericaEast1 => "SOUTHAMERICA-EAST1",
                    GcpStorageRegion::SouthAmericaWest1 => "SOUTHAMERICA-WEST1",
                    GcpStorageRegion::UsCentral1 => "US-CENTRAL1",
                    GcpStorageRegion::UsEast1 => "US-EAST1",
                    GcpStorageRegion::UsEast4 => "US-EAST4",
                    GcpStorageRegion::UsEast5 => "US-EAST5",
                    GcpStorageRegion::UsSouth1 => "US-SOUTH1",
                    GcpStorageRegion::UsWest1 => "US-WEST1",
                    GcpStorageRegion::UsWest2 => "US-WEST2",
                    GcpStorageRegion::UsWest3 => "US-WEST3",
                    GcpStorageRegion::UsWest4 => "US-WEST4",
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
                    GcpStorageRegion::AsiaEast1 => "ASIA-EAST1",
                    GcpStorageRegion::AsiaEast2 => "ASIA-EAST2",
                    GcpStorageRegion::AsiaNortheast1 => "ASIA-NORTHEAST1",
                    GcpStorageRegion::AsiaNortheast2 => "ASIA-NORTHEAST2",
                    GcpStorageRegion::AsiaNortheast3 => "ASIA-NORTHEAST3",
                    GcpStorageRegion::AsiaSouth1 => "ASIA-SOUTH1",
                    GcpStorageRegion::AsiaSouth2 => "ASIA-SOUTH2",
                    GcpStorageRegion::AsiaSoutheast1 => "ASIA-SOUTHEAST1",
                    GcpStorageRegion::AsiaSoutheast2 => "ASIA-SOUTHEAST2",
                    GcpStorageRegion::AustraliaSoutheast1 => "AUSTRALIA-SOUTHEAST1",
                    GcpStorageRegion::AustraliaSoutheast2 => "AUSTRALIA-SOUTHEAST2",
                    GcpStorageRegion::EuropeCentral2 => "EUROPE-CENTRAL2",
                    GcpStorageRegion::EuropeNorth1 => "EUROPE-NORTH1",
                    GcpStorageRegion::EuropeSouthwest1 => "EUROPE-SOUTHWEST1",
                    GcpStorageRegion::EuropeWest1 => "EUROPE-WEST1",
                    GcpStorageRegion::EuropeWest10 => "EUROPE-WEST10",
                    GcpStorageRegion::EuropeWest12 => "EUROPE-WEST12",
                    GcpStorageRegion::EuropeWest2 => "EUROPE-WEST2",
                    GcpStorageRegion::EuropeWest3 => "EUROPE-WEST3",
                    GcpStorageRegion::EuropeWest4 => "EUROPE-WEST4",
                    GcpStorageRegion::EuropeWest6 => "EUROPE-WEST6",
                    GcpStorageRegion::EuropeWest8 => "EUROPE-WEST8",
                    GcpStorageRegion::EuropeWest9 => "EUROPE-WEST9",
                    GcpStorageRegion::MeCentral1 => "ME-CENTRAL1",
                    GcpStorageRegion::MeCentral2 => "ME-CENTRAL2",
                    GcpStorageRegion::MeWest1 => "ME-WEST1",
                    GcpStorageRegion::NorthAmericaNortheast1 => "NORTHAMERICA-NORTHEAST1",
                    GcpStorageRegion::NorthAmericaNortheast2 => "NORTHAMERICA-NORTHEAST2",
                    GcpStorageRegion::SouthAmericaEast1 => "SOUTHAMERICA-EAST1",
                    GcpStorageRegion::SouthAmericaWest1 => "SOUTHAMERICA-WEST1",
                    GcpStorageRegion::UsCentral1 => "US-CENTRAL1",
                    GcpStorageRegion::UsEast1 => "US-EAST1",
                    GcpStorageRegion::UsEast4 => "US-EAST4",
                    GcpStorageRegion::UsEast5 => "US-EAST5",
                    GcpStorageRegion::UsSouth1 => "US-SOUTH1",
                    GcpStorageRegion::UsWest1 => "US-WEST1",
                    GcpStorageRegion::UsWest2 => "US-WEST2",
                    GcpStorageRegion::UsWest3 => "US-WEST3",
                    GcpStorageRegion::UsWest4 => "US-WEST4",
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
                    GcpRegion::AsiaEast1 => GcpStorageRegion::AsiaEast1,
                    GcpRegion::AsiaEast2 => GcpStorageRegion::AsiaEast2,
                    GcpRegion::AsiaNortheast1 => GcpStorageRegion::AsiaNortheast1,
                    GcpRegion::AsiaNortheast2 => GcpStorageRegion::AsiaNortheast2,
                    GcpRegion::AsiaNortheast3 => GcpStorageRegion::AsiaNortheast3,
                    GcpRegion::AsiaSouth1 => GcpStorageRegion::AsiaSouth1,
                    GcpRegion::AsiaSouth2 => GcpStorageRegion::AsiaSouth2,
                    GcpRegion::AsiaSoutheast1 => GcpStorageRegion::AsiaSoutheast1,
                    GcpRegion::AsiaSoutheast2 => GcpStorageRegion::AsiaSoutheast2,
                    GcpRegion::AustraliaSoutheast1 => GcpStorageRegion::AustraliaSoutheast1,
                    GcpRegion::AustraliaSoutheast2 => GcpStorageRegion::AustraliaSoutheast2,
                    GcpRegion::EuropeCentral2 => GcpStorageRegion::EuropeCentral2,
                    GcpRegion::EuropeNorth1 => GcpStorageRegion::EuropeNorth1,
                    GcpRegion::EuropeSouthwest1 => GcpStorageRegion::EuropeSouthwest1,
                    GcpRegion::EuropeWest1 => GcpStorageRegion::EuropeWest1,
                    GcpRegion::EuropeWest10 => GcpStorageRegion::EuropeWest10,
                    GcpRegion::EuropeWest12 => GcpStorageRegion::EuropeWest12,
                    GcpRegion::EuropeWest2 => GcpStorageRegion::EuropeWest2,
                    GcpRegion::EuropeWest3 => GcpStorageRegion::EuropeWest3,
                    GcpRegion::EuropeWest4 => GcpStorageRegion::EuropeWest4,
                    GcpRegion::EuropeWest6 => GcpStorageRegion::EuropeWest6,
                    GcpRegion::EuropeWest8 => GcpStorageRegion::EuropeWest8,
                    GcpRegion::EuropeWest9 => GcpStorageRegion::EuropeWest9,
                    GcpRegion::MeCentral1 => GcpStorageRegion::MeCentral1,
                    GcpRegion::MeCentral2 => GcpStorageRegion::MeCentral2,
                    GcpRegion::MeWest1 => GcpStorageRegion::MeWest1,
                    GcpRegion::NorthAmericaNortheast1 => GcpStorageRegion::NorthAmericaNortheast1,
                    GcpRegion::NorthAmericaNortheast2 => GcpStorageRegion::NorthAmericaNortheast2,
                    GcpRegion::SouthAmericaEast1 => GcpStorageRegion::SouthAmericaEast1,
                    GcpRegion::SouthAmericaWest1 => GcpStorageRegion::SouthAmericaWest1,
                    GcpRegion::UsCentral1 => GcpStorageRegion::UsCentral1,
                    GcpRegion::UsEast1 => GcpStorageRegion::UsEast1,
                    GcpRegion::UsEast4 => GcpStorageRegion::UsEast4,
                    GcpRegion::UsEast5 => GcpStorageRegion::UsEast5,
                    GcpRegion::UsSouth1 => GcpStorageRegion::UsSouth1,
                    GcpRegion::UsWest1 => GcpStorageRegion::UsWest1,
                    GcpRegion::UsWest2 => GcpStorageRegion::UsWest2,
                    GcpRegion::UsWest3 => GcpStorageRegion::UsWest3,
                    GcpRegion::UsWest4 => GcpStorageRegion::UsWest4,
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
                input: "ASIA-EAST1",
                expected: GcpStorageRegion::AsiaEast1,
            },
            TestCase {
                input: " ASIA-EAST1  ",
                expected: GcpStorageRegion::AsiaEast1,
            },
            TestCase {
                input: "asia-east1",
                expected: GcpStorageRegion::AsiaEast1,
            },
            TestCase {
                input: " asia-east1",
                expected: GcpStorageRegion::AsiaEast1,
            },
            TestCase {
                input: "ASIA-EAST2",
                expected: GcpStorageRegion::AsiaEast2,
            },
            TestCase {
                input: " ASIA-EAST2  ",
                expected: GcpStorageRegion::AsiaEast2,
            },
            TestCase {
                input: "asia-east2",
                expected: GcpStorageRegion::AsiaEast2,
            },
            TestCase {
                input: " asia-east2",
                expected: GcpStorageRegion::AsiaEast2,
            },
            TestCase {
                input: "ASIA-NORTHEAST1",
                expected: GcpStorageRegion::AsiaNortheast1,
            },
            TestCase {
                input: " ASIA-NORTHEAST1  ",
                expected: GcpStorageRegion::AsiaNortheast1,
            },
            TestCase {
                input: "asia-northeast1",
                expected: GcpStorageRegion::AsiaNortheast1,
            },
            TestCase {
                input: " asia-northeast1",
                expected: GcpStorageRegion::AsiaNortheast1,
            },
            TestCase {
                input: "ASIA-NORTHEAST2",
                expected: GcpStorageRegion::AsiaNortheast2,
            },
            TestCase {
                input: " ASIA-NORTHEAST2  ",
                expected: GcpStorageRegion::AsiaNortheast2,
            },
            TestCase {
                input: "asia-northeast2",
                expected: GcpStorageRegion::AsiaNortheast2,
            },
            TestCase {
                input: " asia-northeast2",
                expected: GcpStorageRegion::AsiaNortheast2,
            },
            TestCase {
                input: "ASIA-NORTHEAST3",
                expected: GcpStorageRegion::AsiaNortheast3,
            },
            TestCase {
                input: " ASIA-NORTHEAST3  ",
                expected: GcpStorageRegion::AsiaNortheast3,
            },
            TestCase {
                input: "asia-northeast3",
                expected: GcpStorageRegion::AsiaNortheast3,
            },
            TestCase {
                input: " asia-northeast3",
                expected: GcpStorageRegion::AsiaNortheast3,
            },
            TestCase {
                input: "ASIA-SOUTH1",
                expected: GcpStorageRegion::AsiaSouth1,
            },
            TestCase {
                input: " ASIA-SOUTH1  ",
                expected: GcpStorageRegion::AsiaSouth1,
            },
            TestCase {
                input: "asia-south1",
                expected: GcpStorageRegion::AsiaSouth1,
            },
            TestCase {
                input: " asia-south1",
                expected: GcpStorageRegion::AsiaSouth1,
            },
            TestCase {
                input: "ASIA-SOUTH2",
                expected: GcpStorageRegion::AsiaSouth2,
            },
            TestCase {
                input: " ASIA-SOUTH2  ",
                expected: GcpStorageRegion::AsiaSouth2,
            },
            TestCase {
                input: "asia-south2",
                expected: GcpStorageRegion::AsiaSouth2,
            },
            TestCase {
                input: " asia-south2",
                expected: GcpStorageRegion::AsiaSouth2,
            },
            TestCase {
                input: "ASIA-SOUTHEAST1",
                expected: GcpStorageRegion::AsiaSoutheast1,
            },
            TestCase {
                input: " ASIA-SOUTHEAST1  ",
                expected: GcpStorageRegion::AsiaSoutheast1,
            },
            TestCase {
                input: "asia-southeast1",
                expected: GcpStorageRegion::AsiaSoutheast1,
            },
            TestCase {
                input: " asia-southeast1",
                expected: GcpStorageRegion::AsiaSoutheast1,
            },
            TestCase {
                input: "ASIA-SOUTHEAST2",
                expected: GcpStorageRegion::AsiaSoutheast2,
            },
            TestCase {
                input: " ASIA-SOUTHEAST2  ",
                expected: GcpStorageRegion::AsiaSoutheast2,
            },
            TestCase {
                input: "asia-southeast2",
                expected: GcpStorageRegion::AsiaSoutheast2,
            },
            TestCase {
                input: " asia-southeast2",
                expected: GcpStorageRegion::AsiaSoutheast2,
            },
            TestCase {
                input: "AUSTRALIA-SOUTHEAST1",
                expected: GcpStorageRegion::AustraliaSoutheast1,
            },
            TestCase {
                input: " AUSTRALIA-SOUTHEAST1  ",
                expected: GcpStorageRegion::AustraliaSoutheast1,
            },
            TestCase {
                input: "australia-southeast1",
                expected: GcpStorageRegion::AustraliaSoutheast1,
            },
            TestCase {
                input: " australia-southeast1",
                expected: GcpStorageRegion::AustraliaSoutheast1,
            },
            TestCase {
                input: "AUSTRALIA-SOUTHEAST2",
                expected: GcpStorageRegion::AustraliaSoutheast2,
            },
            TestCase {
                input: " AUSTRALIA-SOUTHEAST2  ",
                expected: GcpStorageRegion::AustraliaSoutheast2,
            },
            TestCase {
                input: "australia-southeast2",
                expected: GcpStorageRegion::AustraliaSoutheast2,
            },
            TestCase {
                input: " australia-southeast2",
                expected: GcpStorageRegion::AustraliaSoutheast2,
            },
            TestCase {
                input: "EUROPE-CENTRAL2",
                expected: GcpStorageRegion::EuropeCentral2,
            },
            TestCase {
                input: " EUROPE-CENTRAL2  ",
                expected: GcpStorageRegion::EuropeCentral2,
            },
            TestCase {
                input: "europe-central2",
                expected: GcpStorageRegion::EuropeCentral2,
            },
            TestCase {
                input: " europe-central2",
                expected: GcpStorageRegion::EuropeCentral2,
            },
            TestCase {
                input: "EUROPE-NORTH1",
                expected: GcpStorageRegion::EuropeNorth1,
            },
            TestCase {
                input: " EUROPE-NORTH1  ",
                expected: GcpStorageRegion::EuropeNorth1,
            },
            TestCase {
                input: "europe-north1",
                expected: GcpStorageRegion::EuropeNorth1,
            },
            TestCase {
                input: " europe-north1",
                expected: GcpStorageRegion::EuropeNorth1,
            },
            TestCase {
                input: "EUROPE-SOUTHWEST1",
                expected: GcpStorageRegion::EuropeSouthwest1,
            },
            TestCase {
                input: " EUROPE-SOUTHWEST1  ",
                expected: GcpStorageRegion::EuropeSouthwest1,
            },
            TestCase {
                input: "europe-southwest1",
                expected: GcpStorageRegion::EuropeSouthwest1,
            },
            TestCase {
                input: " europe-southwest1",
                expected: GcpStorageRegion::EuropeSouthwest1,
            },
            TestCase {
                input: "EUROPE-WEST1",
                expected: GcpStorageRegion::EuropeWest1,
            },
            TestCase {
                input: " EUROPE-WEST1  ",
                expected: GcpStorageRegion::EuropeWest1,
            },
            TestCase {
                input: "europe-west1",
                expected: GcpStorageRegion::EuropeWest1,
            },
            TestCase {
                input: " europe-west1",
                expected: GcpStorageRegion::EuropeWest1,
            },
            TestCase {
                input: "EUROPE-WEST10",
                expected: GcpStorageRegion::EuropeWest10,
            },
            TestCase {
                input: " EUROPE-WEST10  ",
                expected: GcpStorageRegion::EuropeWest10,
            },
            TestCase {
                input: "europe-west10",
                expected: GcpStorageRegion::EuropeWest10,
            },
            TestCase {
                input: " europe-west10",
                expected: GcpStorageRegion::EuropeWest10,
            },
            TestCase {
                input: "EUROPE-WEST12",
                expected: GcpStorageRegion::EuropeWest12,
            },
            TestCase {
                input: " EUROPE-WEST12  ",
                expected: GcpStorageRegion::EuropeWest12,
            },
            TestCase {
                input: "europe-west12",
                expected: GcpStorageRegion::EuropeWest12,
            },
            TestCase {
                input: " europe-west12",
                expected: GcpStorageRegion::EuropeWest12,
            },
            TestCase {
                input: "EUROPE-WEST2",
                expected: GcpStorageRegion::EuropeWest2,
            },
            TestCase {
                input: " EUROPE-WEST2  ",
                expected: GcpStorageRegion::EuropeWest2,
            },
            TestCase {
                input: "europe-west2",
                expected: GcpStorageRegion::EuropeWest2,
            },
            TestCase {
                input: " europe-west2",
                expected: GcpStorageRegion::EuropeWest2,
            },
            TestCase {
                input: "EUROPE-WEST3",
                expected: GcpStorageRegion::EuropeWest3,
            },
            TestCase {
                input: " EUROPE-WEST3  ",
                expected: GcpStorageRegion::EuropeWest3,
            },
            TestCase {
                input: "europe-west3",
                expected: GcpStorageRegion::EuropeWest3,
            },
            TestCase {
                input: " europe-west3",
                expected: GcpStorageRegion::EuropeWest3,
            },
            TestCase {
                input: "EUROPE-WEST4",
                expected: GcpStorageRegion::EuropeWest4,
            },
            TestCase {
                input: " EUROPE-WEST4  ",
                expected: GcpStorageRegion::EuropeWest4,
            },
            TestCase {
                input: "europe-west4",
                expected: GcpStorageRegion::EuropeWest4,
            },
            TestCase {
                input: " europe-west4",
                expected: GcpStorageRegion::EuropeWest4,
            },
            TestCase {
                input: "EUROPE-WEST6",
                expected: GcpStorageRegion::EuropeWest6,
            },
            TestCase {
                input: " EUROPE-WEST6  ",
                expected: GcpStorageRegion::EuropeWest6,
            },
            TestCase {
                input: "europe-west6",
                expected: GcpStorageRegion::EuropeWest6,
            },
            TestCase {
                input: " europe-west6",
                expected: GcpStorageRegion::EuropeWest6,
            },
            TestCase {
                input: "EUROPE-WEST8",
                expected: GcpStorageRegion::EuropeWest8,
            },
            TestCase {
                input: " EUROPE-WEST8  ",
                expected: GcpStorageRegion::EuropeWest8,
            },
            TestCase {
                input: "europe-west8",
                expected: GcpStorageRegion::EuropeWest8,
            },
            TestCase {
                input: " europe-west8",
                expected: GcpStorageRegion::EuropeWest8,
            },
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
            TestCase {
                input: "ME-CENTRAL1",
                expected: GcpStorageRegion::MeCentral1,
            },
            TestCase {
                input: " ME-CENTRAL1  ",
                expected: GcpStorageRegion::MeCentral1,
            },
            TestCase {
                input: "me-central1",
                expected: GcpStorageRegion::MeCentral1,
            },
            TestCase {
                input: " me-central1",
                expected: GcpStorageRegion::MeCentral1,
            },
            TestCase {
                input: "ME-CENTRAL2",
                expected: GcpStorageRegion::MeCentral2,
            },
            TestCase {
                input: " ME-CENTRAL2  ",
                expected: GcpStorageRegion::MeCentral2,
            },
            TestCase {
                input: "me-central2",
                expected: GcpStorageRegion::MeCentral2,
            },
            TestCase {
                input: " me-central2",
                expected: GcpStorageRegion::MeCentral2,
            },
            TestCase {
                input: "ME-WEST1",
                expected: GcpStorageRegion::MeWest1,
            },
            TestCase {
                input: " ME-WEST1  ",
                expected: GcpStorageRegion::MeWest1,
            },
            TestCase {
                input: "me-west1",
                expected: GcpStorageRegion::MeWest1,
            },
            TestCase {
                input: " me-west1",
                expected: GcpStorageRegion::MeWest1,
            },
            TestCase {
                input: "NORTHAMERICA-NORTHEAST1",
                expected: GcpStorageRegion::NorthAmericaNortheast1,
            },
            TestCase {
                input: " NORTHAMERICA-NORTHEAST1  ",
                expected: GcpStorageRegion::NorthAmericaNortheast1,
            },
            TestCase {
                input: "northamerica-northeast1",
                expected: GcpStorageRegion::NorthAmericaNortheast1,
            },
            TestCase {
                input: " northamerica-northeast1",
                expected: GcpStorageRegion::NorthAmericaNortheast1,
            },
            TestCase {
                input: "NORTHAMERICA-NORTHEAST2",
                expected: GcpStorageRegion::NorthAmericaNortheast2,
            },
            TestCase {
                input: " NORTHAMERICA-NORTHEAST2  ",
                expected: GcpStorageRegion::NorthAmericaNortheast2,
            },
            TestCase {
                input: "northamerica-northeast2",
                expected: GcpStorageRegion::NorthAmericaNortheast2,
            },
            TestCase {
                input: " northamerica-northeast2",
                expected: GcpStorageRegion::NorthAmericaNortheast2,
            },
            TestCase {
                input: "SOUTHAMERICA-EAST1",
                expected: GcpStorageRegion::SouthAmericaEast1,
            },
            TestCase {
                input: " SOUTHAMERICA-EAST1  ",
                expected: GcpStorageRegion::SouthAmericaEast1,
            },
            TestCase {
                input: "southamerica-east1",
                expected: GcpStorageRegion::SouthAmericaEast1,
            },
            TestCase {
                input: " southamerica-east1",
                expected: GcpStorageRegion::SouthAmericaEast1,
            },
            TestCase {
                input: "SOUTHAMERICA-WEST1",
                expected: GcpStorageRegion::SouthAmericaWest1,
            },
            TestCase {
                input: " SOUTHAMERICA-WEST1  ",
                expected: GcpStorageRegion::SouthAmericaWest1,
            },
            TestCase {
                input: "southamerica-west1",
                expected: GcpStorageRegion::SouthAmericaWest1,
            },
            TestCase {
                input: " southamerica-west1",
                expected: GcpStorageRegion::SouthAmericaWest1,
            },
            TestCase {
                input: "US-CENTRAL1",
                expected: GcpStorageRegion::UsCentral1,
            },
            TestCase {
                input: " US-CENTRAL1  ",
                expected: GcpStorageRegion::UsCentral1,
            },
            TestCase {
                input: "us-central1",
                expected: GcpStorageRegion::UsCentral1,
            },
            TestCase {
                input: " us-central1",
                expected: GcpStorageRegion::UsCentral1,
            },
            TestCase {
                input: "US-EAST1",
                expected: GcpStorageRegion::UsEast1,
            },
            TestCase {
                input: " US-EAST1  ",
                expected: GcpStorageRegion::UsEast1,
            },
            TestCase {
                input: "us-east1",
                expected: GcpStorageRegion::UsEast1,
            },
            TestCase {
                input: " us-east1",
                expected: GcpStorageRegion::UsEast1,
            },
            TestCase {
                input: "US-EAST4",
                expected: GcpStorageRegion::UsEast4,
            },
            TestCase {
                input: " US-EAST4  ",
                expected: GcpStorageRegion::UsEast4,
            },
            TestCase {
                input: "us-east4",
                expected: GcpStorageRegion::UsEast4,
            },
            TestCase {
                input: " us-east4",
                expected: GcpStorageRegion::UsEast4,
            },
            TestCase {
                input: "US-EAST5",
                expected: GcpStorageRegion::UsEast5,
            },
            TestCase {
                input: " US-EAST5  ",
                expected: GcpStorageRegion::UsEast5,
            },
            TestCase {
                input: "us-east5",
                expected: GcpStorageRegion::UsEast5,
            },
            TestCase {
                input: " us-east5",
                expected: GcpStorageRegion::UsEast5,
            },
            TestCase {
                input: "US-SOUTH1",
                expected: GcpStorageRegion::UsSouth1,
            },
            TestCase {
                input: " US-SOUTH1  ",
                expected: GcpStorageRegion::UsSouth1,
            },
            TestCase {
                input: "us-south1",
                expected: GcpStorageRegion::UsSouth1,
            },
            TestCase {
                input: " us-south1",
                expected: GcpStorageRegion::UsSouth1,
            },
            TestCase {
                input: "US-WEST1",
                expected: GcpStorageRegion::UsWest1,
            },
            TestCase {
                input: " US-WEST1  ",
                expected: GcpStorageRegion::UsWest1,
            },
            TestCase {
                input: "us-west1",
                expected: GcpStorageRegion::UsWest1,
            },
            TestCase {
                input: " us-west1",
                expected: GcpStorageRegion::UsWest1,
            },
            TestCase {
                input: "US-WEST2",
                expected: GcpStorageRegion::UsWest2,
            },
            TestCase {
                input: " US-WEST2  ",
                expected: GcpStorageRegion::UsWest2,
            },
            TestCase {
                input: "us-west2",
                expected: GcpStorageRegion::UsWest2,
            },
            TestCase {
                input: " us-west2",
                expected: GcpStorageRegion::UsWest2,
            },
            TestCase {
                input: "US-WEST3",
                expected: GcpStorageRegion::UsWest3,
            },
            TestCase {
                input: " US-WEST3  ",
                expected: GcpStorageRegion::UsWest3,
            },
            TestCase {
                input: "us-west3",
                expected: GcpStorageRegion::UsWest3,
            },
            TestCase {
                input: " us-west3",
                expected: GcpStorageRegion::UsWest3,
            },
            TestCase {
                input: "US-WEST4",
                expected: GcpStorageRegion::UsWest4,
            },
            TestCase {
                input: " US-WEST4  ",
                expected: GcpStorageRegion::UsWest4,
            },
            TestCase {
                input: "us-west4",
                expected: GcpStorageRegion::UsWest4,
            },
            TestCase {
                input: " us-west4",
                expected: GcpStorageRegion::UsWest4,
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
