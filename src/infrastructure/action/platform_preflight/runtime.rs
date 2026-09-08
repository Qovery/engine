//! Shared time bounds for asynchronous reads executed from synchronous preflight workers.

use crate::runtime::block_on;
use std::future::Future;
use std::time::Duration;
use tokio::time::error::Elapsed;

pub(super) const KUBERNETES_READ_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn block_on_timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, Elapsed> {
    block_on(async move { tokio::time::timeout(duration, future).await })
}

#[cfg(test)]
mod tests {
    use super::block_on_timeout;
    use std::time::Duration;

    #[test]
    fn timeout_is_constructed_inside_the_engine_runtime() {
        let value = block_on_timeout(Duration::from_secs(1), async { 42 }).unwrap();

        assert_eq!(value, 42);
    }
}
