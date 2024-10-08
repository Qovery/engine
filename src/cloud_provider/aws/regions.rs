use crate::cloud_provider::aws::regions::AwsZone::*;
use crate::cloud_provider::aws::regions::RegionAndZoneErrors::*;
use crate::models::domain::ToTerraformString;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::StorageRegion;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

// Sync with Qovery Core team if you update this content
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter)]
pub enum AwsZone {
    // North Virginia
    UsEast1A,
    UsEast1B,
    UsEast1C,
    // Ohio
    UsEast2A,
    UsEast2B,
    UsEast2C,
    // Oregon
    UsWest2A,
    UsWest2B,
    UsWest2C,
    // Cap Town
    AfSouth1A,
    AfSouth1B,
    AfSouth1C,
    // Hong Kong
    ApEast1A,
    ApEast1B,
    ApEast1C,
    // Mumbai
    ApSouth1A,
    ApSouth1B,
    ApSouth1C,
    // Tokyo
    ApNortheast1A,
    ApNortheast1C,
    ApNortheast1D,
    // Seoul
    ApNortheast2A,
    ApNortheast2B,
    ApNortheast2C,
    // Osaka
    ApNortheast3A,
    ApNortheast3B,
    ApNortheast3C,
    // Singapore
    ApSoutheast1A,
    ApSoutheast1B,
    ApSoutheast1C,
    // Sydney
    ApSoutheast2A,
    ApSoutheast2B,
    ApSoutheast2C,
    // Toronto
    CaCentral1A,
    CaCentral1B,
    CaCentral1D,
    // Beijing
    CnNorth1A,
    CnNorth1B,
    CnNorth1C,
    // Ningxia
    CnNorthwest1A,
    CnNorthwest1B,
    CnNorthwest1C,
    // Frankfurt
    EuCentral1A,
    EuCentral1B,
    EuCentral1C,
    // Ireland
    EuWest1A,
    EuWest1B,
    EuWest1C,
    // London
    EuWest2A,
    EuWest2B,
    EuWest2C,
    // Paris
    EuWest3A,
    EuWest3B,
    EuWest3C,
    // Stockholm
    EuNorth1A,
    EuNorth1B,
    EuNorth1C,
    // Milan
    EuSouth1A,
    EuSouth1B,
    EuSouth1C,
    // Bahrain
    MeSouth1A,
    MeSouth1B,
    MeSouth1C,
    // Sao Paulo
    SaEast1A,
    SaEast1B,
    SaEast1C,
    // Asia Pacific (Hyderabad)
    ApSouth2A,
    ApSouth2B,
    ApSouth2C,
    // Europe (Spain)
    EuSouth2A,
    EuSouth2B,
    EuSouth2C,
    // Middle East (UAE)
    MeCentral1A,
    MeCentral1B,
    MeCentral1C,
}

impl AwsZone {
    pub fn region(&self) -> AwsRegion {
        match self {
            UsEast1A | UsEast1B | UsEast1C => AwsRegion::UsEast1,
            UsEast2A | UsEast2B | UsEast2C => AwsRegion::UsEast2,
            UsWest2A | UsWest2B | UsWest2C => AwsRegion::UsWest2,
            AfSouth1A | AfSouth1B | AfSouth1C => AwsRegion::AfSouth1,
            ApEast1A | ApEast1B | ApEast1C => AwsRegion::ApEast1,
            ApSouth1A | ApSouth1B | ApSouth1C => AwsRegion::ApSouth1,
            ApSouth2A | ApSouth2B | ApSouth2C => AwsRegion::ApSouth2,
            ApNortheast1A | ApNortheast1C | ApNortheast1D => AwsRegion::ApNortheast1,
            ApNortheast2A | ApNortheast2B | ApNortheast2C => AwsRegion::ApNortheast2,
            ApNortheast3A | ApNortheast3B | ApNortheast3C => AwsRegion::ApNortheast3,
            ApSoutheast1A | ApSoutheast1B | ApSoutheast1C => AwsRegion::ApSoutheast1,
            ApSoutheast2A | ApSoutheast2B | ApSoutheast2C => AwsRegion::ApSoutheast2,
            CaCentral1A | CaCentral1B | CaCentral1D => AwsRegion::CaCentral1,
            CnNorth1A | CnNorth1B | CnNorth1C => AwsRegion::CnNorth1,
            CnNorthwest1A | CnNorthwest1B | CnNorthwest1C => AwsRegion::CnNorthwest1,
            EuCentral1A | EuCentral1B | EuCentral1C => AwsRegion::EuCentral1,
            EuWest1A | EuWest1B | EuWest1C => AwsRegion::EuWest1,
            EuWest2A | EuWest2B | EuWest2C => AwsRegion::EuWest2,
            EuWest3A | EuWest3B | EuWest3C => AwsRegion::EuWest3,
            EuNorth1A | EuNorth1B | EuNorth1C => AwsRegion::EuNorth1,
            EuSouth1A | EuSouth1B | EuSouth1C => AwsRegion::EuSouth1,
            EuSouth2A | EuSouth2B | EuSouth2C => AwsRegion::EuSouth2,
            MeSouth1A | MeSouth1B | MeSouth1C => AwsRegion::MeSouth1,
            MeCentral1A | MeCentral1B | MeCentral1C => AwsRegion::MeCentral1,
            SaEast1A | SaEast1B | SaEast1C => AwsRegion::SaEast1,
        }
    }
}

impl ToTerraformString for AwsZone {
    fn to_terraform_format_string(&self) -> String {
        format!("\"{self}\"")
    }
}

impl ToCloudProviderFormat for AwsZone {
    fn to_cloud_provider_format(&self) -> &str {
        match &self {
            UsEast1A => "us-east-1a",
            UsEast1B => "us-east-1b",
            UsEast1C => "us-east-1c",
            UsEast2A => "us-east-2a",
            UsEast2B => "us-east-2b",
            UsEast2C => "us-east-2c",
            UsWest2A => "us-west-2a",
            UsWest2B => "us-west-2b",
            UsWest2C => "us-west-2c",
            AfSouth1A => "af-south-1a",
            AfSouth1B => "af-south-1b",
            AfSouth1C => "af-south-1c",
            ApEast1A => "ap-east-1a",
            ApEast1B => "ap-east-1b",
            ApEast1C => "ap-east-1c",
            ApSouth1A => "ap-south-1a",
            ApSouth1B => "ap-south-1b",
            ApSouth1C => "ap-south-1c",
            ApNortheast1A => "ap-northeast-1a",
            ApNortheast1C => "ap-northeast-1c",
            ApNortheast1D => "ap-northeast-1d",
            ApNortheast2A => "ap-northeast-2a",
            ApNortheast2B => "ap-northeast-2b",
            ApNortheast2C => "ap-northeast-2c",
            ApNortheast3A => "ap-northeast-3a",
            ApNortheast3B => "ap-northeast-3b",
            ApNortheast3C => "ap-northeast-3c",
            ApSoutheast1A => "ap-southeast-1a",
            ApSoutheast1B => "ap-southeast-1b",
            ApSoutheast1C => "ap-southeast-1c",
            ApSoutheast2A => "ap-southeast-2a",
            ApSoutheast2B => "ap-southeast-2b",
            ApSoutheast2C => "ap-southeast-2c",
            CaCentral1A => "ca-central-1a",
            CaCentral1B => "ca-central-1b",
            CaCentral1D => "ca-central-1d",
            CnNorth1A => "cn-north-1a",
            CnNorth1B => "cn-north-1b",
            CnNorth1C => "cn-north-1c",
            CnNorthwest1A => "cn-northwest-1a",
            CnNorthwest1B => "cn-northwest-1b",
            CnNorthwest1C => "cn-northwest-1c",
            EuCentral1A => "eu-central-1a",
            EuCentral1B => "eu-central-1b",
            EuCentral1C => "eu-central-1c",
            EuWest1A => "eu-west-1a",
            EuWest1B => "eu-west-1b",
            EuWest1C => "eu-west-1c",
            EuWest2A => "eu-west-2a",
            EuWest2B => "eu-west-2b",
            EuWest2C => "eu-west-2c",
            EuWest3A => "eu-west-3a",
            EuWest3B => "eu-west-3b",
            EuWest3C => "eu-west-3c",
            EuNorth1A => "eu-north-1a",
            EuNorth1B => "eu-north-1b",
            EuNorth1C => "eu-north-1c",
            EuSouth1A => "eu-south-1a",
            EuSouth1B => "eu-south-1b",
            EuSouth1C => "eu-south-1c",
            MeSouth1A => "me-south-1a",
            MeSouth1B => "me-south-1b",
            MeSouth1C => "me-south-1c",
            SaEast1A => "sa-east-1a",
            SaEast1B => "sa-east-1b",
            SaEast1C => "sa-east-1c",
            ApSouth2A => "ap-south-2a",
            ApSouth2B => "ap-south-2b",
            ApSouth2C => "ap-south-2c",
            EuSouth2A => "eu-south-2a",
            EuSouth2B => "eu-south-2b",
            EuSouth2C => "eu-south-2c",
            MeCentral1A => "me-central-1a",
            MeCentral1B => "me-central-1b",
            MeCentral1C => "me-central-1c",
        }
    }
}

impl Display for AwsZone {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_cloud_provider_format())
    }
}

impl FromStr for AwsZone {
    type Err = ();

    fn from_str(s: &str) -> Result<AwsZone, ()> {
        let v: &str = &s.to_lowercase();
        match v {
            "us-east-1a" => Ok(UsEast1A),
            "us-east-1b" => Ok(UsEast1B),
            "us-east-1c" => Ok(UsEast1C),
            "us-east-2a" => Ok(UsEast2A),
            "us-east-2b" => Ok(UsEast2B),
            "us-east-2c" => Ok(UsEast2C),
            "us-west-2a" => Ok(UsWest2A),
            "us-west-2b" => Ok(UsWest2B),
            "us-west-2c" => Ok(UsWest2C),
            "af-south-1a" => Ok(AfSouth1A),
            "af-south-1b" => Ok(AfSouth1B),
            "af-south-1c" => Ok(AfSouth1C),
            "ap-east-1a" => Ok(ApEast1A),
            "ap-east-1b" => Ok(ApEast1B),
            "ap-east-1c" => Ok(ApEast1C),
            "ap-south-1a" => Ok(ApSouth1A),
            "ap-south-1b" => Ok(ApSouth1B),
            "ap-south-1c" => Ok(ApSouth1C),
            "ap-northeast-1a" => Ok(ApNortheast1A),
            "ap-northeast-1c" => Ok(ApNortheast1C),
            "ap-northeast-1d" => Ok(ApNortheast1D),
            "ap-northeast-2a" => Ok(ApNortheast2A),
            "ap-northeast-2b" => Ok(ApNortheast2B),
            "ap-northeast-2c" => Ok(ApNortheast2C),
            "ap-northeast-3a" => Ok(ApNortheast3A),
            "ap-northeast-3b" => Ok(ApNortheast3B),
            "ap-northeast-3c" => Ok(ApNortheast3C),
            "ap-southeast-1a" => Ok(ApSoutheast1A),
            "ap-southeast-1b" => Ok(ApSoutheast1B),
            "ap-southeast-1c" => Ok(ApSoutheast1C),
            "ap-southeast-2a" => Ok(ApSoutheast2A),
            "ap-southeast-2b" => Ok(ApSoutheast2B),
            "ap-southeast-2c" => Ok(ApSoutheast2C),
            "ca-central-1a" => Ok(CaCentral1A),
            "ca-central-1b" => Ok(CaCentral1B),
            "ca-central-1d" => Ok(CaCentral1D),
            "cn-north-1a" => Ok(CnNorth1A),
            "cn-north-1b" => Ok(CnNorth1B),
            "cn-north-1c" => Ok(CnNorth1C),
            "cn-northwest-1a" => Ok(CnNorthwest1A),
            "cn-northwest-1b" => Ok(CnNorthwest1B),
            "cn-northwest-1c" => Ok(CnNorthwest1C),
            "eu-central-1a" => Ok(EuCentral1A),
            "eu-central-1b" => Ok(EuCentral1B),
            "eu-central-1c" => Ok(EuCentral1C),
            "eu-west-1a" => Ok(EuWest1A),
            "eu-west-1b" => Ok(EuWest1B),
            "eu-west-1c" => Ok(EuWest1C),
            "eu-west-2a" => Ok(EuWest2A),
            "eu-west-2b" => Ok(EuWest2B),
            "eu-west-2c" => Ok(EuWest2C),
            "eu-west-3a" => Ok(EuWest3A),
            "eu-west-3b" => Ok(EuWest3B),
            "eu-west-3c" => Ok(EuWest3C),
            "eu-north-1a" => Ok(EuNorth1A),
            "eu-north-1b" => Ok(EuNorth1B),
            "eu-north-1c" => Ok(EuNorth1C),
            "eu-south-1a" => Ok(EuSouth1A),
            "eu-south-1b" => Ok(EuSouth1B),
            "eu-south-1c" => Ok(EuSouth1C),
            "me-south-1a" => Ok(MeSouth1A),
            "me-south-1b" => Ok(MeSouth1B),
            "me-south-1c" => Ok(MeSouth1C),
            "sa-east-1a" => Ok(SaEast1A),
            "sa-east-1b" => Ok(SaEast1B),
            "sa-east-1c" => Ok(SaEast1C),
            "ap-south-2a" => Ok(ApSouth2A),
            "ap-south-2b" => Ok(ApSouth2B),
            "ap-south-2c" => Ok(ApSouth2C),
            "eu-south-2a" => Ok(EuSouth2A),
            "eu-south-2b" => Ok(EuSouth2B),
            "eu-south-2c" => Ok(EuSouth2C),
            "me-central-1a" => Ok(MeCentral1A),
            "me-central-1b" => Ok(MeCentral1B),
            "me-central-1c" => Ok(MeCentral1C),
            _ => Err(()),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter)]
// Sync with Qovery Core team if you update this content
pub enum AwsRegion {
    UsEast1,
    UsEast2,
    UsWest2,
    AfSouth1,
    ApEast1,
    ApSouth1,
    ApSouth2,
    ApNortheast1,
    ApNortheast2,
    ApNortheast3,
    ApSoutheast1,
    ApSoutheast2,
    CaCentral1,
    CnNorth1,
    CnNorthwest1,
    EuCentral1,
    EuWest1,
    EuWest2,
    EuWest3,
    EuNorth1,
    EuSouth1,
    EuSouth2,
    MeCentral1,
    MeSouth1,
    SaEast1,
}

impl FromStr for AwsRegion {
    type Err = ();

    fn from_str(s: &str) -> Result<AwsRegion, ()> {
        let v: &str = &s.to_lowercase();
        match v {
            "ap-east-1" | "apeast1" => Ok(AwsRegion::ApEast1),
            "ap-northeast-1" | "apnortheast1" => Ok(AwsRegion::ApNortheast1),
            "ap-northeast-2" | "apnortheast2" => Ok(AwsRegion::ApNortheast2),
            "ap-northeast-3" | "apnortheast3" => Ok(AwsRegion::ApNortheast3),
            "ap-south-1" | "apsouth1" => Ok(AwsRegion::ApSouth1),
            "ap-south-2" | "apsouth2" => Ok(AwsRegion::ApSouth2),
            "ap-southeast-1" | "apsoutheast1" => Ok(AwsRegion::ApSoutheast1),
            "ap-southeast-2" | "apsoutheast2" => Ok(AwsRegion::ApSoutheast2),
            "ca-central-1" | "cacentral1" => Ok(AwsRegion::CaCentral1),
            "eu-central-1" | "eucentral1" => Ok(AwsRegion::EuCentral1),
            "eu-west-1" | "euwest1" => Ok(AwsRegion::EuWest1),
            "eu-west-2" | "euwest2" => Ok(AwsRegion::EuWest2),
            "eu-west-3" | "euwest3" => Ok(AwsRegion::EuWest3),
            "eu-north-1" | "eunorth1" => Ok(AwsRegion::EuNorth1),
            "eu-south-1" | "eusouth1" => Ok(AwsRegion::EuSouth1),
            "eu-south-2" | "eusouth2" => Ok(AwsRegion::EuSouth2),
            "me-south-1" | "mesouth1" => Ok(AwsRegion::MeSouth1),
            "me-central-1" | "mecentral1" => Ok(AwsRegion::MeCentral1),
            "sa-east-1" | "saeast1" => Ok(AwsRegion::SaEast1),
            "us-east-1" | "useast1" => Ok(AwsRegion::UsEast1),
            "us-east-2" | "useast2" => Ok(AwsRegion::UsEast2),
            "us-west-2" | "uswest2" => Ok(AwsRegion::UsWest2),
            "cn-north-1" | "cnnorth1" => Ok(AwsRegion::CnNorth1),
            "cn-northwest-1" | "cnnorthwest1" => Ok(AwsRegion::CnNorthwest1),
            "af-south-1" | "afsouth1" => Ok(AwsRegion::AfSouth1),
            _ => Err(()),
        }
    }
}

impl StorageRegion for AwsRegion {}

impl ToCloudProviderFormat for AwsRegion {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            AwsRegion::UsEast1 => "us-east-1",
            AwsRegion::UsEast2 => "us-east-2",
            AwsRegion::UsWest2 => "us-west-2",
            AwsRegion::AfSouth1 => "af-south-1",
            AwsRegion::ApEast1 => "ap-east-1",
            AwsRegion::ApSouth1 => "ap-south-1",
            AwsRegion::ApSouth2 => "ap-south-2",
            AwsRegion::ApNortheast1 => "ap-northeast-1",
            AwsRegion::ApNortheast2 => "ap-northeast-2",
            AwsRegion::ApNortheast3 => "ap-northeast-3",
            AwsRegion::ApSoutheast1 => "ap-southeast-1",
            AwsRegion::ApSoutheast2 => "ap-southeast-2",
            AwsRegion::CaCentral1 => "ca-central-1",
            AwsRegion::CnNorth1 => "cn-north-1",
            AwsRegion::CnNorthwest1 => "cn-northwest-1",
            AwsRegion::EuCentral1 => "eu-central-1",
            AwsRegion::EuWest1 => "eu-west-1",
            AwsRegion::EuWest2 => "eu-west-2",
            AwsRegion::EuWest3 => "eu-west-3",
            AwsRegion::EuNorth1 => "eu-north-1",
            AwsRegion::EuSouth1 => "eu-south-1",
            AwsRegion::EuSouth2 => "eu-south-2",
            AwsRegion::MeSouth1 => "me-south-1",
            AwsRegion::MeCentral1 => "me-central-1",
            AwsRegion::SaEast1 => "sa-east-1",
        }
    }
}

impl AwsRegion {
    pub fn new(&self) -> &AwsRegion {
        self
    }

    pub fn get_zones_to_string(&self) -> Vec<String> {
        let zones = self.zones();
        let zones_to_string: Vec<String> = zones.into_iter().map(|x| x.to_string()).collect();
        zones_to_string
    }

    pub fn zones(&self) -> Vec<AwsZone> {
        // Warning, order matters
        match self {
            AwsRegion::UsEast1 => {
                vec![UsEast1A, UsEast1B, UsEast1C]
            }
            AwsRegion::UsEast2 => {
                vec![UsEast2A, UsEast2B, UsEast2C]
            }
            AwsRegion::UsWest2 => {
                vec![UsWest2A, UsWest2B, UsWest2C]
            }
            AwsRegion::AfSouth1 => {
                vec![AfSouth1A, AfSouth1B, AfSouth1C]
            }
            AwsRegion::ApEast1 => {
                vec![ApEast1A, ApEast1B, ApEast1C]
            }
            AwsRegion::ApSouth1 => {
                vec![ApSouth1A, ApSouth1B, ApSouth1C]
            }
            AwsRegion::ApSouth2 => {
                vec![ApSouth2A, ApSouth2B, ApSouth2C]
            }
            AwsRegion::ApNortheast1 => {
                vec![ApNortheast1A, ApNortheast1C, ApNortheast1D]
            }
            AwsRegion::ApNortheast2 => {
                vec![ApNortheast2A, ApNortheast2B, ApNortheast2C]
            }
            AwsRegion::ApNortheast3 => {
                vec![ApNortheast3A, ApNortheast3B, ApNortheast3C]
            }
            AwsRegion::ApSoutheast1 => {
                vec![ApSoutheast1A, ApSoutheast1B, ApSoutheast1C]
            }
            AwsRegion::ApSoutheast2 => {
                vec![ApSoutheast2A, ApSoutheast2B, ApSoutheast2C]
            }
            AwsRegion::CaCentral1 => {
                vec![CaCentral1A, CaCentral1B, CaCentral1D]
            }
            AwsRegion::CnNorth1 => {
                vec![CnNorth1A, CnNorth1B, CnNorth1C]
            }
            AwsRegion::CnNorthwest1 => {
                vec![CnNorthwest1A, CnNorthwest1B, CnNorthwest1C]
            }
            AwsRegion::EuCentral1 => {
                vec![EuCentral1A, EuCentral1B, EuCentral1C]
            }
            AwsRegion::EuWest1 => {
                vec![EuWest1A, EuWest1B, EuWest1C]
            }
            AwsRegion::EuWest2 => {
                vec![EuWest2A, EuWest2B, EuWest2C]
            }
            AwsRegion::EuWest3 => {
                vec![EuWest3A, EuWest3B, EuWest3C]
            }
            AwsRegion::EuNorth1 => {
                vec![EuNorth1A, EuNorth1B, EuNorth1C]
            }
            AwsRegion::EuSouth1 => {
                vec![EuSouth1A, EuSouth1B, EuSouth1C]
            }
            AwsRegion::EuSouth2 => {
                vec![EuSouth2A, EuSouth2B, EuSouth2C]
            }
            AwsRegion::MeSouth1 => {
                vec![MeSouth1A, MeSouth1B, MeSouth1C]
            }
            AwsRegion::MeCentral1 => {
                vec![MeCentral1A, MeCentral1B, MeCentral1C]
            }
            AwsRegion::SaEast1 => {
                vec![SaEast1A, SaEast1B, SaEast1C]
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RegionAndZoneErrors {
    RegionNotFound,
    RegionNotSupported,
    ZoneNotFound,
    ZoneNotSupported,
}

impl Display for RegionAndZoneErrors {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RegionNotFound => "Region not found",
            RegionNotSupported => "Region not supported",
            ZoneNotFound => "Zone not found",
            ZoneNotSupported => "Zone not supported",
        })
    }
}

impl AwsZone {
    pub fn from_string(zone: String) -> Result<AwsZone, RegionAndZoneErrors> {
        // create tmp region from zone and get zone name (one letter)
        let sanitized_zone_name = zone.to_lowercase().replace(['-', '_'], "");
        let mut sanitized_region = sanitized_zone_name.clone();
        sanitized_region.pop();

        // ensure the region exists
        let region = match AwsRegion::from_str(&sanitized_region) {
            Ok(x) => x,
            Err(_) => return Err(RegionNotFound),
        };
        if region.to_string().to_lowercase() != sanitized_region {
            return Err(RegionNotFound);
        };

        // check if the zone is currently supported
        for zone in region.zones() {
            if zone.to_string().replace('-', "") == sanitized_zone_name {
                return Ok(zone);
            }
        }

        Err(ZoneNotSupported)
    }

    pub fn get_region(&self) -> String {
        let zone = self.to_string();
        zone[0..zone.len() - 1].to_string()
    }
}

impl Display for AwsRegion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::regions::{AwsRegion, AwsZone};
    use crate::models::ToCloudProviderFormat;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_aws_region_to_aws_format() {
        for region in AwsRegion::iter() {
            assert_eq!(
                match region {
                    AwsRegion::UsEast1 => "us-east-1",
                    AwsRegion::UsEast2 => "us-east-2",
                    AwsRegion::UsWest2 => "us-west-2",
                    AwsRegion::AfSouth1 => "af-south-1",
                    AwsRegion::ApEast1 => "ap-east-1",
                    AwsRegion::ApSouth1 => "ap-south-1",
                    AwsRegion::ApSouth2 => "ap-south-2",
                    AwsRegion::ApNortheast1 => "ap-northeast-1",
                    AwsRegion::ApNortheast2 => "ap-northeast-2",
                    AwsRegion::ApNortheast3 => "ap-northeast-3",
                    AwsRegion::ApSoutheast1 => "ap-southeast-1",
                    AwsRegion::ApSoutheast2 => "ap-southeast-2",
                    AwsRegion::CaCentral1 => "ca-central-1",
                    AwsRegion::CnNorth1 => "cn-north-1",
                    AwsRegion::CnNorthwest1 => "cn-northwest-1",
                    AwsRegion::EuCentral1 => "eu-central-1",
                    AwsRegion::EuWest1 => "eu-west-1",
                    AwsRegion::EuWest2 => "eu-west-2",
                    AwsRegion::EuWest3 => "eu-west-3",
                    AwsRegion::EuNorth1 => "eu-north-1",
                    AwsRegion::EuSouth1 => "eu-south-1",
                    AwsRegion::EuSouth2 => "eu-south-2",
                    AwsRegion::MeSouth1 => "me-south-1",
                    AwsRegion::MeCentral1 => "me-central-1",
                    AwsRegion::SaEast1 => "sa-east-1",
                },
                region.to_cloud_provider_format()
            );
        }
    }

    #[test]
    fn test_aws_region_zones() {
        for region in AwsRegion::iter() {
            assert_eq!(
                match region {
                    AwsRegion::UsEast1 => {
                        vec![AwsZone::UsEast1A, AwsZone::UsEast1B, AwsZone::UsEast1C]
                    }
                    AwsRegion::UsEast2 => {
                        vec![AwsZone::UsEast2A, AwsZone::UsEast2B, AwsZone::UsEast2C]
                    }
                    AwsRegion::UsWest2 => {
                        vec![AwsZone::UsWest2A, AwsZone::UsWest2B, AwsZone::UsWest2C]
                    }
                    AwsRegion::AfSouth1 => {
                        vec![AwsZone::AfSouth1A, AwsZone::AfSouth1B, AwsZone::AfSouth1C]
                    }
                    AwsRegion::ApEast1 => {
                        vec![AwsZone::ApEast1A, AwsZone::ApEast1B, AwsZone::ApEast1C]
                    }
                    AwsRegion::ApSouth1 => {
                        vec![AwsZone::ApSouth1A, AwsZone::ApSouth1B, AwsZone::ApSouth1C]
                    }
                    AwsRegion::ApSouth2 => {
                        vec![AwsZone::ApSouth2A, AwsZone::ApSouth2B, AwsZone::ApSouth2C]
                    }
                    AwsRegion::ApNortheast1 => {
                        vec![AwsZone::ApNortheast1A, AwsZone::ApNortheast1C, AwsZone::ApNortheast1D]
                    }
                    AwsRegion::ApNortheast2 => {
                        vec![AwsZone::ApNortheast2A, AwsZone::ApNortheast2B, AwsZone::ApNortheast2C]
                    }
                    AwsRegion::ApNortheast3 => {
                        vec![AwsZone::ApNortheast3A, AwsZone::ApNortheast3B, AwsZone::ApNortheast3C]
                    }
                    AwsRegion::ApSoutheast1 => {
                        vec![AwsZone::ApSoutheast1A, AwsZone::ApSoutheast1B, AwsZone::ApSoutheast1C]
                    }
                    AwsRegion::ApSoutheast2 => {
                        vec![AwsZone::ApSoutheast2A, AwsZone::ApSoutheast2B, AwsZone::ApSoutheast2C]
                    }
                    AwsRegion::CaCentral1 => {
                        vec![AwsZone::CaCentral1A, AwsZone::CaCentral1B, AwsZone::CaCentral1D]
                    }
                    AwsRegion::CnNorth1 => {
                        vec![AwsZone::CnNorth1A, AwsZone::CnNorth1B, AwsZone::CnNorth1C]
                    }
                    AwsRegion::CnNorthwest1 => {
                        vec![AwsZone::CnNorthwest1A, AwsZone::CnNorthwest1B, AwsZone::CnNorthwest1C]
                    }
                    AwsRegion::EuCentral1 => {
                        vec![AwsZone::EuCentral1A, AwsZone::EuCentral1B, AwsZone::EuCentral1C]
                    }
                    AwsRegion::EuWest1 => {
                        vec![AwsZone::EuWest1A, AwsZone::EuWest1B, AwsZone::EuWest1C]
                    }
                    AwsRegion::EuWest2 => {
                        vec![AwsZone::EuWest2A, AwsZone::EuWest2B, AwsZone::EuWest2C]
                    }
                    AwsRegion::EuWest3 => {
                        vec![AwsZone::EuWest3A, AwsZone::EuWest3B, AwsZone::EuWest3C]
                    }
                    AwsRegion::EuNorth1 => {
                        vec![AwsZone::EuNorth1A, AwsZone::EuNorth1B, AwsZone::EuNorth1C]
                    }
                    AwsRegion::EuSouth1 => {
                        vec![AwsZone::EuSouth1A, AwsZone::EuSouth1B, AwsZone::EuSouth1C]
                    }
                    AwsRegion::EuSouth2 => {
                        vec![AwsZone::EuSouth2A, AwsZone::EuSouth2B, AwsZone::EuSouth2C]
                    }
                    AwsRegion::MeSouth1 => {
                        vec![AwsZone::MeSouth1A, AwsZone::MeSouth1B, AwsZone::MeSouth1C]
                    }
                    AwsRegion::MeCentral1 => {
                        vec![AwsZone::MeCentral1A, AwsZone::MeCentral1B, AwsZone::MeCentral1C]
                    }
                    AwsRegion::SaEast1 => {
                        vec![AwsZone::SaEast1A, AwsZone::SaEast1B, AwsZone::SaEast1C]
                    }
                },
                region.zones(),
            )
        }
    }

    #[test]
    fn test_aws_region_to_string() {
        for region in AwsRegion::iter() {
            assert_eq!(
                match region {
                    AwsRegion::UsEast1 => "UsEast1",
                    AwsRegion::UsEast2 => "UsEast2",
                    AwsRegion::UsWest2 => "UsWest2",
                    AwsRegion::AfSouth1 => "AfSouth1",
                    AwsRegion::ApEast1 => "ApEast1",
                    AwsRegion::ApSouth1 => "ApSouth1",
                    AwsRegion::ApSouth2 => "ApSouth2",
                    AwsRegion::ApNortheast1 => "ApNortheast1",
                    AwsRegion::ApNortheast2 => "ApNortheast2",
                    AwsRegion::ApNortheast3 => "ApNortheast3",
                    AwsRegion::ApSoutheast1 => "ApSoutheast1",
                    AwsRegion::ApSoutheast2 => "ApSoutheast2",
                    AwsRegion::CaCentral1 => "CaCentral1",
                    AwsRegion::CnNorth1 => "CnNorth1",
                    AwsRegion::CnNorthwest1 => "CnNorthwest1",
                    AwsRegion::EuCentral1 => "EuCentral1",
                    AwsRegion::EuWest1 => "EuWest1",
                    AwsRegion::EuWest2 => "EuWest2",
                    AwsRegion::EuWest3 => "EuWest3",
                    AwsRegion::EuNorth1 => "EuNorth1",
                    AwsRegion::EuSouth1 => "EuSouth1",
                    AwsRegion::EuSouth2 => "EuSouth2",
                    AwsRegion::MeSouth1 => "MeSouth1",
                    AwsRegion::MeCentral1 => "MeCentral1",
                    AwsRegion::SaEast1 => "SaEast1",
                },
                region.to_string()
            );
        }
    }

    #[test]
    fn test_aws_region_from_str() {
        // test all supported regions
        for region in AwsRegion::iter() {
            assert_eq!(region, AwsRegion::from_str(region.to_cloud_provider_format()).unwrap());
        }

        // test unsupported region
        assert!(AwsRegion::from_str("an-unsupported-region").is_err());
    }

    #[test]
    fn test_aws_zone_to_aws_format() {
        for zone in AwsZone::iter() {
            assert_eq!(
                match zone {
                    AwsZone::UsEast1A => "us-east-1a",
                    AwsZone::UsEast1B => "us-east-1b",
                    AwsZone::UsEast1C => "us-east-1c",
                    AwsZone::UsEast2A => "us-east-2a",
                    AwsZone::UsEast2B => "us-east-2b",
                    AwsZone::UsEast2C => "us-east-2c",
                    AwsZone::UsWest2A => "us-west-2a",
                    AwsZone::UsWest2B => "us-west-2b",
                    AwsZone::UsWest2C => "us-west-2c",
                    AwsZone::AfSouth1A => "af-south-1a",
                    AwsZone::AfSouth1B => "af-south-1b",
                    AwsZone::AfSouth1C => "af-south-1c",
                    AwsZone::ApEast1A => "ap-east-1a",
                    AwsZone::ApEast1B => "ap-east-1b",
                    AwsZone::ApEast1C => "ap-east-1c",
                    AwsZone::ApSouth1A => "ap-south-1a",
                    AwsZone::ApSouth1B => "ap-south-1b",
                    AwsZone::ApSouth1C => "ap-south-1c",
                    AwsZone::ApNortheast1A => "ap-northeast-1a",
                    AwsZone::ApNortheast1C => "ap-northeast-1c",
                    AwsZone::ApNortheast1D => "ap-northeast-1d",
                    AwsZone::ApNortheast2A => "ap-northeast-2a",
                    AwsZone::ApNortheast2B => "ap-northeast-2b",
                    AwsZone::ApNortheast2C => "ap-northeast-2c",
                    AwsZone::ApNortheast3A => "ap-northeast-3a",
                    AwsZone::ApNortheast3B => "ap-northeast-3b",
                    AwsZone::ApNortheast3C => "ap-northeast-3c",
                    AwsZone::ApSoutheast1A => "ap-southeast-1a",
                    AwsZone::ApSoutheast1B => "ap-southeast-1b",
                    AwsZone::ApSoutheast1C => "ap-southeast-1c",
                    AwsZone::ApSoutheast2A => "ap-southeast-2a",
                    AwsZone::ApSoutheast2B => "ap-southeast-2b",
                    AwsZone::ApSoutheast2C => "ap-southeast-2c",
                    AwsZone::CaCentral1A => "ca-central-1a",
                    AwsZone::CaCentral1B => "ca-central-1b",
                    AwsZone::CaCentral1D => "ca-central-1d",
                    AwsZone::CnNorth1A => "cn-north-1a",
                    AwsZone::CnNorth1B => "cn-north-1b",
                    AwsZone::CnNorth1C => "cn-north-1c",
                    AwsZone::CnNorthwest1A => "cn-northwest-1a",
                    AwsZone::CnNorthwest1B => "cn-northwest-1b",
                    AwsZone::CnNorthwest1C => "cn-northwest-1c",
                    AwsZone::EuCentral1A => "eu-central-1a",
                    AwsZone::EuCentral1B => "eu-central-1b",
                    AwsZone::EuCentral1C => "eu-central-1c",
                    AwsZone::EuWest1A => "eu-west-1a",
                    AwsZone::EuWest1B => "eu-west-1b",
                    AwsZone::EuWest1C => "eu-west-1c",
                    AwsZone::EuWest2A => "eu-west-2a",
                    AwsZone::EuWest2B => "eu-west-2b",
                    AwsZone::EuWest2C => "eu-west-2c",
                    AwsZone::EuWest3A => "eu-west-3a",
                    AwsZone::EuWest3B => "eu-west-3b",
                    AwsZone::EuWest3C => "eu-west-3c",
                    AwsZone::EuNorth1A => "eu-north-1a",
                    AwsZone::EuNorth1B => "eu-north-1b",
                    AwsZone::EuNorth1C => "eu-north-1c",
                    AwsZone::EuSouth1A => "eu-south-1a",
                    AwsZone::EuSouth1B => "eu-south-1b",
                    AwsZone::EuSouth1C => "eu-south-1c",
                    AwsZone::MeSouth1A => "me-south-1a",
                    AwsZone::MeSouth1B => "me-south-1b",

                    AwsZone::MeSouth1C => "me-south-1c",
                    AwsZone::SaEast1A => "sa-east-1a",
                    AwsZone::SaEast1B => "sa-east-1b",
                    AwsZone::SaEast1C => "sa-east-1c",
                    AwsZone::ApSouth2A => "ap-south-2a",
                    AwsZone::ApSouth2B => "ap-south-2b",
                    AwsZone::ApSouth2C => "ap-south-2c",
                    AwsZone::EuSouth2A => "eu-south-2a",
                    AwsZone::EuSouth2B => "eu-south-2b",
                    AwsZone::EuSouth2C => "eu-south-2c",
                    AwsZone::MeCentral1A => "me-central-1a",
                    AwsZone::MeCentral1B => "me-central-1b",
                    AwsZone::MeCentral1C => "me-central-1c",
                },
                zone.to_cloud_provider_format(),
            );
        }
    }

    #[test]
    fn test_aws_zone_region() {
        for zone in AwsZone::iter() {
            assert_eq!(
                match zone {
                    AwsZone::UsEast1A | AwsZone::UsEast1B | AwsZone::UsEast1C => AwsRegion::UsEast1,
                    AwsZone::UsEast2A | AwsZone::UsEast2B | AwsZone::UsEast2C => AwsRegion::UsEast2,
                    AwsZone::UsWest2A | AwsZone::UsWest2B | AwsZone::UsWest2C => AwsRegion::UsWest2,
                    AwsZone::AfSouth1A | AwsZone::AfSouth1B | AwsZone::AfSouth1C => AwsRegion::AfSouth1,
                    AwsZone::ApEast1A | AwsZone::ApEast1B | AwsZone::ApEast1C => AwsRegion::ApEast1,
                    AwsZone::ApSouth1A | AwsZone::ApSouth1B | AwsZone::ApSouth1C => AwsRegion::ApSouth1,
                    AwsZone::ApSouth2A | AwsZone::ApSouth2B | AwsZone::ApSouth2C => AwsRegion::ApSouth2,
                    AwsZone::ApNortheast1A | AwsZone::ApNortheast1C | AwsZone::ApNortheast1D => AwsRegion::ApNortheast1,
                    AwsZone::ApNortheast2A | AwsZone::ApNortheast2B | AwsZone::ApNortheast2C => AwsRegion::ApNortheast2,
                    AwsZone::ApNortheast3A | AwsZone::ApNortheast3B | AwsZone::ApNortheast3C => AwsRegion::ApNortheast3,
                    AwsZone::ApSoutheast1A | AwsZone::ApSoutheast1B | AwsZone::ApSoutheast1C => AwsRegion::ApSoutheast1,
                    AwsZone::ApSoutheast2A | AwsZone::ApSoutheast2B | AwsZone::ApSoutheast2C => AwsRegion::ApSoutheast2,
                    AwsZone::CaCentral1A | AwsZone::CaCentral1B | AwsZone::CaCentral1D => AwsRegion::CaCentral1,
                    AwsZone::CnNorth1A | AwsZone::CnNorth1B | AwsZone::CnNorth1C => AwsRegion::CnNorth1,
                    AwsZone::CnNorthwest1A | AwsZone::CnNorthwest1B | AwsZone::CnNorthwest1C => AwsRegion::CnNorthwest1,
                    AwsZone::EuCentral1A | AwsZone::EuCentral1B | AwsZone::EuCentral1C => AwsRegion::EuCentral1,
                    AwsZone::EuWest1A | AwsZone::EuWest1B | AwsZone::EuWest1C => AwsRegion::EuWest1,
                    AwsZone::EuWest2A | AwsZone::EuWest2B | AwsZone::EuWest2C => AwsRegion::EuWest2,
                    AwsZone::EuWest3A | AwsZone::EuWest3B | AwsZone::EuWest3C => AwsRegion::EuWest3,
                    AwsZone::EuNorth1A | AwsZone::EuNorth1B | AwsZone::EuNorth1C => AwsRegion::EuNorth1,
                    AwsZone::EuSouth1A | AwsZone::EuSouth1B | AwsZone::EuSouth1C => AwsRegion::EuSouth1,
                    AwsZone::EuSouth2A | AwsZone::EuSouth2B | AwsZone::EuSouth2C => AwsRegion::EuSouth2,
                    AwsZone::MeSouth1A | AwsZone::MeSouth1B | AwsZone::MeSouth1C => AwsRegion::MeSouth1,
                    AwsZone::MeCentral1A | AwsZone::MeCentral1B | AwsZone::MeCentral1C => AwsRegion::MeCentral1,
                    AwsZone::SaEast1A | AwsZone::SaEast1B | AwsZone::SaEast1C => AwsRegion::SaEast1,
                },
                zone.region()
            );
        }
    }

    #[test]
    fn test_aws_zone_to_string() {
        for zone in AwsZone::iter() {
            assert_eq!(
                match zone {
                    AwsZone::UsEast1A => "us-east-1a",
                    AwsZone::UsEast1B => "us-east-1b",
                    AwsZone::UsEast1C => "us-east-1c",
                    AwsZone::UsEast2A => "us-east-2a",
                    AwsZone::UsEast2B => "us-east-2b",
                    AwsZone::UsEast2C => "us-east-2c",
                    AwsZone::UsWest2A => "us-west-2a",
                    AwsZone::UsWest2B => "us-west-2b",
                    AwsZone::UsWest2C => "us-west-2c",
                    AwsZone::AfSouth1A => "af-south-1a",
                    AwsZone::AfSouth1B => "af-south-1b",
                    AwsZone::AfSouth1C => "af-south-1c",
                    AwsZone::ApEast1A => "ap-east-1a",
                    AwsZone::ApEast1B => "ap-east-1b",
                    AwsZone::ApEast1C => "ap-east-1c",
                    AwsZone::ApSouth1A => "ap-south-1a",
                    AwsZone::ApSouth1B => "ap-south-1b",
                    AwsZone::ApSouth1C => "ap-south-1c",
                    AwsZone::ApNortheast1A => "ap-northeast-1a",
                    AwsZone::ApNortheast1C => "ap-northeast-1c",
                    AwsZone::ApNortheast1D => "ap-northeast-1d",
                    AwsZone::ApNortheast2A => "ap-northeast-2a",
                    AwsZone::ApNortheast2B => "ap-northeast-2b",
                    AwsZone::ApNortheast2C => "ap-northeast-2c",
                    AwsZone::ApNortheast3A => "ap-northeast-3a",
                    AwsZone::ApNortheast3B => "ap-northeast-3b",
                    AwsZone::ApNortheast3C => "ap-northeast-3c",
                    AwsZone::ApSoutheast1A => "ap-southeast-1a",
                    AwsZone::ApSoutheast1B => "ap-southeast-1b",
                    AwsZone::ApSoutheast1C => "ap-southeast-1c",
                    AwsZone::ApSoutheast2A => "ap-southeast-2a",
                    AwsZone::ApSoutheast2B => "ap-southeast-2b",
                    AwsZone::ApSoutheast2C => "ap-southeast-2c",
                    AwsZone::CaCentral1A => "ca-central-1a",
                    AwsZone::CaCentral1B => "ca-central-1b",
                    AwsZone::CaCentral1D => "ca-central-1d",
                    AwsZone::CnNorth1A => "cn-north-1a",
                    AwsZone::CnNorth1B => "cn-north-1b",
                    AwsZone::CnNorth1C => "cn-north-1c",
                    AwsZone::CnNorthwest1A => "cn-northwest-1a",
                    AwsZone::CnNorthwest1B => "cn-northwest-1b",
                    AwsZone::CnNorthwest1C => "cn-northwest-1c",
                    AwsZone::EuCentral1A => "eu-central-1a",
                    AwsZone::EuCentral1B => "eu-central-1b",
                    AwsZone::EuCentral1C => "eu-central-1c",
                    AwsZone::EuWest1A => "eu-west-1a",
                    AwsZone::EuWest1B => "eu-west-1b",
                    AwsZone::EuWest1C => "eu-west-1c",
                    AwsZone::EuWest2A => "eu-west-2a",
                    AwsZone::EuWest2B => "eu-west-2b",
                    AwsZone::EuWest2C => "eu-west-2c",
                    AwsZone::EuWest3A => "eu-west-3a",
                    AwsZone::EuWest3B => "eu-west-3b",
                    AwsZone::EuWest3C => "eu-west-3c",
                    AwsZone::EuNorth1A => "eu-north-1a",
                    AwsZone::EuNorth1B => "eu-north-1b",
                    AwsZone::EuNorth1C => "eu-north-1c",
                    AwsZone::EuSouth1A => "eu-south-1a",
                    AwsZone::EuSouth1B => "eu-south-1b",
                    AwsZone::EuSouth1C => "eu-south-1c",
                    AwsZone::MeSouth1A => "me-south-1a",
                    AwsZone::MeSouth1B => "me-south-1b",
                    AwsZone::MeSouth1C => "me-south-1c",
                    AwsZone::SaEast1A => "sa-east-1a",
                    AwsZone::SaEast1B => "sa-east-1b",
                    AwsZone::SaEast1C => "sa-east-1c",
                    AwsZone::ApSouth2A => "ap-south-2a",
                    AwsZone::ApSouth2B => "ap-south-2b",
                    AwsZone::ApSouth2C => "ap-south-2c",
                    AwsZone::EuSouth2A => "eu-south-2a",
                    AwsZone::EuSouth2B => "eu-south-2b",
                    AwsZone::EuSouth2C => "eu-south-2c",
                    AwsZone::MeCentral1A => "me-central-1a",
                    AwsZone::MeCentral1B => "me-central-1b",
                    AwsZone::MeCentral1C => "me-central-1c",
                },
                zone.to_string(),
            );
        }
    }

    #[test]
    fn test_aws_zone_from_str() {
        // test all supported zones
        for zone in AwsZone::iter() {
            assert_eq!(zone, AwsZone::from_str(zone.to_cloud_provider_format()).unwrap());
        }

        // test unsupported zone
        assert!(AwsRegion::from_str("an-unsupported-zone").is_err());
    }
}
