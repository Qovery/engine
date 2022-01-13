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
            let x = arg_value.split("=").collect::<Vec<&str>>();
            x.get(0).unwrap_or(&"").to_string()
        })
        .collect::<HashSet<String>>();

    Ok(used_args)
}

/// Return env var args that are really used in the Dockerfile
/// env_var_args is a vector of value "key=value".
/// which is the format of the value expected by docker with the argument "build-arg"
pub fn match_used_env_var_args(
    env_var_args: Vec<String>,
    dockerfile_content: Vec<u8>,
) -> Result<Vec<String>, Utf8Error> {
    // extract env vars used in the Dockerfile
    let used_args = extract_dockerfile_args(dockerfile_content)?;

    // match env var args and dockerfile env vargs
    let env_var_arg_keys = env_var_args
        .iter()
        .map(|env_var| env_var.split("=").next().unwrap_or(&"").to_string())
        .collect::<HashSet<String>>();

    let matched_env_args_keys = env_var_arg_keys
        .intersection(&used_args)
        .map(|arg| arg.clone())
        .collect::<HashSet<String>>();

    Ok(env_var_args
        .into_iter()
        .filter(|env_var_arg| {
            let env_var_arg_key = env_var_arg.split("=").next().unwrap_or("");
            matched_env_args_keys.contains(env_var_arg_key)
        })
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

        let env_var_args_to_match = vec![
            "foo=abcdvalue".to_string(),
            "bar=abcdvalue".to_string(),
            "toto=abcdvalue".to_string(),
            "x=abcdvalue".to_string(),
        ];

        let matched_vars = match_used_env_var_args(env_var_args_to_match.clone(), dockerfile.to_vec());

        assert_eq!(matched_vars.clone().unwrap(), env_var_args_to_match.clone());

        assert_eq!(matched_vars.unwrap().len(), 4);

        let matched_vars = match_used_env_var_args(
            vec!["toto=abcdvalue".to_string(), "x=abcdvalue".to_string()],
            dockerfile.to_vec(),
        );

        assert_eq!(matched_vars.unwrap().len(), 2);

        let matched_vars = match_used_env_var_args(vec![], dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 0);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let matched_vars = match_used_env_var_args(env_var_args_to_match.clone(), dockerfile.to_vec());

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

        let matched_vars = match_used_env_var_args(
            vec![
                "PRISMIC_REPO_NAME=abcdvalue".to_string(),
                "PRISMIC_API_KEY=abcdvalue".to_string(),
                "PRISMIC_CUSTOM_TYPES_API_TOKEN=abcdvalue".to_string(),
            ],
            dockerfile.to_vec(),
        );

        assert_eq!(matched_vars.unwrap().len(), 3);

        let matched_vars =
            match_used_env_var_args(vec!["PRISMIC_REPO_NAME=abcdvalue".to_string()], dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 1);

        let matched_vars = match_used_env_var_args(vec![], dockerfile.to_vec());

        assert_eq!(matched_vars.unwrap().len(), 0);
    }
}
