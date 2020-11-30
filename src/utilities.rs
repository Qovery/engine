use crate::error::StringError;
use core::option::Option::{None, Some};
use core::result::Result;
use core::result::Result::{Err, Ok};

// unfortunately some proposed versions are not SemVer like Elasticache (6.x)
// this is why we need ot have our own structure
pub struct VersionsNumber {
    pub(crate) major: String,
    pub(crate) minor: Option<String>,
    pub(crate) patch: Option<String>,
}

pub fn get_version_number(version: &str) -> Result<VersionsNumber, StringError> {
    let mut version_split = version.split(".");

    let major = match version_split.next() {
        Some(major) => major.to_string(),
        _ => {
            return Err(StringError::new(
                "please check the version you've sent, it can't be checked".to_string(),
            ))
        }
    };

    let minor = match version_split.next() {
        Some(minor) => Some(minor.to_string()),
        _ => None,
    };

    let patch = match version_split.next() {
        Some(patch) => Some(patch.to_string()),
        _ => None,
    };

    Ok(VersionsNumber {
        major,
        minor,
        patch,
    })
}
