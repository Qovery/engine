// this fn should implements the algorithm describe here: https://qovery.atlassian.net/secure/RapidBoard.jspa?rapidView=10&modal=detail&selectedIssue=DEV-283
pub fn get_firsts_namespaces_to_delete(namespaces: Vec<&str>) -> Vec<&str> {
    // from all namespaces remove managed and never delete namespaces
    namespaces
        .into_iter()
        .filter(|item| !get_qovery_managed_namespaces().contains(item))
        .filter(|item| !get_never_delete_namespaces().contains(item))
        .collect()
}

pub fn get_qovery_managed_namespaces() -> &'static [&'static str] {
    // order is very important because of dependencies
    &["logging", "nginx-ingress", "qovery", "cert-manager", "prometheus"]
}

fn get_never_delete_namespaces() -> &'static [&'static str] {
    &["default", "kube-node-lease", "kube-public", "kube-system"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_never_delete_namespaces() {
        // setup:
        let expected = vec!["default", "kube-node-lease", "kube-public", "kube-system"];

        // execute:
        let result = get_never_delete_namespaces();

        // verify:
        assert_eq!(expected, result);
    }

    #[test]
    fn test_get_qovery_managed_namespaces() {
        // setup:
        let expected = vec!["logging", "nginx-ingress", "qovery", "cert-manager", "prometheus"];

        // execute:
        let result = get_qovery_managed_namespaces();

        // verify:
        assert_eq!(expected, result);
    }

    #[test]
    fn test_get_firsts_namespaces_to_delete() {
        // setup:
        struct TestCase<'a> {
            input: Vec<&'a str>,
            expected_output: Vec<&'a str>,
            description: &'a str,
        }

        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: Vec::new(),
                expected_output: Vec::new(),
                description: "empty vec",
            },
            TestCase {
                input: vec!["a", "b", "c", "d"],
                expected_output: vec!["a", "b", "c", "d"],
                description: "everything can be deleted",
            },
            TestCase {
                input: vec![
                    "a",
                    "b",
                    "c",
                    "d",
                    "default",
                    "kube-node-lease",
                    "kube-public",
                    "kube-system",
                ],
                expected_output: vec!["a", "b", "c", "d"],
                description: "multiple elements among never to be deleted list",
            },
            TestCase {
                input: vec!["a", "b", "c", "d", "kube-system"],
                expected_output: vec!["a", "b", "c", "d"],
                description: "one element among never to be deleted list",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = get_firsts_namespaces_to_delete(tc.input.clone());

            // verify:
            assert_eq!(
                tc.expected_output,
                result,
                "case: {}, all: {:?} never_delete: {:?}",
                tc.description,
                tc.input,
                get_never_delete_namespaces()
            );
        }
    }
}
