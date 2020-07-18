use crypto::digest::Digest;
use crypto::sha1::Sha1;

pub fn to_sha1(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.input_str(input);
    hasher.result_str()
}
