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

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::btreemap;
    use std::collections::BTreeMap;

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

        let args = btreemap![
            "foo" => "abcdvalue",
            "bar" => "abcdvalue",
            "toto" => "abcdvalue",
            "x" => "abcdvalue",
        ];

        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret, args);

        let args = btreemap!["toto" => "abcdvalue", "x" => "abcdvalue"];
        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 2);

        let args: BTreeMap<&str, &str> = btreemap![];
        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 0);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 0);
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

        let args = btreemap![
            "PRISMIC_REPO_NAME" => "abcdvalue",
            "PRISMIC_API_KEY" => "abcdvalue",
            "PRISMIC_CUSTOM_TYPES_API_TOKEN" => "abcdvalue",
        ];
        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 3);

        let args = btreemap!["PRISMIC_REPO_NAME" => "abcdvalue"];
        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 1);

        let args: BTreeMap<&str, &str> = btreemap![];
        let matched_vars = extract_dockerfile_args(dockerfile.to_vec()).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 0);
    }
}
