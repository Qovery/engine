pub fn cut(str: String, max_length: usize) -> String {
    if str.len() <= max_length {
        str
    } else {
        str.as_str()[..max_length - 1].to_string()
    }
}

pub fn terraform_list_format(tf_vec: Vec<String>) -> String {
    format!("{{{}}}", tf_vec.join(","))
}
