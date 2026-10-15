use crate::runtime::block_on;
use http::StatusCode;
use kube::Error;
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::thread;
use std::time::Duration;
use tracing::warn;

const MAX_ATTEMPTS: u32 = 8;
const INITIAL_DELAY: Duration = Duration::from_secs(2);
const MAX_DELAY: Duration = Duration::from_secs(30);

/// Kube API calls run through [`retry_on_transient_kube_error`], named in its retry logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KubeOperation {
    ListClusterSecretStores,
    ListExternalSecrets,
}

impl Display for KubeOperation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            KubeOperation::ListClusterSecretStores => "List ClusterSecretStores",
            KubeOperation::ListExternalSecrets => "List ExternalSecrets",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KubeErrorClass {
    Transient { server_retry_after: Option<Duration> },
    Permanent,
}

fn classify(error: &Error) -> KubeErrorClass {
    let Error::Api(status) = error else {
        return KubeErrorClass::Permanent;
    };
    match StatusCode::from_u16(status.code) {
        Ok(StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT) => {
            KubeErrorClass::Transient {
                server_retry_after: status
                    .details
                    .as_ref()
                    .map(|details| details.retry_after_seconds)
                    .filter(|seconds| *seconds > 0)
                    .map(|seconds| Duration::from_secs(u64::from(seconds))),
            }
        }
        _ => KubeErrorClass::Permanent,
    }
}

fn retry_delay(retry_index: u32, server_retry_after: Option<Duration>) -> Duration {
    let backoff = INITIAL_DELAY
        .saturating_mul(2_u32.saturating_pow(retry_index))
        .min(MAX_DELAY);
    server_retry_after.map_or(backoff, |retry_after| backoff.max(retry_after.min(MAX_DELAY)))
}

/// Runs a read-only kube API call, retrying it with backoff while the API server answers 429, 503 or 504.
///
/// Any other error is returned at once, unchanged. Must not be called from inside an async context.
pub(crate) fn retry_on_transient_kube_error<T, F, Fut>(operation: KubeOperation, call: F) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    retry_with_sleep(operation, call, thread::sleep)
}

fn retry_with_sleep<T, F, Fut, S>(operation: KubeOperation, mut call: F, mut sleep: S) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
    S: FnMut(Duration),
{
    let mut attempt = 1;
    loop {
        let error = match block_on(call()) {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let KubeErrorClass::Transient { server_retry_after } = classify(&error) else {
            return Err(error);
        };
        if attempt >= MAX_ATTEMPTS {
            return Err(error);
        }
        let wait = retry_delay(attempt - 1, server_retry_after);
        warn!(
            "{operation}: transient kube API error on attempt {attempt}/{MAX_ATTEMPTS}, retrying in {}s: {error}",
            wait.as_secs()
        );
        sleep(wait);
        attempt += 1;
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::runtime::block_on;
    use http::{Request, Response};
    use kube::Client;
    use kube::client::Body;
    use serde_json::{Value, json};
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::service_fn;

    pub(crate) fn status_body(code: u16, reason: &str) -> Value {
        json!({
            "kind": "Status", "apiVersion": "v1", "status": "Failure", "message": reason, "reason": reason,
            "code": code, "details": {"retryAfterSeconds": 1}
        })
    }

    pub(crate) fn empty_list_body() -> Value {
        json!({"kind": "List", "apiVersion": "v1", "metadata": {}, "items": []})
    }

    /// Kube client answering each request with the next scripted response, repeating the last one.
    pub(crate) fn scripted_kube_client(responses: Vec<(u16, Value)>) -> (Client, Arc<AtomicUsize>) {
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&requests);
        let service = service_fn(move |_request: Request<Body>| {
            let index = counter.fetch_add(1, Ordering::SeqCst).min(responses.len() - 1);
            let (status, body) = &responses[index];
            let response = Response::builder()
                .status(*status)
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap();
            async move { Ok::<_, Infallible>(response) }
        });
        (block_on(async { Client::new(service, "default") }), requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::core::Status;
    use kube::core::response::StatusDetails;
    use std::cell::Cell;
    use std::future::ready;

    fn api_error(code: u16, retry_after_seconds: Option<u32>) -> Error {
        let status = Status::failure("storage is (re)initializing", "TooManyRequests").with_code(code);
        let status = match retry_after_seconds {
            Some(retry_after_seconds) => status.with_details(StatusDetails {
                name: String::new(),
                group: String::new(),
                kind: String::new(),
                uid: String::new(),
                causes: Vec::new(),
                retry_after_seconds,
            }),
            None => status,
        };
        Error::Api(status.boxed())
    }

    #[test]
    fn classify_retries_only_transient_api_codes() {
        for code in [429, 503, 504] {
            assert_eq!(
                classify(&api_error(code, None)),
                KubeErrorClass::Transient {
                    server_retry_after: None
                },
                "code {code}"
            );
        }
        for code in [400, 401, 403, 404, 409, 500, 502] {
            assert_eq!(classify(&api_error(code, None)), KubeErrorClass::Permanent, "code {code}");
        }
        assert_eq!(classify(&Error::LinesCodecMaxLineLengthExceeded), KubeErrorClass::Permanent);
    }

    #[test]
    fn classify_reads_server_retry_after() {
        assert_eq!(
            classify(&api_error(429, Some(5))),
            KubeErrorClass::Transient {
                server_retry_after: Some(Duration::from_secs(5))
            }
        );
        assert_eq!(
            classify(&api_error(429, Some(0))),
            KubeErrorClass::Transient {
                server_retry_after: None
            }
        );
    }

    #[test]
    fn retry_delay_backs_off_exponentially_up_to_cap() {
        let delays: Vec<u64> = (0..MAX_ATTEMPTS - 1).map(|i| retry_delay(i, None).as_secs()).collect();
        assert_eq!(delays, vec![2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(retry_delay(u32::MAX, None), MAX_DELAY);
    }

    #[test]
    fn retry_delay_honours_larger_server_retry_after_within_cap() {
        assert_eq!(retry_delay(0, Some(Duration::from_secs(1))), Duration::from_secs(2));
        assert_eq!(retry_delay(0, Some(Duration::from_secs(10))), Duration::from_secs(10));
        assert_eq!(retry_delay(1, Some(Duration::from_secs(600))), MAX_DELAY);
    }

    #[test]
    fn retries_transient_error_until_success() {
        let calls = Cell::new(0);
        let mut sleeps = Vec::new();
        let result = retry_with_sleep(
            KubeOperation::ListClusterSecretStores,
            || {
                calls.set(calls.get() + 1);
                ready(if calls.get() < 3 {
                    Err(api_error(429, Some(3)))
                } else {
                    Ok("listed")
                })
            },
            |wait| sleeps.push(wait),
        );
        assert_eq!(result.unwrap(), "listed");
        assert_eq!(calls.get(), 3);
        assert_eq!(sleeps, vec![Duration::from_secs(3), Duration::from_secs(4)]);
    }

    #[test]
    fn returns_permanent_error_without_retry() {
        let calls = Cell::new(0);
        let mut sleeps = Vec::new();
        let result: Result<(), Error> = retry_with_sleep(
            KubeOperation::ListClusterSecretStores,
            || {
                calls.set(calls.get() + 1);
                ready(Err(api_error(404, None)))
            },
            |wait| sleeps.push(wait),
        );
        assert!(matches!(result, Err(Error::Api(status)) if status.code == 404));
        assert_eq!(calls.get(), 1);
        assert!(sleeps.is_empty());
    }

    #[test]
    fn returns_last_transient_error_once_budget_is_spent() {
        let calls = Cell::new(0);
        let mut sleeps = Vec::new();
        let result: Result<(), Error> = retry_with_sleep(
            KubeOperation::ListClusterSecretStores,
            || {
                calls.set(calls.get() + 1);
                ready(Err(api_error(if calls.get() < MAX_ATTEMPTS { 429 } else { 503 }, None)))
            },
            |wait| sleeps.push(wait),
        );
        assert!(matches!(result, Err(Error::Api(status)) if status.code == 503));
        assert_eq!(calls.get(), MAX_ATTEMPTS);
        assert_eq!(sleeps.iter().sum::<Duration>(), Duration::from_secs(120));
    }
}
