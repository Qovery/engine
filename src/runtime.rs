use lazy_static::lazy_static;
use std::future::Future;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use tokio::time::error::Elapsed;
use tokio::time::timeout;

lazy_static! {
    static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("tokio-engine-blocking")
        .enable_all()
        .build()
        .unwrap();
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    TOKIO_RUNTIME.block_on(future)
}

pub fn block_on_with_timeout<F: Future>(future: F) -> Result<F::Output, Elapsed> {
    TOKIO_RUNTIME.block_on(async { timeout(Duration::from_secs(60 * 5), future).await })
}
