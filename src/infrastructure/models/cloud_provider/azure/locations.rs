use crate::environment::models::ToCloudProviderFormat;
use serde_derive::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter, Hash)]
pub enum AzureZone {
    #[serde(rename = "1")]
    One,
    #[serde(rename = "2")]
    Two,
    #[serde(rename = "3")]
    Three,
}

impl ToCloudProviderFormat for AzureZone {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            AzureZone::One => "1",
            AzureZone::Two => "2",
            AzureZone::Three => "3",
        }
    }
}

impl Display for AzureZone {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_cloud_provider_format())
    }
}

impl FromStr for AzureZone {
    type Err = ();

    fn from_str(s: &str) -> Result<AzureZone, ()> {
        let v: &str = &s.to_lowercase();
        match v {
            "1" => Ok(AzureZone::One),
            "2" => Ok(AzureZone::Two),
            "3" => Ok(AzureZone::Three),
            _ => Err(()),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter)]
#[serde(rename_all = "lowercase")]
pub enum AzureLocation {
    #[serde(alias = "AustraliaCentral")]
    AustraliaCentral,
    #[serde(alias = "AustraliaCentral2")]
    AustraliaCentral2,
    #[serde(alias = "AustraliaEast")]
    AustraliaEast,
    #[serde(alias = "AustraliaSoutheast")]
    AustraliaSoutheast,
    #[serde(alias = "AustriaEast")]
    AustriaEast,
    #[serde(alias = "BrazilSouth")]
    BrazilSouth,
    #[serde(alias = "BrazilSoutheast")]
    BrazilSoutheast,
    #[serde(alias = "CanadaCentral")]
    CanadaCentral,
    #[serde(alias = "CanadaEast")]
    CanadaEast,
    #[serde(alias = "CentralIndia")]
    CentralIndia,
    #[serde(alias = "CentralUS")]
    CentralUS,
    #[serde(alias = "EastAsia")]
    EastAsia,
    #[serde(alias = "EastUS")]
    EastUS,
    #[serde(alias = "EastUS2")]
    EastUS2,
    #[serde(alias = "FranceCentral")]
    FranceCentral,
    #[serde(alias = "FranceSouth")]
    FranceSouth,
    #[serde(alias = "GermanyNorth")]
    GermanyNorth,
    #[serde(alias = "GermanyWestCentral")]
    GermanyWestCentral,
    #[serde(alias = "IndonesiaCentral")]
    IndonesiaCentral,
    #[serde(alias = "IsraelCentral")]
    IsraelCentral,
    #[serde(alias = "ItalyNorth")]
    ItalyNorth,
    #[serde(alias = "JapanEast")]
    JapanEast,
    #[serde(alias = "JapanWest")]
    JapanWest,
    #[serde(alias = "KoreaCentral")]
    KoreaCentral,
    #[serde(alias = "KoreaSouth")]
    KoreaSouth,
    #[serde(alias = "MexicoCentral")]
    MexicoCentral,
    #[serde(alias = "NewZealandNorth")]
    NewZealandNorth,
    #[serde(alias = "NorthCentralUS")]
    NorthCentralUS,
    #[serde(alias = "NorthEurope")]
    NorthEurope,
    #[serde(alias = "NorwayEast")]
    NorwayEast,
    #[serde(alias = "NorwayWest")]
    NorwayWest,
    #[serde(alias = "PolandCentral")]
    PolandCentral,
    #[serde(alias = "QatarCentral")]
    QatarCentral,
    #[serde(alias = "SouthAfricaNorth")]
    SouthAfricaNorth,
    #[serde(alias = "SouthAfricaWest")]
    SouthAfricaWest,
    #[serde(alias = "SouthCentralUS")]
    SouthCentralUS,
    #[serde(alias = "SouthIndia")]
    SouthIndia,
    #[serde(alias = "SoutheastAsia")]
    SoutheastAsia,
    #[serde(alias = "SpainCentral")]
    SpainCentral,
    #[serde(alias = "SwedenCentral")]
    SwedenCentral,
    #[serde(alias = "SwedenSouth")]
    SwedenSouth,
    #[serde(alias = "SwitzerlandNorth")]
    SwitzerlandNorth,
    #[serde(alias = "SwitzerlandWest")]
    SwitzerlandWest,
    #[serde(alias = "UAECentral")]
    UAECentral,
    #[serde(alias = "UAENorth")]
    UAENorth,
    #[serde(alias = "UKSouth")]
    UKSouth,
    #[serde(alias = "UKWest")]
    UKWest,
    #[serde(alias = "WestCentralUS")]
    WestCentralUS,
    #[serde(alias = "WestEurope")]
    WestEurope,
    #[serde(alias = "WestIndia")]
    WestIndia,
    #[serde(alias = "WestUS")]
    WestUS,
    #[serde(alias = "WestUS2")]
    WestUS2,
    #[serde(alias = "WestUS3")]
    WestUS3,
}

impl AzureLocation {
    pub fn zones(&self) -> Vec<AzureZone> {
        // TODO(benjaminch): Azure integration, make sure to check if the location supports zones
        vec![AzureZone::One, AzureZone::Two, AzureZone::Three]
    }
}

impl ToCloudProviderFormat for AzureLocation {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            AzureLocation::AustraliaCentral => "australiacentral",
            AzureLocation::AustraliaCentral2 => "australiacentral2",
            AzureLocation::AustraliaEast => "australiaeast",
            AzureLocation::AustraliaSoutheast => "australiasoutheast",
            AzureLocation::AustriaEast => "austriaeast",
            AzureLocation::BrazilSouth => "brazilsouth",
            AzureLocation::BrazilSoutheast => "brazilsoutheast",
            AzureLocation::CanadaCentral => "canadacentral",
            AzureLocation::CanadaEast => "canadaeast",
            AzureLocation::CentralIndia => "centralindia",
            AzureLocation::CentralUS => "centralus",
            AzureLocation::EastAsia => "eastasia",
            AzureLocation::EastUS => "eastus",
            AzureLocation::EastUS2 => "eastus2",
            AzureLocation::FranceCentral => "francecentral",
            AzureLocation::FranceSouth => "francesouth",
            AzureLocation::GermanyNorth => "germanynorth",
            AzureLocation::GermanyWestCentral => "germanywestcentral",
            AzureLocation::IndonesiaCentral => "indonesiacentral",
            AzureLocation::IsraelCentral => "israelcentral",
            AzureLocation::ItalyNorth => "italynorth",
            AzureLocation::JapanEast => "japaneast",
            AzureLocation::JapanWest => "japanwest",
            AzureLocation::KoreaCentral => "koreacentral",
            AzureLocation::KoreaSouth => "koreasouth",
            AzureLocation::MexicoCentral => "mexicocentral",
            AzureLocation::NewZealandNorth => "newzealandnorth",
            AzureLocation::NorthCentralUS => "northcentralus",
            AzureLocation::NorthEurope => "northeurope",
            AzureLocation::NorwayEast => "norwayeast",
            AzureLocation::NorwayWest => "norwaywest",
            AzureLocation::PolandCentral => "polandcentral",
            AzureLocation::QatarCentral => "qatarcentral",
            AzureLocation::SouthAfricaNorth => "southafricanorth",
            AzureLocation::SouthAfricaWest => "southafricawest",
            AzureLocation::SouthCentralUS => "southcentralus",
            AzureLocation::SouthIndia => "southindia",
            AzureLocation::SoutheastAsia => "southeastasia",
            AzureLocation::SpainCentral => "spaincentral",
            AzureLocation::SwedenCentral => "swedencentral",
            AzureLocation::SwedenSouth => "swedensouth",
            AzureLocation::SwitzerlandNorth => "switzerlandnorth",
            AzureLocation::SwitzerlandWest => "switzerlandwest",
            AzureLocation::UAECentral => "uaecentral",
            AzureLocation::UAENorth => "uaenorth",
            AzureLocation::UKSouth => "uksouth",
            AzureLocation::UKWest => "ukwest",
            AzureLocation::WestCentralUS => "westcentralus",
            AzureLocation::WestEurope => "westeurope",
            AzureLocation::WestIndia => "westindia",
            AzureLocation::WestUS => "westus",
            AzureLocation::WestUS2 => "westus2",
            AzureLocation::WestUS3 => "westus3",
        }
    }
}

impl Display for AzureLocation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_cloud_provider_format())
    }
}

impl FromStr for AzureLocation {
    type Err = ();

    fn from_str(s: &str) -> Result<AzureLocation, ()> {
        let v: &str = &s.to_lowercase();
        match v {
            "australiacentral" => Ok(AzureLocation::AustraliaCentral),
            "australiacentral2" => Ok(AzureLocation::AustraliaCentral2),
            "australiaeast" => Ok(AzureLocation::AustraliaEast),
            "australiasoutheast" => Ok(AzureLocation::AustraliaSoutheast),
            "austriaeast" => Ok(AzureLocation::AustriaEast),
            "brazilsouth" => Ok(AzureLocation::BrazilSouth),
            "brazilsoutheast" => Ok(AzureLocation::BrazilSoutheast),
            "canadacentral" => Ok(AzureLocation::CanadaCentral),
            "canadaeast" => Ok(AzureLocation::CanadaEast),
            "centralindia" => Ok(AzureLocation::CentralIndia),
            "centralus" => Ok(AzureLocation::CentralUS),
            "eastasia" => Ok(AzureLocation::EastAsia),
            "eastus" => Ok(AzureLocation::EastUS),
            "eastus2" => Ok(AzureLocation::EastUS2),
            "francecentral" => Ok(AzureLocation::FranceCentral),
            "francesouth" => Ok(AzureLocation::FranceSouth),
            "germanynorth" => Ok(AzureLocation::GermanyNorth),
            "germanywestcentral" => Ok(AzureLocation::GermanyWestCentral),
            "indonesiacentral" => Ok(AzureLocation::IndonesiaCentral),
            "israelcentral" => Ok(AzureLocation::IsraelCentral),
            "italynorth" => Ok(AzureLocation::ItalyNorth),
            "japaneast" => Ok(AzureLocation::JapanEast),
            "japanwest" => Ok(AzureLocation::JapanWest),
            "koreacentral" => Ok(AzureLocation::KoreaCentral),
            "koreasouth" => Ok(AzureLocation::KoreaSouth),
            "mexicocentral" => Ok(AzureLocation::MexicoCentral),
            "newzealandnorth" => Ok(AzureLocation::NewZealandNorth),
            "northcentralus" => Ok(AzureLocation::NorthCentralUS),
            "northeurope" => Ok(AzureLocation::NorthEurope),
            "norwayeast" => Ok(AzureLocation::NorwayEast),
            "norwaywest" => Ok(AzureLocation::NorwayWest),
            "polandcentral" => Ok(AzureLocation::PolandCentral),
            "qatarcentral" => Ok(AzureLocation::QatarCentral),
            "southafricanorth" => Ok(AzureLocation::SouthAfricaNorth),
            "southafricawest" => Ok(AzureLocation::SouthAfricaWest),
            "southcentralus" => Ok(AzureLocation::SouthCentralUS),
            "southindia" => Ok(AzureLocation::SouthIndia),
            "southeastasia" => Ok(AzureLocation::SoutheastAsia),
            "spaincentral" => Ok(AzureLocation::SpainCentral),
            "swedencentral" => Ok(AzureLocation::SwedenCentral),
            "swedensouth" => Ok(AzureLocation::SwedenSouth),
            "switzerlandnorth" => Ok(AzureLocation::SwitzerlandNorth),
            "switzerlandwest" => Ok(AzureLocation::SwitzerlandWest),
            "uaecentral" => Ok(AzureLocation::UAECentral),
            "uaenorth" => Ok(AzureLocation::UAENorth),
            "uksouth" => Ok(AzureLocation::UKSouth),
            "ukwest" => Ok(AzureLocation::UKWest),
            "westcentralus" => Ok(AzureLocation::WestCentralUS),
            "westeurope" => Ok(AzureLocation::WestEurope),
            "westindia" => Ok(AzureLocation::WestIndia),
            "westus" => Ok(AzureLocation::WestUS),
            "westus2" => Ok(AzureLocation::WestUS2),
            "westus3" => Ok(AzureLocation::WestUS3),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::ToCloudProviderFormat;
    use crate::infrastructure::models::cloud_provider::azure::locations::{AzureLocation, AzureZone};
    use std::collections::HashSet;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_azure_location_to_azure_format() {
        for location in AzureLocation::iter() {
            assert_eq!(
                match location {
                    AzureLocation::AustraliaCentral => "australiacentral",
                    AzureLocation::AustraliaCentral2 => "australiacentral2",
                    AzureLocation::AustraliaEast => "australiaeast",
                    AzureLocation::AustraliaSoutheast => "australiasoutheast",
                    AzureLocation::AustriaEast => "austriaeast",
                    AzureLocation::BrazilSouth => "brazilsouth",
                    AzureLocation::BrazilSoutheast => "brazilsoutheast",
                    AzureLocation::CanadaCentral => "canadacentral",
                    AzureLocation::CanadaEast => "canadaeast",
                    AzureLocation::CentralIndia => "centralindia",
                    AzureLocation::CentralUS => "centralus",
                    AzureLocation::EastAsia => "eastasia",
                    AzureLocation::EastUS => "eastus",
                    AzureLocation::EastUS2 => "eastus2",
                    AzureLocation::FranceCentral => "francecentral",
                    AzureLocation::FranceSouth => "francesouth",
                    AzureLocation::GermanyNorth => "germanynorth",
                    AzureLocation::GermanyWestCentral => "germanywestcentral",
                    AzureLocation::IndonesiaCentral => "indonesiacentral",
                    AzureLocation::IsraelCentral => "israelcentral",
                    AzureLocation::ItalyNorth => "italynorth",
                    AzureLocation::JapanEast => "japaneast",
                    AzureLocation::JapanWest => "japanwest",
                    AzureLocation::KoreaCentral => "koreacentral",
                    AzureLocation::KoreaSouth => "koreasouth",
                    AzureLocation::MexicoCentral => "mexicocentral",
                    AzureLocation::NewZealandNorth => "newzealandnorth",
                    AzureLocation::NorthCentralUS => "northcentralus",
                    AzureLocation::NorthEurope => "northeurope",
                    AzureLocation::NorwayEast => "norwayeast",
                    AzureLocation::NorwayWest => "norwaywest",
                    AzureLocation::PolandCentral => "polandcentral",
                    AzureLocation::QatarCentral => "qatarcentral",
                    AzureLocation::SouthAfricaNorth => "southafricanorth",
                    AzureLocation::SouthAfricaWest => "southafricawest",
                    AzureLocation::SouthCentralUS => "southcentralus",
                    AzureLocation::SouthIndia => "southindia",
                    AzureLocation::SoutheastAsia => "southeastasia",
                    AzureLocation::SpainCentral => "spaincentral",
                    AzureLocation::SwedenCentral => "swedencentral",
                    AzureLocation::SwedenSouth => "swedensouth",
                    AzureLocation::SwitzerlandNorth => "switzerlandnorth",
                    AzureLocation::SwitzerlandWest => "switzerlandwest",
                    AzureLocation::UAECentral => "uaecentral",
                    AzureLocation::UAENorth => "uaenorth",
                    AzureLocation::UKSouth => "uksouth",
                    AzureLocation::UKWest => "ukwest",
                    AzureLocation::WestCentralUS => "westcentralus",
                    AzureLocation::WestEurope => "westeurope",
                    AzureLocation::WestIndia => "westindia",
                    AzureLocation::WestUS => "westus",
                    AzureLocation::WestUS2 => "westus2",
                    AzureLocation::WestUS3 => "westus3",
                },
                location.to_cloud_provider_format()
            );
        }
    }

    #[test]
    fn test_azure_location_to_string() {
        for location in AzureLocation::iter() {
            assert_eq!(
                match location {
                    AzureLocation::AustraliaCentral => "australiacentral",
                    AzureLocation::AustraliaCentral2 => "australiacentral2",
                    AzureLocation::AustraliaEast => "australiaeast",
                    AzureLocation::AustraliaSoutheast => "australiasoutheast",
                    AzureLocation::AustriaEast => "austriaeast",
                    AzureLocation::BrazilSouth => "brazilsouth",
                    AzureLocation::BrazilSoutheast => "brazilsoutheast",
                    AzureLocation::CanadaCentral => "canadacentral",
                    AzureLocation::CanadaEast => "canadaeast",
                    AzureLocation::CentralIndia => "centralindia",
                    AzureLocation::CentralUS => "centralus",
                    AzureLocation::EastAsia => "eastasia",
                    AzureLocation::EastUS => "eastus",
                    AzureLocation::EastUS2 => "eastus2",
                    AzureLocation::FranceCentral => "francecentral",
                    AzureLocation::FranceSouth => "francesouth",
                    AzureLocation::GermanyNorth => "germanynorth",
                    AzureLocation::GermanyWestCentral => "germanywestcentral",
                    AzureLocation::IndonesiaCentral => "indonesiacentral",
                    AzureLocation::IsraelCentral => "israelcentral",
                    AzureLocation::ItalyNorth => "italynorth",
                    AzureLocation::JapanEast => "japaneast",
                    AzureLocation::JapanWest => "japanwest",
                    AzureLocation::KoreaCentral => "koreacentral",
                    AzureLocation::KoreaSouth => "koreasouth",
                    AzureLocation::MexicoCentral => "mexicocentral",
                    AzureLocation::NewZealandNorth => "newzealandnorth",
                    AzureLocation::NorthCentralUS => "northcentralus",
                    AzureLocation::NorthEurope => "northeurope",
                    AzureLocation::NorwayEast => "norwayeast",
                    AzureLocation::NorwayWest => "norwaywest",
                    AzureLocation::PolandCentral => "polandcentral",
                    AzureLocation::QatarCentral => "qatarcentral",
                    AzureLocation::SouthAfricaNorth => "southafricanorth",
                    AzureLocation::SouthAfricaWest => "southafricawest",
                    AzureLocation::SouthCentralUS => "southcentralus",
                    AzureLocation::SouthIndia => "southindia",
                    AzureLocation::SoutheastAsia => "southeastasia",
                    AzureLocation::SpainCentral => "spaincentral",
                    AzureLocation::SwedenCentral => "swedencentral",
                    AzureLocation::SwedenSouth => "swedensouth",
                    AzureLocation::SwitzerlandNorth => "switzerlandnorth",
                    AzureLocation::SwitzerlandWest => "switzerlandwest",
                    AzureLocation::UAECentral => "uaecentral",
                    AzureLocation::UAENorth => "uaenorth",
                    AzureLocation::UKSouth => "uksouth",
                    AzureLocation::UKWest => "ukwest",
                    AzureLocation::WestCentralUS => "westcentralus",
                    AzureLocation::WestEurope => "westeurope",
                    AzureLocation::WestIndia => "westindia",
                    AzureLocation::WestUS => "westus",
                    AzureLocation::WestUS2 => "westus2",
                    AzureLocation::WestUS3 => "westus3",
                },
                location.to_string()
            );
        }
    }

    #[test]
    fn test_azure_location_from_str() {
        // test all supported locations
        for location in AzureLocation::iter() {
            assert_eq!(location, AzureLocation::from_str(location.to_cloud_provider_format()).unwrap());

            // test unsupported location
            assert!(AzureLocation::from_str("an-unsupported-location").is_err());
        }
    }

    #[test]
    fn test_azure_location_zones() {
        let expected: HashSet<_> = [AzureZone::One, AzureZone::Two, AzureZone::Three].into_iter().collect();

        for location in AzureLocation::iter() {
            let actual: HashSet<_> = location.zones().into_iter().collect();
            assert_eq!(expected, actual, "Mismatch for location {:?}", location);
        }
    }

    #[test]
    fn test_azure_zone_to_azure_format() {
        for zone in AzureZone::iter() {
            assert_eq!(
                match zone {
                    AzureZone::One => "1",
                    AzureZone::Two => "2",
                    AzureZone::Three => "3",
                },
                zone.to_cloud_provider_format()
            );
        }
    }

    #[test]
    fn test_azure_zone_to_string() {
        for zone in AzureZone::iter() {
            assert_eq!(
                match zone {
                    AzureZone::One => "1",
                    AzureZone::Two => "2",
                    AzureZone::Three => "3",
                },
                zone.to_string()
            );
        }
    }

    #[test]
    fn test_azure_zone_from_str() {
        // test all supported zones
        for zone in AzureZone::iter() {
            assert_eq!(zone, AzureZone::from_str(zone.to_cloud_provider_format()).unwrap());

            // test unsupported zone
            assert!(AzureZone::from_str("an-unsupported-zone").is_err());
        }
    }
}
