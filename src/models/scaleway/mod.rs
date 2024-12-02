mod database;
mod database_utils;
mod job;
mod router;

use crate::cloud_provider::Kind;
use crate::errors::CommandError;
use crate::models::types::CloudProvider;
use crate::models::types::SCW;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::StorageRegion;
use std::fmt;
use std::str::FromStr;

pub struct ScwAppExtraSettings {}
pub struct ScwDbExtraSettings {}
pub struct ScwRouterExtraSettings {}

impl CloudProvider for SCW {
    type AppExtraSettings = ScwAppExtraSettings;
    type DbExtraSettings = ScwDbExtraSettings;
    type RouterExtraSettings = ScwRouterExtraSettings;
    fn cloud_provider() -> Kind {
        Kind::Scw
    }

    fn short_name() -> &'static str {
        "SCW"
    }

    fn full_name() -> &'static str {
        "Scaleway"
    }

    fn registry_short_name() -> &'static str {
        "SCW CR"
    }

    fn registry_full_name() -> &'static str {
        "Scaleway Container Registry"
    }

    fn lib_directory_name() -> &'static str {
        "scaleway"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ScwRegion {
    Paris,
    Amsterdam,
    Warsaw,
}

impl ScwRegion {
    // TODO(benjaminch): improve / refactor this!
    pub fn as_str(&self) -> &str {
        match self {
            ScwRegion::Paris => "fr-par",
            ScwRegion::Amsterdam => "nl-ams",
            ScwRegion::Warsaw => "pl-waw",
        }
    }
}

impl fmt::Display for ScwRegion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ScwRegion::Paris => write!(f, "fr-par"),
            ScwRegion::Amsterdam => write!(f, "nl-ams"),
            ScwRegion::Warsaw => write!(f, "pl-waw"),
        }
    }
}

impl FromStr for ScwRegion {
    type Err = ();

    fn from_str(s: &str) -> Result<ScwRegion, ()> {
        if s.len() < "fr-par".len() {
            return Err(());
        }

        match &s[..6] {
            "fr-par" => Ok(ScwRegion::Paris),
            "nl-ams" => Ok(ScwRegion::Amsterdam),
            "pl-waw" => Ok(ScwRegion::Warsaw),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScwZone {
    Paris1,
    Paris2,
    Paris3,
    Amsterdam1,
    Amsterdam2,
    Amsterdam3,
    Warsaw1,
    Warsaw2,
    Warsaw3,
}

impl ScwZone {
    // TODO(benjaminch): improve / refactor this!
    pub fn as_str(&self) -> &str {
        match self {
            ScwZone::Paris1 => "fr-par-1",
            ScwZone::Paris2 => "fr-par-2",
            ScwZone::Paris3 => "fr-par-3",
            ScwZone::Amsterdam1 => "nl-ams-1",
            ScwZone::Amsterdam2 => "nl-ams-2",
            ScwZone::Amsterdam3 => "nl-ams-3",
            ScwZone::Warsaw1 => "pl-waw-1",
            ScwZone::Warsaw2 => "pl-waw-2",
            ScwZone::Warsaw3 => "pl-waw-3",
        }
    }

    pub fn region(&self) -> ScwRegion {
        match self {
            ScwZone::Paris1 => ScwRegion::Paris,
            ScwZone::Paris2 => ScwRegion::Paris,
            ScwZone::Paris3 => ScwRegion::Paris,
            ScwZone::Amsterdam1 => ScwRegion::Amsterdam,
            ScwZone::Amsterdam2 => ScwRegion::Amsterdam,
            ScwZone::Amsterdam3 => ScwRegion::Amsterdam,
            ScwZone::Warsaw1 => ScwRegion::Warsaw,
            ScwZone::Warsaw2 => ScwRegion::Warsaw,
            ScwZone::Warsaw3 => ScwRegion::Warsaw,
        }
    }
}

impl fmt::Display for ScwZone {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.to_cloud_provider_format())
    }
}

impl FromStr for ScwZone {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<ScwZone, CommandError> {
        match s {
            "fr-par-1" => Ok(ScwZone::Paris1),
            "fr-par-2" => Ok(ScwZone::Paris2),
            "fr-par-3" => Ok(ScwZone::Paris3),
            "nl-ams-1" => Ok(ScwZone::Amsterdam1),
            "nl-ams-2" => Ok(ScwZone::Amsterdam2),
            "nl-ams-3" => Ok(ScwZone::Amsterdam3),
            "pl-waw-1" => Ok(ScwZone::Warsaw1),
            "pl-waw-2" => Ok(ScwZone::Warsaw2),
            "pl-waw-3" => Ok(ScwZone::Warsaw3),
            _ => Err(CommandError::new_from_safe_message(format!("`{s}` zone is not supported"))),
        }
    }
}

impl StorageRegion for ScwZone {}

impl ToCloudProviderFormat for ScwZone {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            ScwZone::Paris1 => "fr-par-1",
            ScwZone::Paris2 => "fr-par-2",
            ScwZone::Paris3 => "fr-par-3",
            ScwZone::Amsterdam1 => "nl-ams-1",
            ScwZone::Amsterdam2 => "nl-ams-2",
            ScwZone::Amsterdam3 => "nl-ams-3",
            ScwZone::Warsaw1 => "pl-waw-1",
            ScwZone::Warsaw2 => "pl-waw-2",
            ScwZone::Warsaw3 => "pl-waw-3",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum ScwStorageType {
    SbvSsd,
}

impl ScwStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            ScwStorageType::SbvSsd => "scw-sbv-ssd-0".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScwRegion, ScwZone};
    use std::str::FromStr;

    #[test]
    fn test_region_to_str() {
        assert_eq!("fr-par", ScwRegion::Paris.as_str());
        assert_eq!("nl-ams", ScwRegion::Amsterdam.as_str());
        assert_eq!("pl-waw", ScwRegion::Warsaw.as_str());
    }

    #[test]
    fn test_region_from_str() {
        assert_eq!(ScwRegion::from_str("fr-par"), Ok(ScwRegion::Paris));
        assert_eq!(ScwRegion::from_str("nl-ams"), Ok(ScwRegion::Amsterdam));
        assert_eq!(ScwRegion::from_str("pl-waw"), Ok(ScwRegion::Warsaw));
        assert_eq!(ScwRegion::from_str("fr-par-1"), Ok(ScwRegion::Paris));
        assert_eq!(ScwRegion::from_str("fr-par-2"), Ok(ScwRegion::Paris));
        assert_eq!(ScwRegion::from_str("fr-par-3"), Ok(ScwRegion::Paris));
        assert_eq!(ScwRegion::from_str("nl-ams-1"), Ok(ScwRegion::Amsterdam));
        assert_eq!(ScwRegion::from_str("nl-ams-2"), Ok(ScwRegion::Amsterdam));
        assert_eq!(ScwRegion::from_str("nl-ams-3"), Ok(ScwRegion::Amsterdam));
        assert_eq!(ScwRegion::from_str("pl-waw-1"), Ok(ScwRegion::Warsaw));
        assert_eq!(ScwRegion::from_str("pl-waw-2"), Ok(ScwRegion::Warsaw));
        assert_eq!(ScwRegion::from_str("pl-waw-3"), Ok(ScwRegion::Warsaw));
    }

    #[test]
    fn test_zone_to_str() {
        assert_eq!("fr-par-1", ScwZone::Paris1.as_str());
        assert_eq!("fr-par-2", ScwZone::Paris2.as_str());
        assert_eq!("fr-par-3", ScwZone::Paris3.as_str());
        assert_eq!("nl-ams-1", ScwZone::Amsterdam1.as_str());
        assert_eq!("nl-ams-2", ScwZone::Amsterdam2.as_str());
        assert_eq!("nl-ams-3", ScwZone::Amsterdam3.as_str());
        assert_eq!("pl-waw-1", ScwZone::Warsaw1.as_str());
        assert_eq!("pl-waw-2", ScwZone::Warsaw2.as_str());
        assert_eq!("pl-waw-3", ScwZone::Warsaw3.as_str());
    }

    #[test]
    fn test_zone_from_str() {
        assert_eq!(ScwZone::from_str("fr-par-1"), Ok(ScwZone::Paris1));
        assert_eq!(ScwZone::from_str("fr-par-2"), Ok(ScwZone::Paris2));
        assert_eq!(ScwZone::from_str("fr-par-3"), Ok(ScwZone::Paris3));
        assert_eq!(ScwZone::from_str("nl-ams-1"), Ok(ScwZone::Amsterdam1));
        assert_eq!(ScwZone::from_str("nl-ams-2"), Ok(ScwZone::Amsterdam2));
        assert_eq!(ScwZone::from_str("nl-ams-3"), Ok(ScwZone::Amsterdam3));
        assert_eq!(ScwZone::from_str("pl-waw-1"), Ok(ScwZone::Warsaw1));
        assert_eq!(ScwZone::from_str("pl-waw-2"), Ok(ScwZone::Warsaw2));
        assert_eq!(ScwZone::from_str("pl-waw-3"), Ok(ScwZone::Warsaw3));
    }

    #[test]
    fn test_zone_region() {
        assert_eq!(ScwZone::Paris1.region(), ScwRegion::Paris);
        assert_eq!(ScwZone::Paris2.region(), ScwRegion::Paris);
        assert_eq!(ScwZone::Paris3.region(), ScwRegion::Paris);
        assert_eq!(ScwZone::Amsterdam1.region(), ScwRegion::Amsterdam);
        assert_eq!(ScwZone::Amsterdam2.region(), ScwRegion::Amsterdam);
        assert_eq!(ScwZone::Amsterdam3.region(), ScwRegion::Amsterdam);
        assert_eq!(ScwZone::Warsaw1.region(), ScwRegion::Warsaw);
        assert_eq!(ScwZone::Warsaw2.region(), ScwRegion::Warsaw);
        assert_eq!(ScwZone::Warsaw3.region(), ScwRegion::Warsaw);
    }
}
