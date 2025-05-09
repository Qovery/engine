use base64::engine::general_purpose;
use base64::{DecodeError, Engine};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::infrastructure::models::build_platform::GitRepositoryExtraFile;
use reqwest::header::{HeaderMap, HeaderValue};
use uuid::Uuid;

// generate the right header for digital ocean with token
pub fn get_header_with_bearer(token: &str) -> HeaderMap<HeaderValue> {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("Authorization", format!("Bearer {token}").parse().unwrap());
    headers
}

pub fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub fn compute_image_tag<P: AsRef<Path> + Hash, T: AsRef<Path> + Hash>(
    root_path: P,
    dockerfile_path: &Option<T>,
    dockerfile_content: &Option<String>,
    docker_extra_files_to_inject: &[GitRepositoryExtraFile],
    environment_variables: &BTreeMap<String, String>,
    commit_id: &str,
    docker_target_build_stage: &Option<String>,
) -> String {
    // Image tag == hash(root_path) + commit_id truncate to 127 char
    // https://github.com/distribution/distribution/blob/6affafd1f030087d88f88841bf66a8abe2bf4d24/reference/regexp.go#L41
    let mut hasher = DefaultHasher::new();

    // If any of those variables changes, we'll get a new hash value, what results in a new image
    // build and avoids using cache. It is important to build a new image, as those variables may
    // affect the build result even if user didn't change his code.
    root_path.hash(&mut hasher);

    dockerfile_content.hash(&mut hasher);
    if dockerfile_path.is_some() {
        // only use when a Dockerfile is used to prevent build cache miss every single time
        // we redeploy an app with a env var changed with Buildpacks.
        dockerfile_path.hash(&mut hasher);
        environment_variables.hash(&mut hasher);
    }

    // Include docker_target_build_stage in the hash calculation only if it's Some
    if let Some(build_stage) = docker_target_build_stage {
        build_stage.hash(&mut hasher);
    }

    if !docker_extra_files_to_inject.is_empty() {
        docker_extra_files_to_inject.iter().for_each(|extra_file| {
            extra_file.content.hash(&mut hasher);
            extra_file.path.hash(&mut hasher);
        });
    }

    let mut tag = format!("{}-{}", hasher.finish(), commit_id);
    tag.truncate(127);

    tag
}

pub fn to_short_id(id: &Uuid) -> String {
    format!("z{}", id.to_string().split_at(8).0)
}

pub fn to_qovery_name(id: &Uuid) -> String {
    format!("qovery-{}", to_short_id(id))
}

pub fn base64_replace_comma_to_new_line(multiple_credentials: String) -> Result<String, DecodeError> {
    let decoded_value_byte = general_purpose::STANDARD.decode(multiple_credentials)?;
    let decoded_value = decoded_value_byte.iter().map(|c| *c as char).collect::<String>();
    let replaced_comma = decoded_value.replace(',', "\n");
    Ok(general_purpose::STANDARD.encode(replaced_comma))
}

pub fn envs_to_slice(env_var: &[(String, String)]) -> Vec<(&str, &str)> {
    env_var.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

pub fn envs_to_string(env_var: Vec<(&str, &str)>) -> Vec<(String, String)> {
    env_var
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests_utilities {
    use crate::infrastructure::models::build_platform::GitRepositoryExtraFile;
    use crate::utilities::{base64_replace_comma_to_new_line, compute_image_tag};
    use base64::Engine;
    use base64::engine::general_purpose;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn test_get_image_tag() {
        let image_tag = compute_image_tag(
            "/".to_string(),
            &Some("Dockerfile".to_string()),
            &None,
            &[],
            &BTreeMap::new(),
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );

        let image_tag_2 = compute_image_tag(
            "/".to_string(),
            &Some("Dockerfile.qovery".to_string()),
            &None,
            &[],
            &BTreeMap::new(),
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );

        assert_ne!(image_tag, image_tag_2);

        let image_tag_3 = compute_image_tag(
            "/xxx".to_string(),
            &Some("Dockerfile.qovery".to_string()),
            &None,
            &[],
            &BTreeMap::new(),
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );

        assert_ne!(image_tag, image_tag_3);

        let image_tag_3_2 = compute_image_tag(
            "/xxx".to_string(),
            &Some("Dockerfile.qovery".to_string()),
            &None,
            &[],
            &BTreeMap::new(),
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );

        assert_eq!(image_tag_3, image_tag_3_2);

        let image_tag_4 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &None,
            &[],
            &BTreeMap::new(),
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );

        let mut env_vars_5 = BTreeMap::new();
        env_vars_5.insert("toto".to_string(), "key".to_string());

        let image_tag_5 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &None,
            &[],
            &env_vars_5,
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );

        assert_eq!(image_tag_4, image_tag_5);

        let image_tag_5 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &Some("FROM my-custom-dockerfile".to_string()),
            &[],
            &env_vars_5,
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );
        assert_ne!(image_tag_4, image_tag_5);

        let image_tag_6 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &Some("FROM my-custom-dockerfile".to_string()),
            &[GitRepositoryExtraFile {
                path: PathBuf::from("/path"),
                content: "toto".to_string(),
            }],
            &env_vars_5,
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );
        assert_ne!(image_tag_5, image_tag_6);

        let image_tag_7 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &Some("FROM my-custom-dockerfile".to_string()),
            &[GitRepositoryExtraFile {
                path: PathBuf::from("/path"),
                content: "tata".to_string(),
            }],
            &env_vars_5,
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &None,
        );
        assert_ne!(image_tag_6, image_tag_7);

        // Test that the hash changes when the docker_target_build_stage changes
        let image_tag_8 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &Some("FROM my-custom-dockerfile".to_string()),
            &[],
            &env_vars_5,
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &Some("stage1".to_string()),
        );

        let image_tag_9 = compute_image_tag(
            "/".to_string(),
            &None as &Option<&str>,
            &Some("FROM my-custom-dockerfile".to_string()),
            &[],
            &env_vars_5,
            "63d8c437337416a7067d3f358197ac47d003fab9",
            &Some("stage2".to_string()),
        );

        assert_ne!(image_tag_8, image_tag_9);
    }

    #[test]
    pub fn test_comma_to_new_line_base64_replacement() {
        // check basic_auth vars replacement
        let env_var_base64 = general_purpose::STANDARD.encode(b"dennis:ritchie,linus:torvalds");
        let basic_auth_replacement = base64_replace_comma_to_new_line(env_var_base64).unwrap();
        let decoded_res = general_purpose::STANDARD.decode(basic_auth_replacement).unwrap();
        let decoded_res_string = decoded_res.iter().map(|c| *c as char).collect::<String>();
        assert_eq!(decoded_res_string, "dennis:ritchie\nlinus:torvalds".to_string());
    }
}
