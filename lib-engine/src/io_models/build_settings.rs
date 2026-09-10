use serde::{Deserialize, Serialize};

fn default_timeout_max_sec() -> u32 {
    30 * 60
}
fn default_cpu_max_in_milli() -> u32 {
    4000
}
fn default_ram_max_in_gib() -> u32 {
    8
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BuildSettings {
    #[serde(default = "default_timeout_max_sec")]
    pub timeout_max_sec: u32,
    #[serde(default = "default_cpu_max_in_milli")]
    pub cpu_max_in_milli: u32,
    #[serde(default = "default_ram_max_in_gib")]
    pub ram_max_in_gib: u32,
    #[serde(default)]
    pub ephemeral_storage_in_gib: Option<u32>,
    #[serde(default)]
    pub disable_buildkit_cache: bool,
    #[serde(default)]
    pub skip_git_submodules: bool,
}

impl Default for BuildSettings {
    fn default() -> Self {
        Self {
            timeout_max_sec: default_timeout_max_sec(),
            cpu_max_in_milli: default_cpu_max_in_milli(),
            ram_max_in_gib: default_ram_max_in_gib(),
            ephemeral_storage_in_gib: None,
            disable_buildkit_cache: false,
            skip_git_submodules: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- BuildSettings serde tests ---

    #[test]
    fn empty_json_object_deserializes_to_defaults() {
        let bs: BuildSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(bs.timeout_max_sec, 1800);
        assert_eq!(bs.cpu_max_in_milli, 4000);
        assert_eq!(bs.ram_max_in_gib, 8);
        assert_eq!(bs.ephemeral_storage_in_gib, None);
        assert!(!bs.disable_buildkit_cache);
        assert!(!bs.skip_git_submodules);
    }

    #[test]
    fn deserializes_all_fields_when_present() {
        let json = r#"{
            "timeout_max_sec": 3600,
            "cpu_max_in_milli": 8000,
            "ram_max_in_gib": 16,
            "ephemeral_storage_in_gib": 50,
            "disable_buildkit_cache": true,
            "skip_git_submodules": true
        }"#;
        let bs: BuildSettings = serde_json::from_str(json).unwrap();
        assert_eq!(bs.timeout_max_sec, 3600);
        assert_eq!(bs.cpu_max_in_milli, 8000);
        assert_eq!(bs.ram_max_in_gib, 16);
        assert_eq!(bs.ephemeral_storage_in_gib, Some(50));
        assert!(bs.disable_buildkit_cache);
        assert!(bs.skip_git_submodules);
    }

    #[test]
    fn partial_fields_use_defaults_for_missing() {
        let json = r#"{"timeout_max_sec": 999}"#;
        let bs: BuildSettings = serde_json::from_str(json).unwrap();
        assert_eq!(bs.timeout_max_sec, 999);
        assert_eq!(bs.cpu_max_in_milli, 4000); // default
        assert_eq!(bs.ram_max_in_gib, 8); // default
        assert_eq!(bs.ephemeral_storage_in_gib, None); // default
        assert!(!bs.disable_buildkit_cache); // default
        assert!(!bs.skip_git_submodules); // default
    }

    #[test]
    fn zero_values_are_preserved_not_replaced_by_defaults() {
        let json = r#"{"timeout_max_sec": 0, "cpu_max_in_milli": 0, "ram_max_in_gib": 0}"#;
        let bs: BuildSettings = serde_json::from_str(json).unwrap();
        assert_eq!(bs.timeout_max_sec, 0);
        assert_eq!(bs.cpu_max_in_milli, 0);
        assert_eq!(bs.ram_max_in_gib, 0);
    }

    #[test]
    fn default_impl_matches_serde_defaults() {
        let from_default = BuildSettings::default();
        let from_serde: BuildSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(from_default, from_serde);
    }

    #[test]
    fn option_build_settings_none_when_absent() {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            #[serde(default)]
            build_settings: Option<BuildSettings>,
        }
        let w: Wrapper = serde_json::from_str("{}").unwrap();
        assert!(w.build_settings.is_none());
    }

    #[test]
    fn option_build_settings_some_when_empty_object() {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            #[serde(default)]
            build_settings: Option<BuildSettings>,
        }
        let w: Wrapper = serde_json::from_str(r#"{"build_settings": {}}"#).unwrap();
        assert!(w.build_settings.is_some());
        assert_eq!(w.build_settings.unwrap(), BuildSettings::default());
    }

    #[test]
    fn option_build_settings_null_is_none() {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            #[serde(default)]
            build_settings: Option<BuildSettings>,
        }
        let w: Wrapper = serde_json::from_str(r#"{"build_settings": null}"#).unwrap();
        assert!(w.build_settings.is_none());
    }

    #[test]
    fn roundtrip_serialization() {
        let bs = BuildSettings {
            timeout_max_sec: 3600,
            cpu_max_in_milli: 8000,
            ram_max_in_gib: 16,
            ephemeral_storage_in_gib: Some(50),
            disable_buildkit_cache: true,
            skip_git_submodules: true,
        };
        let json = serde_json::to_string(&bs).unwrap();
        let deserialized: BuildSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(bs, deserialized);
    }
}
