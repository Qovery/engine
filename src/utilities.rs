use reqwest::header;
use reqwest::header::{HeaderMap, HeaderValue};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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
