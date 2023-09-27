use lazy_static::lazy_static;
use prometheus::{self, Encoder, IntCounter, TextEncoder};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;
use warp::http::StatusCode;
use warp::reply::{WithHeader, WithStatus};
use warp::Filter;

static MAX_THREADS: usize = 2;
lazy_static! {
    static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
        .thread_name("tokio")
        .max_blocking_threads(MAX_THREADS)
        .enable_all()
        .build()
        .unwrap();
    static ref METRICS_PROMETHEUS_NB_CALLS: IntCounter = register_int_counter!(
        "prometheus_endpoint_nb_call",
        "Number of time since startup that the prometheus endpoint got called"
    )
    .unwrap();
}

/// Launch a webserver as a Tokio task
///
/// Use the static tokio runtime to start a webserver.
/// Spawn a thread that block until the webserver stop (never)
pub fn launch_http_server(listen_on: &str, shutdown_handle: Arc<AtomicBool>) -> JoinHandle<()> {
    let listen_on: SocketAddr = listen_on.parse().unwrap_or_else(|_| {
        panic!("Cannot parse webserver listen_on parameter, should be ip:port instead {listen_on}")
    });

    info!("Starting tokio runtime");
    TOKIO_RUNTIME.spawn(launch_warp(listen_on, shutdown_handle))
}

pub fn launch_task<R: Send + 'static>(future: impl Future<Output = R> + Send + 'static) -> JoinHandle<R> {
    TOKIO_RUNTIME.spawn(future)
}

pub fn launch_blocking_task<R: Send + 'static>(task: impl FnOnce() -> R + Send + 'static) -> JoinHandle<R> {
    TOKIO_RUNTIME.spawn_blocking(task)
}

pub fn block_on<R>(task: impl Future<Output = R>) -> R {
    TOKIO_RUNTIME.block_on(task)
}

/// Start warp webserver
///
/// In most cast, only one should be started per application
async fn launch_warp(listen_on: SocketAddr, shutdown_handle: Arc<AtomicBool>) {
    let prometheus_srv = warp::path!("metrics").and(warp::get()).map(prometheus_service);
    let shutdown_srv = warp::path!("shutdown")
        .and(warp::get())
        .and(warp::addr::remote())
        .map(move |remote_addr| shutdown_service(remote_addr, &shutdown_handle));

    let healthcheck_srv = warp::path!("healthz").and(warp::get()).map(|| warp::reply::html("OK"));

    let routes = prometheus_srv.or(healthcheck_srv).or(shutdown_srv);

    warp::serve(routes).run(listen_on).await;
}

fn shutdown_service(remote_addr: Option<SocketAddr>, shutdown_handle: &AtomicBool) -> WithStatus<String> {
    match remote_addr.as_ref().map(|x| x.ip()) {
        Some(IpAddr::V4(ip4)) if ip4.is_loopback() => {}
        Some(IpAddr::V6(ip6))
            if ip6.is_loopback() || ip6.to_ipv4_mapped().map(|ip4| ip4.is_loopback()).unwrap_or(false) => {}
        _ => {
            warn!(
                "Remote addr {:?} wants to shutdown engine but is not allowed. Only call from local are allowed",
                remote_addr
            );
            return warp::reply::with_status("Not Allowed".to_string(), StatusCode::METHOD_NOT_ALLOWED);
        }
    }

    info!("Received API call to shutdown the engine");
    shutdown_handle.store(true, Ordering::Relaxed);

    warp::reply::with_status("OK".to_string(), StatusCode::OK)
}

/// Service responsible of the prometheus endpoint
///
/// Warp prometheus metrics endpoint.
///     1. Gather metrics
///     2. Encode them
///     3. return plain/text http response with it
fn prometheus_service() -> WithHeader<Vec<u8>> {
    METRICS_PROMETHEUS_NB_CALLS.inc();

    // Collect metrics and encode to text to send back to the browser
    let mut buffer = Vec::with_capacity(4096);
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    if encoder.encode(&metric_families, &mut buffer).is_err() {
        error!("Cannot encode prometheus metrics");
    }

    warp::reply::with_header(buffer, "Content-Type", prometheus::TEXT_FORMAT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_webserver() {
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let _handle = launch_http_server("127.0.0.1:8080", shutdown_flag.clone());
        let body = reqwest::blocking::get("http://127.0.0.1:8080/metrics")
            .unwrap()
            .text()
            .unwrap();

        assert!(
            body.contains("prometheus_endpoint_nb_call 1"),
            "can't launch properly webserver"
        );

        let body = reqwest::blocking::get("http://127.0.0.1:8080/shutdown")
            .unwrap()
            .text()
            .unwrap();

        assert_eq!(&body, "OK");
        assert!(shutdown_flag.load(Ordering::Relaxed));
    }
}
