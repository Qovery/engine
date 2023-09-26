#![allow(unused_imports)]

use crate::grpc::engine::StepRecord as GrpcStepRecord;
use crate::grpc::engine::{engine_message_rx, EngineMessageRx};
use crate::grpc::engine::{engine_message_tx, DeploymentInfo, DeploymentType, EngineMessageTx};
use crate::grpc::engine::{DeploymentRequest, Metrics};
use crate::grpc::GrpcEngineClient;
use crate::logger::composite_logger::CompositeLogger;
use crate::metrics::METRICS_NB_RUNNING_TASKS;
use crate::models::TaskSelector;
use crate::tokio_utils;
use chrono::Utc;
use futures_util::{stream, Stream, StreamExt};
use prost_types::Timestamp;
use qovery_engine::engine_task::Task;
use qovery_engine::events::io::EngineEvent as EngineEventIo;
use qovery_engine::events::{EngineEvent, EngineMsg, EngineMsgPayload};
use qovery_engine::events::{EnvironmentStep, EventDetails, EventMessage, Stage};
use qovery_engine::logger::{Logger, StdIoLogger};
use qovery_engine::metrics_registry::{MetricsRegistry, StdMetricsRegistry};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, watch, Mutex, OwnedMutexGuard};
use tokio::time::timeout;
use tonic::{Code, Streaming};
use tracing::{error, field, Instrument, Level, Span};

// A single deployment can receive N tasks.
// A task represent a deployment group/engine request.
// The same engine is going to receive all the deployment group/task for a deployment
struct DeploymentContext {
    deployment_info: DeploymentInfo,

    // The channel to send engine events to the gateway/receiver
    // The same channel is re-used across all tasks, so even in case of cnx loss
    // We can resume the connection with the gateway without losing events
    log_tx: UnboundedSender<EngineEvent>,
    log_rx: Arc<Mutex<EngineLogStreamContext>>,

    msg_tx: UnboundedSender<EngineMsg>,
    msg_rx: Arc<Mutex<EngineMsgStreamContext>>,

    // The last message we sent to the gateway, that we will re-emit in case of cnx resume
    last_msg_memento: Arc<Mutex<Option<EngineMessageTx>>>,
}

struct TaskContext {
    task: Arc<Box<dyn Task>>,
    _handle: tokio::task::JoinHandle<()>,
}

// Represent the state when the engine is connected to the gateway and is forwarding/receiving engine events
struct UpstreamGatewayContext {
    msg_stream: Streaming<EngineMessageRx>,
    close_upstream_tx: watch::Sender<()>,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
}

//
//
//
//
//
//                                                                                                 DEPLOYMENT TERMINATED
//                                                |-------------------------------------------------------------------------------------------------------------------+
//                                                |                                                                                                                   |
//                                                |                                                                                                                   |
//                                                |                                                                                                                   |
//                             +------------------|------------------+                    +-------------------------------------+                +--------------------|--------------------+
//                             |                                     |                    |                                     |                |                                         |
//                             |                                     |   Got deployment   |                                     |Got Task to exec|                                         |
//                      +------|       SEEKING_NEW_DEPLOYMENT        ---------------------|        EXECUTING_DEPLOYMENT         -----------------|        EXECUTING_DEPLOYMENT_TASK        |
//                      |      |                                     |                    |                                     |                |                                         |
//  No deployment found |      |                                     |                    |                                     |                |                                         |
//                      +-------------------------|------------------+                    +------------------|------------------+                +--------------------|--------------------+
//                                                |                                                          |                                                        |
//                                                |                                                          |                                                        |
//                                                |                                                          |Lost connection to GTW                                  |
//                                                |                                                          |                                                        |  Lost connection to GTW
//                                                |                                                          |                                                        |
//                                                |                                                          |                                                        |
//                                                |                                                          |      +-------------------------------------+           |
//                                                |                                                          |      |                                     |           |
//                                                |                     FAILURE                              |      |                                     |           |
//                                                +----------------------------------------------------------+------|         RESUMING_DEPLOYMENT         |-----------+
//                                                                                                                  |                                     |
//                                                                                                                  |                                     |
//                                                                                                                  +-------------------------------------+
//
enum DeploymentManagerState {
    // Engine has nothing to do, waiting for a new deployment to be available
    SeekingNewDeployment {},

    // Engine has a deployment, waiting to receive task to be executed for this deployment
    ExecutingDeployment {
        deployment: DeploymentContext,
    },

    // We are executing a deployment and a task, business as usual
    ExecutingDeploymentTask {
        deployment: DeploymentContext,
        task: TaskContext,
        upstream_gtw: UpstreamGatewayContext,
    },

    // We lost the connectivity to the GTW while we are executing a task
    // Trying to reconnect
    ResumingDeploymentTask {
        deployment: DeploymentContext,
        task: TaskContext,
    },
}

impl DeploymentManagerState {
    pub fn does_execute_deployment(&self) -> bool {
        !matches!(self, DeploymentManagerState::SeekingNewDeployment {})
    }
}

impl DeploymentContext {
    pub fn new(deployment_info: DeploymentInfo) -> Self {
        let (log_engine_tx, log_engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
        let (msg_engine_tx, msg_engine_rx) = mpsc::unbounded_channel::<EngineMsg>();
        Self {
            deployment_info,
            log_tx: log_engine_tx,
            log_rx: Arc::new(Mutex::new(EngineLogStreamContext::new(log_engine_rx))),

            msg_tx: msg_engine_tx,
            msg_rx: Arc::new(Mutex::new(EngineMsgStreamContext::new(msg_engine_rx))),

            // To keep the last message in case the cnx with upstream broke and that we need to re-emit the last message
            // on resume
            last_msg_memento: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_message_stream(
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
        ),
        tonic::Status,
    > {
        let (log_tx, msg_tx, msg_stream, abort_upstream_tx) = self.get_message_stream().await;
        let msg_publisher = Box::new(msg_tx);
        let logger_for_task: Box<dyn Logger> =
            Box::new(CompositeLogger::new(vec![Box::new(StdIoLogger::new()), Box::new(log_tx)]));
        let metrics_registry: Box<dyn MetricsRegistry> = Box::new(StdMetricsRegistry::new(msg_publisher));

        let msg_stream = stream::iter(vec![EngineMessageTx {
            message_id: None,
            message: Some(engine_message_tx::Message::DeploymentRequest(self.deployment_info.clone())),
        }])
        .chain(msg_stream);

        engine_client
            .exec_deployment(msg_stream)
            .await
            .map(|msg_stream| (msg_stream.into_inner(), abort_upstream_tx, logger_for_task, metrics_registry))
    }

    pub fn set_last_message_id(&mut self, last_id: String) {
        self.deployment_info.last_message_id = last_id;
    }
}

type MkEngineTask = Box<
    dyn Fn(
            String,
            &DeploymentInfo,
            &GrpcEngineClient,
            Box<dyn Logger>,
            Box<dyn MetricsRegistry>,
        ) -> Result<Box<dyn Task>, EngineEvent>
        + Send,
>;

pub struct DeploymentManager {
    default_wait_time: Duration,
    deadline_for_new_task: Duration,
    deployment_request: DeploymentRequest,
    engine_client: GrpcEngineClient,
    should_shutdown: Arc<AtomicBool>,
    mk_engine_task: MkEngineTask,
}

impl DeploymentManager {
    pub fn new(
        task_type: &TaskSelector,
        engine_client: GrpcEngineClient,
        should_shutdown: Arc<AtomicBool>,
        mk_engine_task: MkEngineTask,
    ) -> Self {
        METRICS_NB_RUNNING_TASKS.set(0);
        let deployment_request = match task_type {
            TaskSelector::Infrastructure(_) => DeploymentRequest {
                deployment_type: DeploymentType::Infrastructure as i32,
            },
            TaskSelector::Environment(_) => DeploymentRequest {
                deployment_type: DeploymentType::Environment as i32,
            },
        };

        Self {
            default_wait_time: Duration::from_secs(10),
            deadline_for_new_task: Duration::from_secs(15),
            deployment_request,
            engine_client,
            should_shutdown,
            mk_engine_task,
        }
    }

    pub async fn run(mut self) {
        const SPAN_NAME: &str = "deploymnt_mngr";
        let mut state = DeploymentManagerState::SeekingNewDeployment {};
        let mut delay = None;
        let mut span = span!(Level::INFO, SPAN_NAME, execution_id = field::Empty);

        while state.does_execute_deployment() || !self.should_shutdown.load(Ordering::Relaxed) {
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }

            let (new_state, new_delay) = self.run_exec_state(state).instrument(span.clone()).await;

            state = new_state;
            delay = new_delay;

            // Add some Hook on transition change for metrics and tracing
            match &state {
                DeploymentManagerState::SeekingNewDeployment { .. } => {
                    METRICS_NB_RUNNING_TASKS.set(0);
                    span = span!(Level::INFO, SPAN_NAME, execution_id = field::Empty);
                }
                DeploymentManagerState::ExecutingDeployment { deployment, .. } => {
                    METRICS_NB_RUNNING_TASKS.set(1);
                    span = span!(Level::INFO, SPAN_NAME, execution_id = deployment.deployment_info.execution_id);
                }
                DeploymentManagerState::ExecutingDeploymentTask { .. } => {}
                DeploymentManagerState::ResumingDeploymentTask { .. } => {}
            }
        }
    }

    async fn run_exec_state(
        &mut self,
        current_st: DeploymentManagerState,
    ) -> (DeploymentManagerState, Option<Duration>) {
        match current_st {
            DeploymentManagerState::SeekingNewDeployment {} => self.seek_new_deployment().await,
            DeploymentManagerState::ExecutingDeployment { deployment } => self.execute_deployment(deployment).await,
            DeploymentManagerState::ExecutingDeploymentTask {
                deployment,
                task,
                upstream_gtw: upstream,
            } => self.execute_deployment_task(deployment, task, upstream).await,
            DeploymentManagerState::ResumingDeploymentTask { deployment, task } => {
                self.resuming_deployment_task(deployment, task).await
            }
        }
    }

    /// We have a new deployment to execute, but no task yet. Contact the gateway to claim the deployment and get a task to execute
    async fn execute_deployment(
        &mut self,
        mut deployment: DeploymentContext,
    ) -> (DeploymentManagerState, Option<Duration>) {
        info!("Starting to execute deployment");
        let (mut msg_stream, close_upstream_tx, logger, metrics_registry) = match deployment
            .execute_deployment(&mut self.engine_client)
            .await
        {
            Ok(upstream_msg) => upstream_msg,
            Err(err) => {
                return match err.code() {
                    Code::NotFound => {
                        error!(
                                "Deployment not found anymore while wanting to execute task for deployment. Took too long for engine to dequeue deployment ? {:?}",
                                &deployment.deployment_info
                            );
                        (DeploymentManagerState::SeekingNewDeployment {}, None)
                    }
                    _ => {
                        error!("Error while getting new deployment: {}", err);
                        let next_step = DeploymentManagerState::ExecutingDeployment { deployment };
                        (next_step, Some(self.default_wait_time))
                    }
                };
            }
        };
        info!(
            "Connected to gateway, executing deployment task for: {:?}",
            deployment.deployment_info
        );

        tokio::select! {
            biased;

            // We lost the connection with gateway to forward engine message, trying to reconnect
            _ = close_upstream_tx.closed() => {
                info!("EngineEvent forwarder to gateway has been close, trying to resume connection");
                let next_step = DeploymentManagerState::ExecutingDeployment { deployment };
                (next_step, None)
            }

            msg = msg_stream.next() => match msg {
                Some(Ok(msg)) => {
                    // We record the last message we received, so in case of cnx loss
                    // we can resume the deployment and restart from the last message
                    deployment.set_last_message_id(msg.message_id.clone());

                    match msg.request {
                        Some(engine_message_rx::Request::DeploymentRequest(payload)) => {
                            info!("Received new deployment task: {}", payload);
                            let task = (self.mk_engine_task)(payload, &deployment.deployment_info, &self.engine_client, logger.clone(), metrics_registry.clone());
                            match task {
                                Ok(task) => {
                                    let next_step = DeploymentManagerState::ExecutingDeploymentTask {
                                        deployment: DeploymentContext::new(deployment.deployment_info),
                                        task: Self::spawn_new_task(task),
                                        upstream_gtw: UpstreamGatewayContext { msg_stream, close_upstream_tx, logger, metrics_registry }
                                    };
                                    (next_step, None)
                                }
                                Err(err) => {
                                    Self::hard_abort_deployment(deployment, err).await
                                }
                            }
                        }
                        Some(_) | None => {
                            error!("Terminating deployment, received {:?} while waiting for a new task", msg);
                            (DeploymentManagerState::SeekingNewDeployment {}, None)
                        }
                    }
                },

                // Return to try to resume the current deployment
                None => {
                    info!("Upstream stream closed");
                    let next_step = DeploymentManagerState::ExecutingDeployment { deployment };
                    (next_step, None)
                }

                // Return to try to resume the current deployment
                Some(Err(e)) => {
                    error!("error while receiving message from grpc server: {}", e);
                    let next_step = DeploymentManagerState::ExecutingDeployment { deployment };
                    (next_step, None)
                }
            }
        }
    }

    /// We have been disconnected from the gateway while we are executing a deployment task
    /// Try to reconnect with upstream and resume msg forwarding.
    async fn resuming_deployment_task(
        &mut self,
        mut deployment: DeploymentContext,
        task: TaskContext,
    ) -> (DeploymentManagerState, Option<Duration>) {
        info!("Trying to resume connection with gateway");
        match deployment.execute_deployment(&mut self.engine_client).await {
            Ok((msg_stream, close_upstream_tx, logger, metrics_registry)) => {
                info!("Resumed connectivity with gtw");
                let next_step = DeploymentManagerState::ExecutingDeploymentTask {
                    deployment,
                    task,
                    upstream_gtw: UpstreamGatewayContext {
                        msg_stream,
                        close_upstream_tx,
                        logger,
                        metrics_registry,
                    },
                };
                (next_step, None)
            }

            Err(err) => match err.code() {
                Code::NotFound => {
                    error!(
                        "Deployment not found anymore while wanting to resume task ??? {:?}",
                        &deployment.deployment_info
                    );
                    Self::terminate_task(task).await;
                    (DeploymentManagerState::SeekingNewDeployment {}, None)
                }
                _ => {
                    error!("Error while resuming connection with gateway: {}", err);
                    let next_step = DeploymentManagerState::ResumingDeploymentTask { deployment, task };
                    (next_step, Some(self.default_wait_time))
                }
            },
        }
    }

    /// Engine has nothing to do, pool the gtw to get a new deployment to execute
    async fn seek_new_deployment(&mut self) -> (DeploymentManagerState, Option<Duration>) {
        let deployment_info = match self
            .engine_client
            .get_new_deployment(self.deployment_request.clone())
            .await
        {
            Ok(deployment_info) => deployment_info.into_inner(),
            Err(err) => {
                if err.code() == Code::NotFound {
                    info!("No deployment found, waiting for a new one");
                } else {
                    error!("Error while getting new deployment: {}", err);
                }
                return (DeploymentManagerState::SeekingNewDeployment {}, Some(self.default_wait_time));
            }
        };

        info!("Got new deployment for: {:?}", deployment_info);
        let next_state = DeploymentManagerState::ExecutingDeployment {
            deployment: DeploymentContext::new(deployment_info),
        };

        (next_state, None)
    }

    /// We have a deployment and a task to execute, we execute the task, business as usual
    async fn execute_deployment_task(
        &mut self,
        mut deployment: DeploymentContext,
        mut task: TaskContext,
        mut upstream: UpstreamGatewayContext,
    ) -> (DeploymentManagerState, Option<Duration>) {
        info!("Starting to execute deployment task");

        loop {
            let task_is_terminated = task.task.is_terminated();
            let mut await_task_termination = task.task.await_terminated();

            tokio::select! {
                biased;

                // If there is no task on-going for this deployment, we wait at max 15sec to receive a new one
                _ = tokio::time::sleep(self.deadline_for_new_task), if task_is_terminated => {
                    info!("No new message after 15s, assuming deployment is terminated");
                    let _ = upstream.close_upstream_tx.send(());
                    return (DeploymentManagerState::SeekingNewDeployment {}, None);
                }

                // Task is terminated, we re-loop to check if there is a new task for this deployment
                // and to set the timeout correctly
                _ = await_task_termination.recv(), if !task_is_terminated => {
                    info!("Engine Task terminated");
                }

                // We lost the connection with gateway to forward engine message, trying to reconnect
                _ = upstream.close_upstream_tx.closed() => {
                    info!("EngineEvent forwarder to gateway has been close, trying to resume connection");
                    let next_step = DeploymentManagerState::ResumingDeploymentTask {
                        deployment,
                        task,
                    };
                    return (next_step, None);
                }


                // We wait to receive a new message from the gateway
                // In case of error, we return to try to resume the current deployment.
                // The server will let us know if the deployment is still valid
                msg = upstream.msg_stream.next() => {
                    match msg {
                        Some(Ok(msg)) => {
                            match msg.request {
                                Some(engine_message_rx::Request::DeploymentRequest(payload)) => {
                                    info!("Received new deployment task: {}", payload);
                                    let new_task = (self.mk_engine_task)(payload, &deployment.deployment_info, &self.engine_client, upstream.logger.clone(), upstream.metrics_registry.clone());
                                    match new_task {
                                        Ok(new_task) => {
                                            Self::await_task_termination(task).await;
                                            task = Self::spawn_new_task(new_task);
                                        }
                                        Err(err) => {
                                            return Self::hard_abort_deployment(deployment, err).await;
                                        }
                                    }
                                }
                                Some(engine_message_rx::Request::DeploymentCancel(_)) => {
                                    info!("Received cancel request: {:?}", msg);
                                    Self::terminate_task(task).await;

                                    return (DeploymentManagerState::SeekingNewDeployment {}, None);
                                }
                                Some(engine_message_rx::Request::Terminated(_)) => {
                                    info!("Received terminated message for deployment: {:?}", msg);
                                    Self::terminate_task(task).await;

                                    return (DeploymentManagerState::SeekingNewDeployment {}, None);
                                }
                                None => {
                                    error!("Invalid payload received from grpc server. Update the protobuf !");
                                }
                            }

                            // We record the last message we received, so in case of cnx loss
                            // we can resume the deployment and restart from the last message
                            deployment.set_last_message_id(msg.message_id.clone());
                        },

                        // Return to try to resume the current deployment
                        None => {
                            info!("Upstream stream closed");
                            let next_step = DeploymentManagerState::ResumingDeploymentTask {
                                deployment,
                                task,
                            };
                            return (next_step, None);
                        }

                        // Return to try to resume the current deployment
                        Some(Err(e)) => {
                            error!("error while receiving message from grpc server: {}", e);
                            let next_step = DeploymentManagerState::ResumingDeploymentTask {
                                deployment,
                                task,
                            };
                            return (next_step, None);
                        }
                    }
                }
            }
        }
    }

    async fn terminate_task(ctx: TaskContext) {
        warn!("Canceling current task");
        ctx.task.cancel();
        info!("Task canceled, waiting for task to terminate");
        Self::await_task_termination(ctx).await;
    }

    async fn await_task_termination(ctx: TaskContext) {
        info!("Waiting for task to terminate");
        while (timeout(Duration::from_secs(10), ctx.task.await_terminated().recv()).await).is_err() {
            info!("Waiting for task to terminate");
        }
        info!("Task terminated");
    }

    fn spawn_new_task(task: Box<dyn Task>) -> TaskContext {
        let task = Arc::new(task);

        let task_handle = tokio_utils::launch_blocking_task({
            let task = task.clone();
            move || {
                task.run();
            }
        });

        TaskContext {
            task,
            _handle: task_handle,
        }
    }

    async fn hard_abort_deployment(
        deployment: DeploymentContext,
        err: EngineEvent,
    ) -> (DeploymentManagerState, Option<Duration>) {
        let event_details = err.get_details().clone();
        let _ = deployment.log_tx.send(err);

        let event_details =
            EventDetails::clone_changing_stage(event_details, Stage::Environment(EnvironmentStep::Terminated));
        let err = EngineEvent::Info(
            event_details,
            EventMessage::new("Qovery Engine has terminated the deployment".to_string(), None),
        );
        let _ = deployment.log_tx.send(err);

        // Wait a bit for the message to be flushed
        let _ = tokio::time::sleep(Duration::from_secs(5)).await;

        (DeploymentManagerState::SeekingNewDeployment {}, None)
    }
}

struct EngineMsgStreamContext {
    msg_receiver: UnboundedReceiver<EngineMsg>,
    should_stop: Option<watch::Receiver<()>>,
}

// Represent the context of the stream that the engine use to communicate with the gateway
struct EngineLogStreamContext {
    log_receiver: UnboundedReceiver<EngineEvent>,
    log_buffer: Vec<EngineEventIo>,
    log_buffer_duration: Duration,
    should_stop: Option<watch::Receiver<()>>,
}

impl EngineLogStreamContext {
    fn new(log_receiver: UnboundedReceiver<EngineEvent>) -> Self {
        EngineLogStreamContext {
            log_receiver,
            log_buffer: Vec::with_capacity(1024),
            log_buffer_duration: Duration::from_secs(1),
            should_stop: None,
        }
    }
}

impl EngineMsgStreamContext {
    fn new(msg_receiver: UnboundedReceiver<EngineMsg>) -> Self {
        EngineMsgStreamContext {
            msg_receiver,
            should_stop: None,
        }
    }
}

struct EngineMessageStream {
    stream: Box<dyn Stream<Item = EngineMessageTx> + Send + 'static>,
}

impl EngineMessageStream {
    pub fn new(
        mut log_stream_context: OwnedMutexGuard<EngineLogStreamContext>,
        mut msg_stream_context: OwnedMutexGuard<EngineMsgStreamContext>,
        mut last_msg_memento: OwnedMutexGuard<Option<EngineMessageTx>>,
    ) -> (Self, watch::Sender<()>) {
        let (abort_handle_tx, abort_handle_rx) = watch::channel(());
        log_stream_context.should_stop = Some(abort_handle_rx);
        msg_stream_context.should_stop = Some(abort_handle_tx.subscribe());

        // We re-emit the last message in case of failure
        let remit_last_msg: Pin<Box<dyn Stream<Item = EngineMessageTx> + Send>> =
            if let Some(last_msg) = last_msg_memento.clone() {
                Box::pin(stream::once(futures_util::future::ready(last_msg)))
            } else {
                Box::pin(stream::empty())
            };

        // Normal flow were we dequeue engine message
        let normal_flow = stream::select(
            stream::unfold(log_stream_context, Self::on_next_log),
            stream::unfold(msg_stream_context, Self::on_next_msg),
        )
        // We inspect each message to store the last_msg to re-emit it in case of failure
        // We set the message id of each msg to have a global order of the message
        .map(move |mut msg| {
            let ts = Utc::now();
            msg.message_id = Some(Timestamp {
                seconds: ts.timestamp(),
                nanos: ts.timestamp_subsec_nanos() as i32,
            });
            last_msg_memento.replace(msg.clone());
            msg
        });

        let s = Self {
            stream: Box::new(remit_last_msg.chain(normal_flow)),
        };

        (s, abort_handle_tx)
    }

    fn convert_logs_to_grpc_message(buffer: &Vec<EngineEventIo>) -> EngineMessageTx {
        EngineMessageTx {
            message_id: None, // Will be generated at a later stage
            message: Some(engine_message_tx::Message::Log(
                serde_json::to_string(buffer).unwrap_or_default(),
            )),
        }
    }

    fn convert_msg_to_grpc_message(msg: EngineMsg) -> EngineMessageTx {
        match msg.payload {
            EngineMsgPayload::Metrics(step_record) => EngineMessageTx {
                message_id: None, // Will be generated at a later stage
                message: Some(engine_message_tx::Message::Metrics(Metrics {
                    step_record: Some(GrpcStepRecord::from_record(step_record)),
                })),
            },
        }
    }

    async fn on_next_msg(
        mut ctx: OwnedMutexGuard<EngineMsgStreamContext>,
    ) -> Option<(EngineMessageTx, OwnedMutexGuard<EngineMsgStreamContext>)> {
        let mut should_stop = ctx.should_stop.take().unwrap();
        let opt_engine_message_tx = tokio::select! {
            biased;

            msg = ctx.msg_receiver.recv() => msg.map(Self::convert_msg_to_grpc_message),

             // Deployment asked to be aborted, we leave to release the mutex of the channel
            _ = should_stop.changed() => None
        };
        ctx.should_stop = Some(should_stop);

        opt_engine_message_tx.map(|engine_message_tx| (engine_message_tx, ctx))
    }

    async fn on_next_log(
        mut ctx: OwnedMutexGuard<EngineLogStreamContext>,
    ) -> Option<(EngineMessageTx, OwnedMutexGuard<EngineLogStreamContext>)> {
        // We re-send previous messages that may not have been received, gateway is responsible for dedup them
        let mut should_stop = ctx.should_stop.take().unwrap();
        let opt_engine_message_tx = tokio::select! {
            biased;

            msg = ctx.log_receiver.recv() => match msg {
                Some(engine_event) => {
                    // Buffer msg to avoid flooding the gateway
                    ctx.log_buffer.clear();
                    ctx.log_buffer.push(EngineEventIo::from(engine_event));
                    tokio::time::sleep(ctx.log_buffer_duration).await;
                    while ctx.log_buffer.len() < ctx.log_buffer.capacity() {
                        match ctx.log_receiver.try_recv() {
                            Ok(engine_event) => ctx.log_buffer.push(EngineEventIo::from(engine_event)),
                            _ => break,
                        }
                    }

                    Some(Self::convert_logs_to_grpc_message(&ctx.log_buffer))
                }
                None => None,
            },

            // Deployment asked to be aborted, we leave to release the mutex of the channel
            _ = should_stop.changed() => None
        };
        ctx.should_stop = Some(should_stop);

        opt_engine_message_tx.map(|engine_message_tx| (engine_message_tx, ctx))
    }
}

impl Stream for EngineMessageStream {
    type Item = EngineMessageTx;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        unsafe { self.map_unchecked_mut(|s| s.stream.deref_mut()).poll_next(cx) }
    }
}

impl Drop for EngineMessageStream {
    fn drop(&mut self) {
        info!("engine message stream to gateway terminated");
    }
}

#[cfg(test)]
mod test {
    use crate::deployment_manager::{DeploymentContext, DeploymentManager, EngineMessageStream};
    use crate::grpc::engine::engine_server::{Engine, EngineServer};
    use crate::grpc::engine::{
        engine_message_rx, DeploymentInfo, DeploymentRequest, EngineMessageRx, EngineMessageTx, GitTokenRequest,
        GitTokenResponse, ServiceVersionRequest, ServiceVersionResponse,
    };
    use crate::grpc::test::new_engine_client_test;
    use crate::models::TaskSelector;
    use futures_util::{pin_mut, stream, Stream, StreamExt};
    use qovery_engine::engine_task::Task;
    use qovery_engine::errors::EngineError;
    use qovery_engine::events;
    use qovery_engine::events::{EngineEvent, EngineMsg, EngineMsgPayload, EnvironmentStep, Stage, Transmitter};
    use qovery_engine::io_models::QoveryIdentifier;
    use qovery_engine::metrics_registry::{StepLabel, StepName, StepRecord};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, RwLock};
    use std::thread;
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tokio::sync::broadcast::Receiver;
    use tokio::time::timeout;
    use tonic::transport::Server;
    use tonic::{Request, Response, Status, Streaming};
    use uuid::Uuid;

    #[tokio::test]
    async fn test_engine_message_stream() {
        let event_details = events::EventDetails::new(
            None,
            QoveryIdentifier::new(Uuid::new_v4()),
            QoveryIdentifier::new(Uuid::new_v4()),
            " test".to_string(),
            Stage::Environment(EnvironmentStep::Cancelled),
            Transmitter::TaskManager(Uuid::new_v4(), "test".to_string()),
        );
        let engine_event =
            EngineEvent::Error(EngineError::new_task_cancellation_requested(event_details.clone()), None);
        let deployment = DeploymentContext::new(DeploymentInfo::default());
        let buffer_duration = Duration::from_millis(100);
        let buffer_deadline = buffer_duration + Duration::from_millis(50);
        deployment.log_rx.clone().lock().await.log_buffer_duration = buffer_duration;

        // Dropping the handle should terminate the stream
        let (mut stream, abort_handle) = EngineMessageStream::new(
            deployment.log_rx.clone().lock_owned().await,
            deployment.msg_rx.clone().lock_owned().await,
            deployment.last_msg_memento.clone().lock_owned().await,
        );
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());
        drop(abort_handle);
        assert!(matches!(timeout(buffer_deadline, stream.next()).await, Ok(None)));

        drop(stream);
        // Log sent should be received and buffered
        let log_tx = deployment.log_tx.clone();
        let (mut stream, abort_handle) = EngineMessageStream::new(
            deployment.log_rx.clone().lock_owned().await,
            deployment.msg_rx.clone().lock_owned().await,
            deployment.last_msg_memento.clone().lock_owned().await,
        );
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());
        let _ = log_tx.send(engine_event.clone());
        let ret = log_tx.send(engine_event.clone());
        assert!(ret.is_ok());

        // We should receive one batch
        assert!(matches!(timeout(buffer_deadline, stream.next()).await, Ok(Some(_))));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());

        // We should receive 2 batch as message have been sent after the buffer duration
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        tokio::spawn({
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                let _ = log_tx.send(engine_event.clone());
                tokio::time::sleep(buffer_deadline).await;
                let ret = log_tx.send(engine_event.clone());
                assert!(ret.is_ok());
            }
        });

        barrier.wait().await;
        assert!(matches!(timeout(buffer_deadline, stream.next()).await, Ok(Some(_))));
        let msg = timeout(buffer_deadline * 2, stream.next()).await;
        assert!(matches!(msg, Ok(Some(_))));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());

        // Resuming the stream should re-send the previous messages
        drop(abort_handle);
        drop(stream);
        let (mut stream, _abort_handle) = EngineMessageStream::new(
            deployment.log_rx.clone().lock_owned().await,
            deployment.msg_rx.clone().lock_owned().await,
            deployment.last_msg_memento.clone().lock_owned().await,
        );
        assert!(matches!(
            timeout(buffer_deadline, stream.next()).await,
            Ok(Some(m)) if m.message_id.as_ref().unwrap() == &msg.unwrap().unwrap().message_id.unwrap()
        ));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());

        // Terminating the deployment should terminate the stream
        drop(deployment);
        assert!(matches!(timeout(buffer_deadline, stream.next()).await, Ok(None)));
    }

    #[tokio::test]
    async fn test_engine_message_stream_msg() {
        let engine_msg = EngineMsg::new(EngineMsgPayload::Metrics(StepRecord::new(
            StepName::Deployment,
            StepLabel::Service,
            Uuid::new_v4(),
        )));
        let deployment = DeploymentContext::new(DeploymentInfo::default());
        let buffer_duration = Duration::from_millis(100);
        let buffer_deadline = buffer_duration + Duration::from_millis(50);
        let msg_tx = deployment.msg_tx.clone();
        let (mut stream, abort_handle) = EngineMessageStream::new(
            deployment.log_rx.clone().lock_owned().await,
            deployment.msg_rx.clone().lock_owned().await,
            deployment.last_msg_memento.clone().lock_owned().await,
        );

        let _ = msg_tx.send(engine_msg.clone());
        let msg = timeout(buffer_deadline * 2, stream.next()).await;
        assert!(matches!(msg, Ok(Some(_))));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());

        // Resuming the stream should re-send the previous messages
        drop(abort_handle);
        drop(stream);
        let (mut stream, _abort_handle) = EngineMessageStream::new(
            deployment.log_rx.clone().lock_owned().await,
            deployment.msg_rx.clone().lock_owned().await,
            deployment.last_msg_memento.clone().lock_owned().await,
        );
        assert!(matches!(
            timeout(buffer_deadline, stream.next()).await,
            Ok(Some(m)) if m.message_id.as_ref().unwrap() == &msg.unwrap().unwrap().message_id.unwrap()
        ));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());
    }

    //
    // Deployment Manager
    //
    struct MyEngineGtwTest {
        deployments: Mutex<Vec<Result<Response<DeploymentInfo>, Status>>>,
        msgs: Vec<Result<EngineMessageRx, Status>>,
    }
    #[tonic::async_trait]
    impl Engine for MyEngineGtwTest {
        async fn is_authorized(&self, _request: Request<()>) -> Result<Response<()>, Status> {
            Ok(Response::new(()))
        }

        async fn get_new_deployment(
            &self,
            _request: Request<DeploymentRequest>,
        ) -> Result<Response<DeploymentInfo>, Status> {
            self.deployments
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| Err(Status::not_found("")))
        }

        type ExecDeploymentStream = Pin<Box<dyn Stream<Item = Result<EngineMessageRx, Status>> + Send>>;
        async fn exec_deployment(
            &self,
            _request: Request<Streaming<EngineMessageTx>>,
        ) -> Result<Response<Self::ExecDeploymentStream>, Status> {
            let stream = Box::pin(stream::iter(self.msgs.clone()).chain(stream::pending()));
            Ok(Response::new(stream))
        }

        async fn get_service_version(
            &self,
            _request: Request<ServiceVersionRequest>,
        ) -> Result<Response<ServiceVersionResponse>, Status> {
            Err(Status::unimplemented("Not implemented"))
        }

        async fn get_git_token(
            &self,
            _request: Request<GitTokenRequest>,
        ) -> Result<Response<GitTokenResponse>, Status> {
            Err(Status::unimplemented("Not implemented"))
        }
    }

    #[derive(Clone)]
    pub struct EngineTaskTest {
        is_terminated: Arc<(RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>)>,
        should_shutdown: Arc<AtomicBool>,
        is_running: Arc<AtomicBool>,
    }

    impl EngineTaskTest {
        pub fn new() -> Self {
            Self {
                is_terminated: {
                    let (tx, rx) = broadcast::channel(1);
                    Arc::new((RwLock::new(Some(tx)), rx))
                },
                should_shutdown: Arc::new(AtomicBool::new(false)),
                is_running: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl Task for EngineTaskTest {
        fn id(&self) -> &str {
            ""
        }

        fn run(&self) {
            info!("Task starting to run");
            self.is_running.store(true, Ordering::Relaxed);
            let _guard = scopeguard::guard((), |_| {
                let Some(is_terminated_tx) = self.is_terminated.0.write().unwrap().take() else {
                    return;
                };
                let _ = is_terminated_tx.send(());
                self.is_running.store(false, Ordering::Relaxed);
            });

            while !self.should_shutdown.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
            }
            info!("Task terminated to run");
        }

        fn cancel(&self) -> bool {
            self.should_shutdown.store(true, Ordering::Relaxed);
            true
        }

        fn cancel_checker(&self) -> Box<dyn Fn() -> bool + Send + Sync> {
            let should_shutdown = self.should_shutdown.clone();
            Box::new(move || should_shutdown.load(Ordering::Relaxed))
        }

        fn is_terminated(&self) -> bool {
            self.is_terminated.0.read().map(|tx| tx.is_none()).unwrap_or(true)
        }

        fn await_terminated(&self) -> Receiver<()> {
            self.is_terminated.1.resubscribe()
        }
    }

    #[tokio::test]
    async fn test_deployment_manager_shutdown() {
        //use tracing_subscriber::EnvFilter;
        //tracing_subscriber::fmt()
        //    .with_env_filter(
        //        EnvFilter::builder()
        //            .with_default_directive(tracing::Level::INFO.into())
        //            .from_env_lossy(),
        //    )
        //    .with_ansi(true)
        //    .init();

        let (client, server) = tokio::io::duplex(1024);

        let engine_gateway = MyEngineGtwTest {
            deployments: Mutex::new(vec![]),
            msgs: vec![],
        };

        tokio::spawn(async move {
            Server::builder()
                .add_service(EngineServer::new(engine_gateway))
                .serve_with_incoming(tokio_stream::iter(vec![Ok::<_, std::io::Error>(server)]))
                .await
        });

        let client = new_engine_client_test(Some(client)).await;

        let task = TaskSelector::Environment("");
        let should_shutdown = Arc::new(AtomicBool::new(false));
        let mk_engine_task = |_, _: &_, _: &_, _, _| {
            let task: Box<dyn Task> = Box::new(EngineTaskTest::new());
            Ok::<_, EngineEvent>(task)
        };

        let mut deployment_mngr =
            DeploymentManager::new(&task, client, should_shutdown.clone(), Box::new(mk_engine_task));
        deployment_mngr.default_wait_time = Duration::from_millis(500);
        let fut = deployment_mngr.run();
        pin_mut!(fut);
        assert!(timeout(Duration::from_secs(1), &mut fut).await.is_err());

        should_shutdown.store(true, Ordering::Relaxed);
        assert!(timeout(Duration::from_secs(1), &mut fut).await.is_ok());
    }

    #[tokio::test]
    async fn test_deployment_manager_executing_task() {
        //use tracing_subscriber::EnvFilter;
        //tracing_subscriber::fmt()
        //    .with_env_filter(
        //        EnvFilter::builder()
        //            .with_default_directive(tracing::Level::INFO.into())
        //            .from_env_lossy(),
        //    )
        //    .with_ansi(true)
        //    .init();

        let (client, server) = tokio::io::duplex(1024);

        let engine_gateway = MyEngineGtwTest {
            deployments: Mutex::new(vec![Ok(Response::new(DeploymentInfo {
                organization_id: "".to_string(),
                cluster_id: "".to_string(),
                execution_id: Uuid::new_v4().to_string(),
                request_id: "".to_string(),
                r#type: 0,
                last_message_id: "".to_string(),
            }))]),
            msgs: vec![Ok(EngineMessageRx {
                message_id: "".to_string(),
                request: Some(engine_message_rx::Request::DeploymentRequest("".to_string())),
            })],
        };

        tokio::spawn(async move {
            Server::builder()
                .add_service(EngineServer::new(engine_gateway))
                .serve_with_incoming(tokio_stream::iter(vec![Ok::<_, std::io::Error>(server)]))
                .await
        });

        let client = new_engine_client_test(Some(client)).await;

        let should_shutdown = Arc::new(AtomicBool::new(false));
        let task = EngineTaskTest::new();
        let task_is_running = task.is_running.clone();
        let task_cancel = task.should_shutdown.clone();

        let mk_engine_task = move |_, _: &_, _: &_, _, _| {
            let task: Box<dyn Task> = Box::new(task.clone());
            Ok::<_, EngineEvent>(task)
        };

        let task = TaskSelector::Environment("");
        let mut deployment_mngr =
            DeploymentManager::new(&task, client, should_shutdown.clone(), Box::new(mk_engine_task));
        deployment_mngr.default_wait_time = Duration::from_millis(200);
        deployment_mngr.deadline_for_new_task = Duration::from_secs(1);
        let fut = deployment_mngr.run();
        pin_mut!(fut);

        assert!(timeout(Duration::from_secs(2), &mut fut).await.is_err());
        assert!(task_is_running.load(Ordering::Relaxed));

        task_cancel.store(true, Ordering::Relaxed);
        should_shutdown.store(true, Ordering::Relaxed);
        assert!(timeout(Duration::from_secs(2), &mut fut).await.is_ok());
        assert!(!task_is_running.load(Ordering::Relaxed));
    }
}
