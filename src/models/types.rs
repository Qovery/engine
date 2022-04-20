use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Write;
use std::str::FromStr;

use crate::cloud_provider::DeploymentTarget;
use crate::errors::{CommandError, EngineError};
use tera::Context as TeraContext;

// Those types are just marker types that are use to tag our struct/object model
pub struct AWS {}
pub struct DO {}
pub struct SCW {}

// CloudProvider trait allows to derive all the custom type we need per provider,
// with our marker type defined above to be able to select the correct one
pub trait CloudProvider {
    type AppExtraSettings;
    type DbExtraSettings;
    type RouterExtraSettings;
    type StorageTypes;

    fn short_name() -> &'static str;
    fn full_name() -> &'static str;
    fn registry_short_name() -> &'static str;
    fn registry_full_name() -> &'static str;
    fn lib_directory_name() -> &'static str;
}

pub trait ToTeraContext {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError>;
}

// unfortunately some proposed versions are not SemVer like Elasticache (6.x)
// this is why we need ot have our own structure
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct VersionsNumber {
    pub(crate) major: String,
    pub(crate) minor: Option<String>,
    pub(crate) patch: Option<String>,
    pub(crate) suffix: Option<String>,
}

impl VersionsNumber {
    pub fn new(major: String, minor: Option<String>, patch: Option<String>, suffix: Option<String>) -> Self {
        VersionsNumber {
            major,
            minor,
            patch,
            suffix,
        }
    }

    pub fn to_major_version_string(&self) -> String {
        self.major.clone()
    }

    pub fn to_major_minor_version_string(&self, default_minor: &str) -> String {
        let test = format!(
            "{}.{}",
            self.major.clone(),
            self.minor.as_ref().unwrap_or(&default_minor.to_string())
        );

        test
    }
}

impl FromStr for VersionsNumber {
    type Err = CommandError;

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        if version.trim() == "" {
            return Err(CommandError::new_from_safe_message("version cannot be empty".to_string()));
        }

        let mut version_split = version.splitn(4, '.').map(|v| v.trim());

        let major = match version_split.next() {
            Some(major) => {
                let major = major.to_string();
                major.replace('v', "")
            }
            None => {
                return Err(CommandError::new_from_safe_message(format!(
                    "please check the version you've sent ({}), it can't be checked",
                    version
                )))
            }
        };

        let minor = version_split.next().map(|minor| {
            let minor = minor.to_string();
            minor.replace('+', "")
        });

        let patch = version_split.next().map(|patch| patch.to_string());

        let suffix = version_split.next().map(|suffix| suffix.to_string());

        // TODO(benjaminch): Handle properly the case where versions are empty
        // eq. 1..2

        Ok(VersionsNumber::new(major, minor, patch, suffix))
    }
}

impl fmt::Display for VersionsNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.major)?;

        if let Some(minor) = &self.minor {
            f.write_char('.')?;
            f.write_str(minor)?;
        }

        if let Some(patch) = &self.patch {
            f.write_char('.')?;
            f.write_str(patch)?;
        }

        if let Some(suffix) = &self.suffix {
            f.write_char('.')?;
            f.write_str(suffix)?;
        }

        Ok(())
    }
}
