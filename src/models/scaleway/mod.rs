mod application;
mod container;
mod database;
mod database_utils;
mod job;
mod router;

use crate::cloud_provider::Kind;
use crate::errors::CommandError;
use crate::models::types::CloudProvider;
use crate::models::types::SCW;
use std::fmt;
use std::str::FromStr;

pub struct ScwAppExtraSettings {}
pub struct ScwDbExtraSettings {}
pub struct ScwRouterExtraSettings {}

impl CloudProvider for SCW {
    type AppExtraSettings = ScwAppExtraSettings;
    type DbExtraSettings = ScwDbExtraSettings;
    type RouterExtraSettings = ScwRouterExtraSettings;
    type StorageTypes = ScwStorageType;

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

    fn loadbalancer_l4_annotations() -> &'static [(&'static str, &'static str)] {
        // SCW doesn't support UDP loadbalancer
        // https://www.scaleway.com/en/docs/network/load-balancer/reference-content/configuring-backends/
        // https://www.scaleway.com/en/docs/containers/kubernetes/api-cli/using-load-balancer-annotations/
        &[
            (
                "service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm",
                "leastconn",
            ),
            ("service.beta.kubernetes.io/scw-loadbalancer-protocol-http", "false"),
            ("service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v1", "false"),
            ("service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2", "false"),
            ("service.beta.kubernetes.io/scw-loadbalancer-health-check-type", "tcp"),
            ("service.beta.kubernetes.io/scw-loadbalancer-use-hostname", "false"),
        ]
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde_derive::Serialize, serde_derive::Deserialize)]
pub enum ScwStorageType {
    #[serde(rename = "b_ssd")]
    BlockSsd,
    #[serde(rename = "l_ssd")]
    LocalSsd,
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
        match s {
            "fr-par" => Ok(ScwRegion::Paris),
            "nl-ams" => Ok(ScwRegion::Amsterdam),
            "pl-waw" => Ok(ScwRegion::Warsaw),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ScwZone {
    Paris1,
    Paris2,
    Paris3,
    Amsterdam1,
    Warsaw1,
}

impl ScwZone {
    // TODO(benjaminch): improve / refactor this!
    pub fn as_str(&self) -> &str {
        match self {
            ScwZone::Paris1 => "fr-par-1",
            ScwZone::Paris2 => "fr-par-2",
            ScwZone::Paris3 => "fr-par-3",
            ScwZone::Amsterdam1 => "nl-ams-1",
            ScwZone::Warsaw1 => "pl-waw-1",
        }
    }

    pub fn region(&self) -> ScwRegion {
        match self {
            ScwZone::Paris1 => ScwRegion::Paris,
            ScwZone::Paris2 => ScwRegion::Paris,
            ScwZone::Paris3 => ScwRegion::Paris,
            ScwZone::Amsterdam1 => ScwRegion::Amsterdam,
            ScwZone::Warsaw1 => ScwRegion::Warsaw,
        }
    }

    // TODO(benjaminch): improve / refactor this!
    pub fn region_str(&self) -> &str {
        match self {
            ScwZone::Paris1 => "fr-par",
            ScwZone::Paris2 => "fr-par",
            ScwZone::Paris3 => "fr-par",
            ScwZone::Amsterdam1 => "nl-ams",
            ScwZone::Warsaw1 => "pl-waw",
        }
    }
}

impl fmt::Display for ScwZone {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ScwZone::Paris1 => write!(f, "fr-par-1"),
            ScwZone::Paris2 => write!(f, "fr-par-2"),
            ScwZone::Paris3 => write!(f, "fr-par-3"),
            ScwZone::Amsterdam1 => write!(f, "nl-ams-1"),
            ScwZone::Warsaw1 => write!(f, "pl-waw-1"),
        }
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
            "pl-waw-1" => Ok(ScwZone::Warsaw1),
            _ => Err(CommandError::new_from_safe_message(format!("`{s}` zone is not supported"))),
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
    }

    #[test]
    fn test_zone_to_str() {
        assert_eq!("fr-par-1", ScwZone::Paris1.as_str());
        assert_eq!("fr-par-2", ScwZone::Paris2.as_str());
        assert_eq!("fr-par-3", ScwZone::Paris3.as_str());
        assert_eq!("nl-ams-1", ScwZone::Amsterdam1.as_str());
        assert_eq!("pl-waw-1", ScwZone::Warsaw1.as_str());
    }

    #[test]
    fn test_zone_from_str() {
        assert_eq!(ScwZone::from_str("fr-par-1"), Ok(ScwZone::Paris1));
        assert_eq!(ScwZone::from_str("fr-par-2"), Ok(ScwZone::Paris2));
        assert_eq!(ScwZone::from_str("fr-par-3"), Ok(ScwZone::Paris3));
        assert_eq!(ScwZone::from_str("nl-ams-1"), Ok(ScwZone::Amsterdam1));
        assert_eq!(ScwZone::from_str("pl-waw-1"), Ok(ScwZone::Warsaw1));
    }

    #[test]
    fn test_zone_region() {
        assert_eq!(ScwZone::Paris1.region(), ScwRegion::Paris);
        assert_eq!(ScwZone::Paris2.region(), ScwRegion::Paris);
        assert_eq!(ScwZone::Paris3.region(), ScwRegion::Paris);
        assert_eq!(ScwZone::Amsterdam1.region(), ScwRegion::Amsterdam);
        assert_eq!(ScwZone::Warsaw1.region(), ScwRegion::Warsaw);
    }
}
