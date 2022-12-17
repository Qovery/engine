use crate::cloud_provider::aws::regions::AwsZones::*;
use crate::cloud_provider::aws::regions::RegionAndZoneErrors::*;
use crate::io_models::domain::ToTerraformString;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use strum_macros::EnumIter;

// Sync with Qovery Core team if you update this content
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, EnumIter)]
pub enum AwsZones {
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
}

impl ToTerraformString for AwsZones {
    fn to_terraform_format_string(&self) -> String {
        format!("\"{}\"", self)
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
            "ap-southeast-1" | "apsoutheast1" => Ok(AwsRegion::ApSoutheast1),
            "ap-southeast-2" | "apsoutheast2" => Ok(AwsRegion::ApSoutheast2),
            "ca-central-1" | "cacentral1" => Ok(AwsRegion::CaCentral1),
            "eu-central-1" | "eucentral1" => Ok(AwsRegion::EuCentral1),
            "eu-west-1" | "euwest1" => Ok(AwsRegion::EuWest1),
            "eu-west-2" | "euwest2" => Ok(AwsRegion::EuWest2),
            "eu-west-3" | "euwest3" => Ok(AwsRegion::EuWest3),
            "eu-north-1" | "eunorth1" => Ok(AwsRegion::EuNorth1),
            "eu-south-1" | "eusouth1" => Ok(AwsRegion::EuSouth1),
            "me-south-1" | "mesouth1" => Ok(AwsRegion::MeSouth1),
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

impl AwsRegion {
    pub fn new(&self) -> &AwsRegion {
        self
    }

    pub fn to_aws_format(&self) -> &str {
        match self {
            AwsRegion::UsEast1 => "us-east-1",
            AwsRegion::UsEast2 => "us-east-2",
            AwsRegion::UsWest2 => "us-west-2",
            AwsRegion::AfSouth1 => "af-south-1",
            AwsRegion::ApEast1 => "ap-east-1",
            AwsRegion::ApSouth1 => "ap-south-1",
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
            AwsRegion::MeSouth1 => "me-south-1",
            AwsRegion::SaEast1 => "sa-east-1",
        }
    }

    pub fn get_zones_to_string(&self) -> Vec<String> {
        let zones = self.get_zones();
        let zones_to_string: Vec<String> = zones.into_iter().map(|x| x.to_string()).collect();
        zones_to_string
    }

    pub fn get_zones(&self) -> Vec<AwsZones> {
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
            AwsRegion::MeSouth1 => {
                vec![MeSouth1A, MeSouth1B, MeSouth1C]
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

impl AwsZones {
    pub fn from_string(zone: String) -> Result<AwsZones, RegionAndZoneErrors> {
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
        for zone in region.get_zones() {
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
        write!(f, "{:?}", self)
    }
}

impl Display for AwsZones {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let str = match self {
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
        };

        write!(f, "{}", str)
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::regions::AwsZones::{EuWest3A, EuWest3B, EuWest3C};
    use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones, RegionAndZoneErrors};
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_aws_zones() {
        assert_eq!(EuWest3A.to_string(), "eu-west-3a".to_string());
        assert_eq!(AwsZones::ApNortheast1D.to_string(), "ap-northeast-1d".to_string());
        assert_eq!(AwsZones::from_string("eu-west-3a".to_string()), Ok(EuWest3A));

        // ensure all zones are supported
        for zone in AwsZones::iter() {
            let sanitized_zone = format!("{:?}", zone);
            let current_zone = AwsZones::from_string(sanitized_zone.to_lowercase());
            assert_eq!(current_zone.unwrap(), zone);
        }
        assert_eq!(
            AwsZones::from_string("eu-west-3x".to_string()),
            Err(RegionAndZoneErrors::ZoneNotSupported)
        );
    }

    #[test]
    fn test_aws_get_region_from_zone() {
        assert_eq!(EuWest3A.get_region(), "eu-west-3".to_string());
        assert_eq!(AwsZones::ApNortheast1D.get_region(), "ap-northeast-1".to_string());
    }

    #[test]
    fn test_aws_region() {
        assert_eq!(AwsRegion::EuWest3.to_aws_format(), "eu-west-3");
        assert_eq!(AwsRegion::EuWest3.to_string(), "EuWest3");
        assert_eq!(AwsRegion::EuWest3.get_zones(), vec![EuWest3A, EuWest3B, EuWest3C]);
        assert_eq!(AwsRegion::from_str("eu-west-3"), Ok(AwsRegion::EuWest3));
        assert_eq!(AwsRegion::from_str("euwest3"), Ok(AwsRegion::EuWest3));
        assert_eq!(AwsRegion::from_str("euwest"), Err(()));

        for region in AwsRegion::iter() {
            let aws_region = format!("{:?}", region);
            assert!(AwsRegion::from_str(aws_region.as_str()).is_ok());
        }
    }
}
