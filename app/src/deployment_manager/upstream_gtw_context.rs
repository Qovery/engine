use crate::grpc::engine::EngineMessageRx;
use qovery_engine::log_file_writer::LogFileWriter;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use std::time::Duration;
use tokio::sync::watch;
use tonic::Streaming;

/// Represent the state when the engine is connected to the gateway and is forwarding/receiving engine events
pub struct UpstreamGatewayContext {
    pub msg_stream: Streaming<EngineMessageRx>,
    pub close_upstream_tx: watch::Sender<()>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
    log_file_writer: Box<LogFileWriter>,
}

impl UpstreamGatewayContext {
    pub fn new(
        msg_stream: Streaming<EngineMessageRx>,
        close_upstream_tx: watch::Sender<()>,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        log_file_writer: Box<LogFileWriter>,
    ) -> Self {
        Self {
            msg_stream,
            close_upstream_tx,
            logger,
            metrics_registry,
            log_file_writer,
        }
    }

    pub fn logger(&self) -> Box<dyn Logger> {
        self.logger.clone()
    }

    pub fn metrics_registry(&self) -> Box<dyn MetricsRegistry> {
        self.metrics_registry.clone()
    }

    pub fn log_file_writer(&self) -> Box<LogFileWriter> {
        self.log_file_writer.clone()
    }

    pub async fn await_termination(self) {
        let close_upstream_tx = self.close_handle();
        info!("Waiting for upstream connection to be terminated");
        while (tokio::time::timeout(Duration::from_secs(10), close_upstream_tx.closed()).await).is_err() {
            info!("Waiting for upstream connection to be terminated");
        }
        info!("Upstream connection terminated");
    }

    fn close_handle(self) -> watch::Sender<()> {
        // To force the drop of the struct
        self.close_upstream_tx
    }
}
