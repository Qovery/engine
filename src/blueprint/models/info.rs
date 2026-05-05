use std::fmt::Display;

use crate::blueprint::models::error::BlueprintError;

/// Parsed blueprint tag info.
/// Tag format: `{provider}/{service_name}/{service_version}/{manifest_version}`
#[derive(Debug)]
pub(in crate::blueprint::models::info) struct Tag {
    pub provider: String,
    pub service_name: String,
    pub service_version: String,
    pub manifest_version: String,
}

impl TryFrom<&str> for Tag {
    type Error = BlueprintError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let split: Vec<&str> = value.split('/').collect();
        if split.len() != 4 {
            return Err(BlueprintError::InvalidTagFormat);
        }

        Ok(Self {
            provider: split[0].into(),
            service_name: split[1].into(),
            service_version: split[2].into(),
            manifest_version: split[3].into(),
        })
    }
}

pub struct BlueprintInfo {
    tag: Tag,
}

impl BlueprintInfo {
    //Not using try_from for flexibility new fields will come.
    pub fn try_new(raw_tag: &str) -> Result<Self, BlueprintError> {
        let tag = Tag::try_from(raw_tag)?;
        Ok(BlueprintInfo { tag })
    }
    pub fn path(&self) -> String {
        format!("{}/{}/{}", self.tag.provider, self.tag.service_name, self.tag.service_version)
    }
}

impl Display for BlueprintInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "provider: {}, service: {}, service version: {}, manifest version: {}",
            self.tag.provider, self.tag.service_name, self.tag.service_version, self.tag.manifest_version
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::blueprint::models::error::BlueprintError;

    use super::*;

    #[test]
    fn tag_parses_raw_value() {
        let tag = Tag::try_from("helm/redis/7/2.1.0").unwrap();
        assert_eq!(tag.provider, "helm");
        assert_eq!(tag.service_name, "redis");
        assert_eq!(tag.service_version, "7");
        assert_eq!(tag.manifest_version, "2.1.0");
    }

    #[test]
    fn tag_rejects_too_few_segments() {
        let err = Tag::try_from("aws/s3/1.0.0").unwrap_err();
        assert_eq!(err, BlueprintError::InvalidTagFormat);
    }

    #[test]
    fn tag_rejects_too_many_segments() {
        let err = Tag::try_from("aws/s3/1/1.0.0/extra").unwrap_err();
        assert_eq!(err, BlueprintError::InvalidTagFormat);
    }

    #[test]
    fn path_excludes_manifest_version() {
        let info = BlueprintInfo::try_new("helm/redis/7/3.0.0").unwrap();
        assert_eq!(info.path(), "helm/redis/7");
    }

    #[test]
    fn display_contains_all_fields() {
        let info = BlueprintInfo::try_new("gcp/cloud-sql/15/1.2.3").unwrap();
        let display = format!("{}", info);
        assert!(display.contains("provider: gcp"));
        assert!(display.contains("service: cloud-sql"));
        assert!(display.contains("service version: 15"));
        assert!(display.contains("manifest version: 1.2.3"));
    }
}
