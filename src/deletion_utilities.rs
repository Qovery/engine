// this fn should implements the algorythm describe here: https://qovery.atlassian.net/secure/RapidBoard.jspa?rapidView=10&modal=detail&selectedIssue=DEV-283
pub fn get_firsts_namespaces_to_delete(namespaces: Vec<&str>) -> Vec<&str> {
    // from all namesapce remove managed and never delete namespaces
    let minus_managed = minus_namespaces(namespaces, get_qovery_managed_namespaces());
    let minus_qovery_managed_and_never_delete = minus_namespaces(minus_managed, get_never_delete_namespaces());
    minus_qovery_managed_and_never_delete
}

fn minus_namespaces<'a>(all: Vec<&'a str>, to_remove_namespaces: Vec<&str>) -> Vec<&'a str> {
    all.into_iter()
        .filter(|item| !to_remove_namespaces.contains(item))
        .collect()
}

pub fn get_qovery_managed_namespaces() -> Vec<&'static str> {
    // order is very important because of dependencies
    vec!["logging", "nginx-ingress", "qovery", "cert-manager", "prometheus"]
}

fn get_never_delete_namespaces() -> Vec<&'static str> {
    vec!["default", "kube-node-lease", "kube-public", "kube-system"]
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
    fn test_minus_namespaces() {
        // setup:
        struct TestCase<'a> {
            input_all: Vec<&'a str>,
            input_to_be_removed: Vec<&'a str>,
            expected_output: Vec<&'a str>,
            description: &'a str,
        }

        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input_all: Vec::new(),
                input_to_be_removed: Vec::new(),
                expected_output: Vec::new(),
                description: "empty vec, nothing to be deleted",
            },
            TestCase {
                input_all: Vec::new(),
                input_to_be_removed: vec!["a", "c"],
                expected_output: Vec::new(),
                description: "empty vec, non empty vec to be deleted",
            },
            TestCase {
                input_all: vec!["a", "b", "c", "d"],
                input_to_be_removed: Vec::new(),
                expected_output: vec!["a", "b", "c", "d"],
                description: "nothing to be deleted",
            },
            TestCase {
                input_all: vec!["a", "b", "c", "d"],
                input_to_be_removed: vec!["b", "c"],
                expected_output: vec!["a", "d"],
                description: "nominal 1",
            },
            TestCase {
                input_all: vec!["a", "b", "c", "d"],
                input_to_be_removed: vec!["a", "d"],
                expected_output: vec!["b", "c"],
                description: "nominal 2",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = minus_namespaces(tc.input_all.clone(), tc.input_to_be_removed.clone());

            // verify:
            assert_eq!(
                tc.expected_output, result,
                "case: {}, all: {:?} to_be_removed: {:?}",
                tc.description, tc.input_all, tc.input_to_be_removed
            );
        }
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
