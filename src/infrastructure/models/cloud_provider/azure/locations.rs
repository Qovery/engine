use crate::environment::models::ToCloudProviderFormat;
use serde_derive::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter)]
pub enum AzureLocation {
    AustraliaCentral,
    AustraliaCentral2,
    AustraliaEast,
    AustraliaSoutheast,
    AustriaEast,
    BrazilSouth,
    BrazilSoutheast,
    CanadaCentral,
    CanadaEast,
    CentralIndia,
    CentralUS,
    EastAsia,
    EastUS,
    EastUS2,
    FranceCentral,
    FranceSouth,
    GermanyNorth,
    GermanyWestCentral,
    IndonesiaCentral,
    IsraelCentral,
    ItalyNorth,
    JapanEast,
    JapanWest,
    KoreaCentral,
    KoreaSouth,
    MexicoCentral,
    NewZealandNorth,
    NorthCentralUS,
    NorthEurope,
    NorwayEast,
    NorwayWest,
    PolandCentral,
    QatarCentral,
    SouthAfricaNorth,
    SouthAfricaWest,
    SouthCentralUS,
    SouthIndia,
    SoutheastAsia,
    SpainCentral,
    SwedenCentral,
    SwedenSouth,
    SwitzerlandNorth,
    SwitzerlandWest,
    UAECentral,
    UAENorth,
    UKSouth,
    UKWest,
    WestCentralUS,
    WestEurope,
    WestIndia,
    WestUS,
    WestUS2,
    WestUS3,
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
    use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
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
}
