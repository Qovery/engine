use std::collections::HashSet;
use std::iter::FromIterator;
use std::str::Utf8Error;

/// Extract ARG value from a Dockerfile content
/// E.g
/// ```dockerfile
/// FROM node
///
/// ARG FOO
/// ARG BAR=default
/// ...
/// ```
///
/// will return a vector of "foo" and "bar" stings
pub fn extract_dockerfile_args(dockerfile_content: Vec<u8>) -> Result<HashSet<String>, Utf8Error> {
    let lines = std::str::from_utf8(dockerfile_content.as_slice())?;
    let lines = lines.lines();

    let used_args = lines
        .into_iter()
        .filter(|line| line.to_uppercase().starts_with("ARG "))
        .map(|line| {
            let x = line.split_whitespace().collect::<Vec<&str>>();
            x.get(1).unwrap_or(&"").to_string()
        })
        .map(|arg_value| {
            let x = arg_value.split("=").collect::<Vec<&str>>();
            x.get(0).unwrap_or(&"").to_string()
        })
        .collect::<HashSet<String>>();

    Ok(used_args)
}

/// Return env var args that are really used in the Dockerfile
pub fn match_used_env_var_args(
    env_var_args: Vec<String>,
    dockerfile_content: Vec<u8>,
) -> Result<Vec<String>, Utf8Error> {
    // extract env vars used in the Dockerfile
    let used_args = extract_dockerfile_args(dockerfile_content)?;

    // match env var args and dockerfile env vargs
    Ok(HashSet::from_iter(env_var_args)
        .intersection(&used_args)
        .map(|arg| arg.clone())
        .collect::<Vec<String>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dockerfile_args() {
        let dockerfile = b"
        FROM node

        ARG foo
        ARG bar=value
         ARG toto

        COPY . .
        ARGUMENT fake
        ARG x
        RUN ls -lh
        ";

        let res = extract_dockerfile_args(dockerfile.to_vec());
        assert_eq!(res.unwrap().len(), 4);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let res = extract_dockerfile_args(dockerfile.to_vec());
        assert_eq!(res.unwrap().len(), 0);
    }

    #[test]
    fn test_match_used_env_var_args() {
        let dockerfile = b"
        FROM node

        ARG foo
        ARG bar=value
         ARG toto

        COPY . .
        ARGUMENT fake
        ARG x
        RUN ls -lh
        ";

        let res = extract_dockerfile_args(dockerfile.to_vec());
        assert_eq!(res.unwrap().len(), 4);

        let matched_vars = match_used_env_var_args(
            vec![
                "foo".to_string(),
                "bar".to_string(),
                "toto".to_string(),
                "x".to_string(),
            ],
            dockerfile.to_vec(),
        );

        assert_eq!(matched_vars.unwrap().len(), 4);

        let matched_vars = match_used_env_var_args(vec!["toto".to_string(), "x".to_string()], dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 2);

        let matched_vars = match_used_env_var_args(vec![], dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 0);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let matched_vars = match_used_env_var_args(
            vec![
                "foo".to_string(),
                "bar".to_string(),
                "toto".to_string(),
                "x".to_string(),
            ],
            dockerfile.to_vec(),
        );

        assert_eq!(matched_vars.unwrap().len(), 0);
    }
}
