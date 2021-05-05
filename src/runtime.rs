use lazy_static::lazy_static;
use std::future::Future;
use std::sync::Mutex;
use tokio::runtime::{Builder, Runtime};

lazy_static! {
    static ref TOKIO_RUNTIME: Mutex<Runtime> = Mutex::new({
        Builder::new()
            .basic_scheduler()
            .thread_name("tokio-engine-blocking")
            .max_threads(1)
            .enable_all()
            .build()
            .unwrap()
    });
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    TOKIO_RUNTIME.lock().unwrap().block_on(future)
}
