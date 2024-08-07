use crate::deployment_manager::engine_message_stream::{
    EngineLogStreamContext, EngineMessageStream, EngineMsgStreamContext,
};
use crate::grpc::engine::{engine_message_tx, DeploymentInfo, EngineMessageRx, EngineMessageTx};
use crate::grpc::GrpcEngineClient;
use crate::logger::composite_logger::CompositeLogger;
use futures_util::{stream, StreamExt};
use qovery_engine::events::{EngineEvent, EngineMsg, EnvironmentStep, EventDetails, EventMessage, Stage};
use qovery_engine::log_file_writer::LogFileWriter;
use qovery_engine::logger::{Logger, StdIoLogger, UnboundedSenderLogger};
use qovery_engine::metrics_registry::{MetricsRegistry, StdMetricsRegistry};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, watch, Mutex};
use tokio_stream::Stream;
use tonic::Streaming;

/// A single deployment can receive N tasks.
/// A task represent a deployment group/engine request.
/// The same engine is going to receive all the deployment group/task for a deployment
pub struct DeploymentContext {
    pub deployment_info: DeploymentInfo,

    // The channel to send engine events to the gateway/receiver
    // The same channel is re-used across all tasks, so even in case of cnx loss
    // We can resume the connection with the gateway without losing events
    log_tx: UnboundedSender<EngineEvent>,
    log_rx: Arc<Mutex<EngineLogStreamContext>>,

    msg_tx: UnboundedSender<EngineMsg>,
    msg_rx: Arc<Mutex<EngineMsgStreamContext>>,

    // The last message we sent to the gateway, that we will re-emit in case of cnx resume
    last_msg_memento: Arc<Mutex<Option<EngineMessageTx>>>,
    log_file_writer: LogFileWriter,
}

impl DeploymentContext {
    pub fn new(deployment_info: DeploymentInfo, log_buffer_duration: Duration, log_file_writer: LogFileWriter) -> Self {
        let (log_engine_tx, log_engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
        let (msg_engine_tx, msg_engine_rx) = mpsc::unbounded_channel::<EngineMsg>();
        Self {
            deployment_info,
            log_tx: log_engine_tx,
            log_rx: Arc::new(Mutex::new(EngineLogStreamContext::new(log_engine_rx, log_buffer_duration))),

            msg_tx: msg_engine_tx,
            msg_rx: Arc::new(Mutex::new(EngineMsgStreamContext::new(msg_engine_rx))),

            // To keep the last message in case the cnx with upstream broke and that we need to re-emit the last message
            // on resume
            last_msg_memento: Arc::new(Mutex::new(None)),
            log_file_writer,
        }
    }

    pub async fn get_message_stream(
        &mut self,
    ) -> (
        UnboundedSender<EngineEvent>,                         // To feed the stream
        UnboundedSender<EngineMsg>,                           // To feed the stream
        impl Stream<Item = EngineMessageTx> + Send + 'static, // The stream that receive the EngineEvent and the EngineMsg
        watch::Sender<()>, // To know/stop the stream and release the rx side of the channel
    ) {
        let (stream, abort_handle) = EngineMessageStream::new(
            self.log_rx.clone().lock_owned().await,
            self.msg_rx.clone().lock_owned().await,
            self.last_msg_memento.clone().lock_owned().await,
        );
        (self.log_tx.clone(), self.msg_tx.clone(), stream, abort_handle)
    }

    pub async fn execute_deployment(
        &mut self,
        engine_client: &mut GrpcEngineClient,
    ) -> Result<
        (
            Streaming<EngineMessageRx>,
            watch::Sender<()>,
            Box<dyn Logger>,
            Box<dyn MetricsRegistry>,
            LogFileWriter,
        ),
        tonic::Status,
    > {
        let (log_tx, msg_tx, msg_stream, abort_upstream_tx) = self.get_message_stream().await;
        let msg_publisher = Box::new(msg_tx);
        let logger_for_task: Box<dyn Logger> = Box::new(CompositeLogger::new(vec![
            Box::new(StdIoLogger::new()),
            Box::new(UnboundedSenderLogger::new(log_tx, vec![])),
        ]));
        let metrics_registry: Box<dyn MetricsRegistry> = Box::new(StdMetricsRegistry::new(msg_publisher));

        let msg_stream = stream::iter(vec![EngineMessageTx {
            message_id: None,
            message: Some(engine_message_tx::Message::DeploymentRequest(self.deployment_info.clone())),
        }])
        .chain(msg_stream);

        engine_client.exec_deployment(msg_stream).await.map(|msg_stream| {
            (
                msg_stream.into_inner(),
                abort_upstream_tx,
                logger_for_task,
                metrics_registry,
                self.log_file_writer.clone(),
            )
        })
    }

    pub fn set_last_message_id(&mut self, last_id: String) {
        self.deployment_info.last_message_id = last_id;
    }

    pub async fn hard_abort_deployment(self, err: EngineEvent) {
        let event_details = err.get_details().clone();
        let _ = self.log_tx.send(err);

        let event_details =
            EventDetails::clone_changing_stage(event_details, Stage::Environment(EnvironmentStep::Terminated));
        let err = EngineEvent::Info(
            event_details,
            EventMessage::new("Qovery Engine has terminated the deployment".to_string(), None),
        );
        let _ = self.log_tx.send(err);

        // Wait for the gateway to receive the last message
        tokio::time::sleep(Duration::from_secs(10)).await;
    }

    pub async fn terminate_deployment(self) {}
}

impl Drop for DeploymentContext {
    fn drop(&mut self) {
        info!("Dropping deployment context");
    }
}
