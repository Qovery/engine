pub fn cut(str: String, max_length: usize) -> String {
    if str.len() <= max_length {
        str
    } else {
        str.as_str()[..max_length - 1].to_string()
    }
}
