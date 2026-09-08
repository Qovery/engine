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
pub fn extract_dockerfile_args(dockerfile_content: &[u8]) -> Result<HashSet<String>, Utf8Error> {
    let lines = std::str::from_utf8(dockerfile_content)?;
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
            x.first().unwrap_or(&"").to_string()
        })
        .collect::<HashSet<String>>();

    Ok(used_args)
}

/// Secret mounts declared by `RUN --mount=type=secret,...` in a Dockerfile.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct DockerfileSecretMounts {
    /// The literal `id=` values carried by the `type=secret` mounts.
    pub ids: HashSet<String>,

    /// Set when at least one `type=secret` mount carries no literal `id=`.
    ///
    /// BuildKit would fall back to the basename of `target`, so `target=/etc/npmrc` would look for
    /// a secret named `npmrc`. Qovery does not guess: a derived name is almost never the variable
    /// name the author had in mind, and a mount that cannot be wired is better rejected than
    /// silently matched. The caller turns this into a build error.
    pub has_mount_without_id: bool,
}

/// Extract the secret mounts of a Dockerfile.
/// E.g
/// ```dockerfile
/// FROM node
///
/// RUN --mount=type=secret,id=NPM_TOKEN \
///     --mount=type=cache,target=/root/.npm \
///     npm ci
/// ```
///
/// will return `NPM_TOKEN` as the only secret mount id.
pub fn extract_dockerfile_secret_mounts(dockerfile_content: &[u8]) -> Result<DockerfileSecretMounts, Utf8Error> {
    let content = std::str::from_utf8(dockerfile_content)?;
    let mut secret_mounts = DockerfileSecretMounts::default();

    for instruction in logical_lines(content) {
        let mut words = instruction.split_whitespace();

        // `--mount` only exists on RUN, and only before the command it runs.
        match words.next() {
            Some(keyword) if keyword.eq_ignore_ascii_case("RUN") => {}
            _ => continue,
        }

        for word in words.take_while(|word| word.starts_with("--")) {
            let Some(options) = word.strip_prefix("--mount=") else {
                continue;
            };
            match parse_secret_mount(options) {
                SecretMount::NotASecret => {}
                SecretMount::WithId(id) => {
                    secret_mounts.ids.insert(id);
                }
                SecretMount::WithoutId => secret_mounts.has_mount_without_id = true,
            }
        }
    }

    Ok(secret_mounts)
}

enum SecretMount {
    NotASecret,
    WithId(String),
    WithoutId,
}

/// Parse the comma-separated options of a single `--mount=` flag.
fn parse_secret_mount(options: &str) -> SecretMount {
    let mut is_secret = false;
    let mut id = None;

    // Options come in an arbitrary order, so `type` may be read after `id`.
    for option in options.split(',') {
        let Some((key, value)) = option.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        match key.trim() {
            key if key.eq_ignore_ascii_case("type") => is_secret = value.eq_ignore_ascii_case("secret"),
            key if key.eq_ignore_ascii_case("id") && !value.is_empty() => id = Some(value.to_string()),
            _ => {}
        }
    }

    match (is_secret, id) {
        (false, _) => SecretMount::NotASecret,
        (true, Some(id)) => SecretMount::WithId(id),
        (true, None) => SecretMount::WithoutId,
    }
}

/// Rejoin the `\` continuations of a Dockerfile so that one item is one instruction.
///
/// Comment lines are dropped before joining, because Docker strips them first: a comment sitting
/// between two continuation lines does not end the instruction.
fn logical_lines(content: &str) -> Vec<String> {
    let mut instructions = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }

        if let Some(head) = line.strip_suffix('\\') {
            current.push_str(head.trim_end());
            current.push(' ');
            continue;
        }

        current.push_str(line);
        if !current.trim().is_empty() {
            instructions.push(std::mem::take(&mut current));
        }
        current.clear();
    }

    if !current.trim().is_empty() {
        instructions.push(current);
    }

    instructions
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

        let res = extract_dockerfile_args(dockerfile);
        assert_eq!(res.unwrap().len(), 4);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let res = extract_dockerfile_args(dockerfile);
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

        let res = extract_dockerfile_args(dockerfile);
        assert_eq!(res.unwrap().len(), 4);

        let args = btreemap![
            "foo" => "abcdvalue",
            "bar" => "abcdvalue",
            "toto" => "abcdvalue",
            "x" => "abcdvalue",
        ];

        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret, args);

        let args = btreemap!["toto" => "abcdvalue", "x" => "abcdvalue"];
        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 2);

        let args: BTreeMap<&str, &str> = btreemap![];
        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 0);

        let dockerfile = b"
        FROM node

        COPY . .
        RUN ls -lh
        ";

        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
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

        let res = extract_dockerfile_args(dockerfile);
        assert_eq!(res.unwrap().len(), 3);

        let args = btreemap![
            "PRISMIC_REPO_NAME" => "abcdvalue",
            "PRISMIC_API_KEY" => "abcdvalue",
            "PRISMIC_CUSTOM_TYPES_API_TOKEN" => "abcdvalue",
        ];
        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 3);

        let args = btreemap!["PRISMIC_REPO_NAME" => "abcdvalue"];
        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 1);

        let args: BTreeMap<&str, &str> = btreemap![];
        let matched_vars = extract_dockerfile_args(dockerfile).unwrap();
        let mut ret = args.clone();
        ret.retain(|k, _| matched_vars.contains(*k));
        assert_eq!(ret.len(), 0);
    }

    fn secret_ids(dockerfile: &[u8]) -> HashSet<String> {
        extract_dockerfile_secret_mounts(dockerfile).unwrap().ids
    }

    #[test]
    fn test_extract_secret_mount_on_a_single_line() {
        let dockerfile = b"
        FROM node

        RUN --mount=type=secret,id=NPM_TOKEN npm ci
        ";

        assert_eq!(secret_ids(dockerfile), HashSet::from(["NPM_TOKEN".to_string()]));
    }

    #[test]
    fn test_extract_secret_mount_spread_over_continuation_lines() {
        let dockerfile = b"
        FROM node

        RUN --mount=type=secret,id=NPM_TOKEN \\
            --mount=type=cache,target=/root/.npm \\
            --mount=type=secret,id=SENTRY_TOKEN \\
            npm ci
        ";

        assert_eq!(
            secret_ids(dockerfile),
            HashSet::from(["NPM_TOKEN".to_string(), "SENTRY_TOKEN".to_string()])
        );
    }

    #[test]
    fn test_extract_secret_mount_ignores_a_comment_between_continuation_lines() {
        let dockerfile = b"
        FROM node

        RUN --mount=type=secret,id=NPM_TOKEN \\
        # the private registry credentials
            --mount=type=secret,id=SENTRY_TOKEN \\
            npm ci
        ";

        assert_eq!(
            secret_ids(dockerfile),
            HashSet::from(["NPM_TOKEN".to_string(), "SENTRY_TOKEN".to_string()])
        );
    }

    #[test]
    fn test_extract_secret_mount_accepts_any_option_order_and_quotes() {
        let dockerfile = b"
        FROM node

        RUN --mount=required=true,target=/etc/npmrc,id=\"NPM_TOKEN\",type=secret npm ci
        ";

        assert_eq!(secret_ids(dockerfile), HashSet::from(["NPM_TOKEN".to_string()]));
    }

    #[test]
    fn test_extract_secret_mount_is_case_insensitive_on_keywords_only() {
        let dockerfile = b"
        FROM node

        run --mount=TYPE=Secret,ID=NPM_TOKEN npm ci
        ";

        assert_eq!(secret_ids(dockerfile), HashSet::from(["NPM_TOKEN".to_string()]));
    }

    #[test]
    fn test_extract_secret_mount_ignores_other_mount_types() {
        let dockerfile = b"
        FROM node

        RUN --mount=type=cache,target=/root/.npm \\
            --mount=type=bind,source=/src,target=/dst \\
            --mount=type=ssh,id=github \\
            --mount=type=tmpfs,target=/tmp \\
            npm ci
        ";

        let mounts = extract_dockerfile_secret_mounts(dockerfile).unwrap();
        assert!(mounts.ids.is_empty());
        assert!(!mounts.has_mount_without_id);
    }

    #[test]
    fn test_extract_secret_mount_ignores_comments_and_non_run_instructions() {
        let dockerfile = b"
        FROM node

        # RUN --mount=type=secret,id=COMMENTED_OUT npm ci
        COPY --mount=type=secret,id=NOT_A_RUN . .
        RUN echo hello
        ";

        let mounts = extract_dockerfile_secret_mounts(dockerfile).unwrap();
        assert!(mounts.ids.is_empty());
        assert!(!mounts.has_mount_without_id);
    }

    #[test]
    fn test_extract_secret_mount_ignores_a_mount_flag_that_belongs_to_the_command() {
        let dockerfile = b"
        FROM node

        RUN my-tool --mount=type=secret,id=NOT_A_DOCKER_FLAG
        ";

        let mounts = extract_dockerfile_secret_mounts(dockerfile).unwrap();
        assert!(mounts.ids.is_empty());
        assert!(!mounts.has_mount_without_id);
    }

    #[test]
    fn test_extract_secret_mount_flags_a_mount_without_id() {
        let dockerfile = b"
        FROM node

        RUN --mount=type=secret,target=/etc/npmrc npm ci
        ";

        let mounts = extract_dockerfile_secret_mounts(dockerfile).unwrap();
        assert!(mounts.ids.is_empty());
        assert!(mounts.has_mount_without_id);

        // An empty id is as unwirable as a missing one.
        let dockerfile = b"
        FROM node

        RUN --mount=type=secret,id= npm ci
        ";

        assert!(
            extract_dockerfile_secret_mounts(dockerfile)
                .unwrap()
                .has_mount_without_id
        );
    }

    #[test]
    fn test_extract_secret_mount_reports_both_a_wired_and_an_unwirable_mount() {
        let dockerfile = b"
        FROM node

        RUN --mount=type=secret,id=NPM_TOKEN \\
            --mount=type=secret,target=/etc/npmrc \\
            npm ci
        ";

        let mounts = extract_dockerfile_secret_mounts(dockerfile).unwrap();
        assert_eq!(mounts.ids, HashSet::from(["NPM_TOKEN".to_string()]));
        assert!(mounts.has_mount_without_id);
    }

    #[test]
    fn test_extract_secret_mount_on_a_dockerfile_without_any() {
        let dockerfile = b"
        FROM node

        ARG FOO
        COPY . .
        RUN ls -lh
        ";

        let mounts = extract_dockerfile_secret_mounts(dockerfile).unwrap();
        assert_eq!(mounts, DockerfileSecretMounts::default());
    }
}
