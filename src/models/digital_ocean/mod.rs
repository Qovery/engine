mod application;
mod database;
mod router;

use crate::errors::CommandError;
use crate::models::types::CloudProvider;
use crate::models::types::DO;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

pub struct DoAppExtraSettings {}
pub struct DoDbExtraSettings {}
pub struct DoRouterExtraSettings {}

impl CloudProvider for DO {
    type AppExtraSettings = DoAppExtraSettings;
    type DbExtraSettings = DoDbExtraSettings;
    type RouterExtraSettings = DoRouterExtraSettings;
    type StorageTypes = DoStorageType;

    fn short_name() -> &'static str {
        "DO"
    }

    fn full_name() -> &'static str {
        "Digital Ocean"
    }

    fn registry_short_name() -> &'static str {
        "DO CR"
    }

    fn registry_full_name() -> &'static str {
        "Digital Ocean Container Registry"
    }

    fn lib_directory_name() -> &'static str {
        "digitalocean"
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum DoStorageType {
    Standard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DoRegion {
    NewYorkCity1,
    NewYorkCity2,
    NewYorkCity3,
    Amsterdam2,
    Amsterdam3,
    SanFrancisco1,
    SanFrancisco2,
    SanFrancisco3,
    Singapore,
    London,
    Frankfurt,
    Toronto,
    Bangalore,
}

impl DoRegion {
    pub fn as_str(&self) -> &str {
        match self {
            DoRegion::NewYorkCity1 => "nyc1",
            DoRegion::NewYorkCity2 => "nyc2",
            DoRegion::NewYorkCity3 => "nyc3",
            DoRegion::Amsterdam2 => "ams2",
            DoRegion::Amsterdam3 => "ams3",
            DoRegion::SanFrancisco1 => "sfo1",
            DoRegion::SanFrancisco2 => "sfo2",
            DoRegion::SanFrancisco3 => "sfo3",
            DoRegion::Singapore => "sgp1",
            DoRegion::London => "lon1",
            DoRegion::Frankfurt => "fra1",
            DoRegion::Toronto => "tor1",
            DoRegion::Bangalore => "blr1",
        }
    }
}

impl Display for DoRegion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            DoRegion::NewYorkCity1 => write!(f, "nyc1"),
            DoRegion::NewYorkCity2 => write!(f, "nyc2"),
            DoRegion::NewYorkCity3 => write!(f, "nyc3"),
            DoRegion::Amsterdam2 => write!(f, "ams2"),
            DoRegion::Amsterdam3 => write!(f, "ams3"),
            DoRegion::SanFrancisco1 => write!(f, "sfo1"),
            DoRegion::SanFrancisco2 => write!(f, "sfo2"),
            DoRegion::SanFrancisco3 => write!(f, "sfo3"),
            DoRegion::Singapore => write!(f, "sgp1"),
            DoRegion::London => write!(f, "lon1"),
            DoRegion::Frankfurt => write!(f, "fra1"),
            DoRegion::Toronto => write!(f, "tor1"),
            DoRegion::Bangalore => write!(f, "blr1"),
        }
    }
}

impl FromStr for DoRegion {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<DoRegion, CommandError> {
        match s {
            "nyc1" => Ok(DoRegion::NewYorkCity1),
            "nyc2" => Ok(DoRegion::NewYorkCity2),
            "nyc3" => Ok(DoRegion::NewYorkCity3),
            "ams2" => Ok(DoRegion::Amsterdam2),
            "ams3" => Ok(DoRegion::Amsterdam3),
            "sfo1" => Ok(DoRegion::SanFrancisco1),
            "sfo2" => Ok(DoRegion::SanFrancisco2),
            "sfo3" => Ok(DoRegion::SanFrancisco3),
            "sgp1" => Ok(DoRegion::Singapore),
            "lon1" => Ok(DoRegion::London),
            "fra1" => Ok(DoRegion::Frankfurt),
            "tor1" => Ok(DoRegion::Toronto),
            "blr1" => Ok(DoRegion::Bangalore),
            _ => {
                return Err(CommandError::new_from_safe_message(format!("`{}` region is not supported", s)));
            }
        }
    }
}
