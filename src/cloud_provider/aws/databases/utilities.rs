pub fn rds_name_sanitizer(max_size: usize, prefix: &str, name: &str) -> String {
    let max_size = max_size - prefix.len();
    let mut new_name = format!("{}-{}", prefix, name.replace("_", "").replace("-", ""));
    if new_name.clone().chars().count() > max_size {
        new_name = new_name[..max_size].to_string();
    }
    new_name
}
