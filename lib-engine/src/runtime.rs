use lazy_static::lazy_static;
use std::future::Future;
use std::sync::RwLock;
use tokio::runtime::{Builder, Runtime};

lazy_static! {
    static ref TOKIO_RUNTIME: RwLock<Runtime> = RwLock::new({
        Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("tokio-engine-blocking")
            .enable_all()
            .build()
            .unwrap()
    });
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    TOKIO_RUNTIME.read().unwrap().block_on(future)
}
