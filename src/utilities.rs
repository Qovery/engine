use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use reqwest::header;
use reqwest::header::{HeaderMap, HeaderValue};

// generate the right header for digital ocean with token
pub fn get_header_with_bearer(token: &str) -> HeaderMap<HeaderValue> {
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("Authorization", format!("Bearer {}", token).parse().unwrap());
    headers
}

pub fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub fn get_image_tag(
    root_path: &String,
    dockerfile_path: &Option<String>,
    environment_variables: &BTreeMap<String, String>,
    commit_id: &String,
) -> String {
    // Image tag == hash(root_path) + commit_id truncate to 127 char
    // https://github.com/distribution/distribution/blob/6affafd1f030087d88f88841bf66a8abe2bf4d24/reference/regexp.go#L41
    let mut hasher = DefaultHasher::new();

    // If any of those variables changes, we'll get a new hash value, what results in a new image
    // build and avoids using cache. It is important to build a new image, as those variables may
    // affect the build result even if user didn't change his code.
    root_path.hash(&mut hasher);

    if dockerfile_path.is_some() {
        // only use when a Dockerfile is used to prevent build cache miss every single time
        // we redeploy an app with a env var changed with Buildpacks.
        dockerfile_path.hash(&mut hasher);

        // TODO check if the environment variables are used in the Dockerfile and only Hash the one that are used
        environment_variables.hash(&mut hasher);
    }

    let mut tag = format!("{}-{}", hasher.finish(), commit_id);
    tag.truncate(127);

    tag
}
