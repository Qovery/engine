use std::collections::HashSet;
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
        .filter(|line| line.to_uppercase().trim().starts_with("ARG "))
        .map(|line| {
            let x = line.split_whitespace().collect::<Vec<&str>>();
            x.get(1).unwrap_or(&"").to_string()
        })
        .map(|arg_value| {
            let x = arg_value.split('=').collect::<Vec<&str>>();
            x.get(0).unwrap_or(&"").to_string()
        })
        .collect::<HashSet<String>>();

    Ok(used_args)
}

/// Return env var args that are really used in the Dockerfile
/// env_var_args is a vector of value "key=value".
/// which is the format of the value expected by docker with the argument "build-arg"
pub fn match_used_env_var_args<'a>(
    env_var_args: &'a [(&'a str, &'a str)],
    dockerfile_content: Vec<u8>,
) -> Result<Vec<(&'a str, &'a str)>, Utf8Error> {
    // extract env vars used in the Dockerfile
    let used_args = extract_dockerfile_args(dockerfile_content)?;

    let mut matched_env_args = env_var_args.to_vec();
    matched_env_args.retain(|(k, _v)| used_args.contains(*k));

    Ok(matched_env_args)
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

        let env_var_args_to_match = vec![
            ("foo", "abcdvalue"),
            ("bar", "abcdvalue"),
            ("toto", "abcdvalue"),
            ("x", "abcdvalue"),
        ];

        let matched_vars = match_used_env_var_args(&env_var_args_to_match, dockerfile.to_vec());

        assert_eq!(matched_vars.clone().unwrap(), env_var_args_to_match.clone());

        assert_eq!(matched_vars.unwrap().len(), 4);

        let args = vec![("toto", "abcdvalue"), ("x", "abcdvalue")];
        let matched_vars = match_used_env_var_args(&args, dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 2);

        let args = vec![];
        let matched_vars = match_used_env_var_args(&args, dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 0);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let matched_vars = match_used_env_var_args(&env_var_args_to_match, dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 0);
    }

    #[test]
    fn test_match_used_env_var_args_2() {
        let dockerfile = b"
        # This file is a template, and might need editing before it works on your project.
        FROM node:16-alpine as build

        WORKDIR /app
        COPY . .

            ARG PRISMIC_REPO_NAME
        ENV PRISMIC_REPO_NAME $PRISMIC_REPO_NAME

        ARG PRISMIC_API_KEY
        ENV PRISMIC_API_KEY $PRISMIC_API_KEY

        ARG PRISMIC_CUSTOM_TYPES_API_TOKEN
        ENV PRISMIC_CUSTOM_TYPES_API_TOKEN $PRISMIC_CUSTOM_TYPES_API_TOKEN

        RUN npm install && npm run build

        FROM nginx:latest
        COPY --from=build /app/public /usr/share/nginx/html
        COPY ./nginx-custom.conf /etc/nginx/conf.d/default.conf

        EXPOSE 80
        CMD [\"nginx\", \"-g\", \"daemon off;\"]
        ";

        let res = extract_dockerfile_args(dockerfile.to_vec());
        assert_eq!(res.unwrap().len(), 3);

        let args = vec![
            ("PRISMIC_REPO_NAME", "abcdvalue"),
            ("PRISMIC_API_KEY", "abcdvalue"),
            ("PRISMIC_CUSTOM_TYPES_API_TOKEN", "abcdvalue"),
        ];
        let matched_vars = match_used_env_var_args(&args, dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 3);

        let args = vec![("PRISMIC_REPO_NAME", "abcdvalue")];
        let matched_vars = match_used_env_var_args(&args, dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 1);

        let args = vec![];
        let matched_vars = match_used_env_var_args(&args, dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 0);
    }
}
