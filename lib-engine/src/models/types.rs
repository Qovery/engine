use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Write;
use std::str::FromStr;
use std::sync::Arc;

use crate::cloud_provider::{DeploymentTarget, Kind};
use crate::errors::{CommandError, EngineError};
use tera::Context as TeraContext;

// Those types are just marker types that are use to tag our struct/object model
pub struct AWS {}
pub struct AWSEc2 {}
pub struct SCW {}

// CloudProvider trait allows to derive all the custom type we need per provider,
// with our marker type defined above to be able to select the correct one
pub trait CloudProvider: Send + Sync {
    type AppExtraSettings: Send + Sync;
    type DbExtraSettings: Send + Sync;
    type RouterExtraSettings: Send + Sync;
    type StorageTypes: Send + Sync;

    fn cloud_provider() -> Kind;
    fn short_name() -> &'static str;
    fn full_name() -> &'static str;
    fn registry_short_name() -> &'static str;
    fn registry_full_name() -> &'static str;
    fn lib_directory_name() -> &'static str;
    fn loadbalancer_l4_annotations() -> &'static [(&'static str, &'static str)];
}

pub trait ToTeraContext {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>>;
}

// unfortunately some proposed versions are not SemVer like Elasticache (6.x)
// this is why we need ot have our own structure
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
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

    pub fn to_major_minor_version_string(&self, default_minor: String) -> String {
        let test = format!("{}.{}", self.major.clone(), self.minor.as_ref().unwrap_or(&default_minor));

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

        let major: Arc<str> = match version_split.next() {
            Some(major) => {
                let major = major.to_string();
                Arc::from(major.replace('v', ""))
            }
            None => {
                return Err(CommandError::new_from_safe_message(format!(
                    "please check the version you've sent ({version}), it can't be checked"
                )))
            }
        };

        let minor: Option<Arc<str>> = version_split.next().map(|minor| {
            let minor = minor.to_string();
            Arc::from(minor.replace('+', ""))
        });

        let patch: Option<Arc<str>> = version_split.next().map(|patch| Arc::from(patch.to_string()));

        let suffix: Option<Arc<str>> = version_split.next().map(|suffix| Arc::from(suffix.to_string()));

        // TODO(benjaminch): Handle properly the case where versions are empty
        // eq. 1..2

        Ok(VersionsNumber::new(
            major.to_string(),
            minor.map(|m| m.to_string()),
            patch.map(|p| p.to_string()),
            suffix.map(|s| s.to_string()),
        ))
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

pub struct VersionsNumberBuilder {
    major: Arc<str>,
    minor: Option<Arc<str>>,
    patch: Option<Arc<str>>,
    suffix: Option<Arc<str>>,
}

impl VersionsNumberBuilder {
    pub fn new() -> Self {
        VersionsNumberBuilder::default()
    }

    pub fn build(&self) -> VersionsNumber {
        VersionsNumber {
            major: self.major.to_string(),
            minor: self.minor.as_ref().map(|m| m.to_string()),
            patch: self.patch.as_ref().map(|p| p.to_string()),
            suffix: self.suffix.as_ref().map(|s| s.to_string()),
        }
    }

    pub fn major(mut self, major: u32) -> Self {
        self.major = Arc::from(major.to_string());
        self
    }

    pub fn major_str(mut self, major: Arc<str>) -> Self {
        self.major = major;
        self
    }

    pub fn minor(mut self, minor: u32) -> Self {
        self.minor = Some(Arc::from(minor.to_string()));
        self
    }

    pub fn minor_str(mut self, minor: Arc<str>) -> Self {
        self.minor = Some(minor);
        self
    }

    pub fn patch(mut self, patch: u32) -> Self {
        self.patch = Some(Arc::from(patch.to_string()));
        self
    }

    pub fn patch_str(mut self, patch: Arc<str>) -> Self {
        self.patch = Some(patch);
        self
    }

    pub fn suffix(mut self, suffix: Arc<str>) -> Self {
        self.suffix = Some(suffix);
        self
    }
}

impl Default for VersionsNumberBuilder {
    fn default() -> Self {
        VersionsNumberBuilder {
            major: Arc::from("0".to_string()),
            minor: None,
            patch: None,
            suffix: None,
        }
    }
}
