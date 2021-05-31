use crypto::digest::Digest;
use crypto::sha1::Sha1;

pub fn to_sha1(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.input_str(input);
    hasher.result_str()
}

pub fn to_sha1_truncate_16(input: &str) -> String {
    let mut hash_str = to_sha1(input);
    hash_str.truncate(16);
    hash_str
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCase<'a> {
        input: &'a str,
        expected_output: String,
        description: &'a str,
    }

    #[test]
    fn test_to_sha1() {
        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: "",
                expected_output: String::from("da39a3ee5e6b4b0d3255bfef95601890afd80709"),
                description: "empty &str input",
            },
            TestCase {
                input: "abc",
                expected_output: String::from("a9993e364706816aba3e25717850c26c9cd0d89d"),
                description: "simple small input 1",
            },
            TestCase {
                input: "abcdefghijklmnopqrstuvwxyz",
                expected_output: String::from("32d10c7b8cf96570ca04ce37f2a19d84240d3a89"),
                description: "simple small input 1",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = to_sha1(tc.input);

            // verify:
            assert_eq!(tc.expected_output, result, "case {} : '{}'", tc.description, tc.input);
        }
    }

    #[test]
    fn test_to_sha1_truncate_16() {
        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                input: "",
                expected_output: String::from("da39a3ee5e6b4b0d"),
                description: "empty &str input",
            },
            TestCase {
                input: "abc",
                expected_output: String::from("a9993e364706816a"),
                description: "simple small input 1",
            },
            TestCase {
                input: "abcdefghijklmnopqrstuvwxyz",
                expected_output: String::from("32d10c7b8cf96570"),
                description: "simple small input 1",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = to_sha1_truncate_16(tc.input);

            // verify:
            assert_eq!(tc.expected_output, result, "case {} : '{}'", tc.description, tc.input);
        }
    }
}
