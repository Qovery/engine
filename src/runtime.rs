use std::future::Future;

use tokio::runtime::Runtime;

pub fn async_run<F: Future>(future: F) -> F::Output {
    // TODO improve - is it efficient to create a Runtime at each exec?
    let mut runtime = Runtime::new().expect("unable to create a tokio runtime");
    runtime.block_on(future)
}
