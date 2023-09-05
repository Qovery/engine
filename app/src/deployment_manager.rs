use crate::grpc::engine::Metrics;
use crate::grpc::engine::StepRecord as GrpcStepRecord;
use crate::grpc::engine::{engine_message_tx, DeploymentInfo, DeploymentType, EngineMessageTx};
use crate::metrics::METRICS_NB_RUNNING_TASKS;
use crate::tokio_utils;
use chrono::Utc;
use futures_util::{stream, Stream, StreamExt};
use prost_types::Timestamp;
use qovery_engine::engine_task::Task;
use qovery_engine::events::io::EngineEvent as EngineEventIo;
use qovery_engine::events::{EngineEvent, EngineMsg, EngineMsgPayload};
use std::future::Future;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, watch, Mutex, OwnedMutexGuard};
use tokio::task::{JoinError, JoinHandle};

// A single deployment can receive N tasks.
// A task represent a deployment group/engine request.
// The same engine is going to receive all the deployment group/task for a deployment
pub struct DeploymentManager {
    // The information regarding the current deployment (execution_id, cluster_id, orga, deployment_type)
    deployment_info: DeploymentInfo,

    // The context of the current task running by the engine
    task: Option<TaskContext>,
    waker: Option<Waker>,

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
    task: Arc<Box<dyn Task>>,    // the engine task
    task_handle: JoinHandle<()>, // the handle for the "thread"/tokio task where is runinng the task
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

impl DeploymentManager {
    pub fn new() -> Self {
        METRICS_NB_RUNNING_TASKS.set(0);
        let (log_engine_tx, log_engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
        let (msg_engine_tx, msg_engine_rx) = mpsc::unbounded_channel::<EngineMsg>();
        Self {
            deployment_info: Default::default(),
            task: None,
            waker: None,
            log_tx: log_engine_tx,
            log_rx: Arc::new(Mutex::new(EngineLogStreamContext::new(log_engine_rx))),

            msg_tx: msg_engine_tx,
            msg_rx: Arc::new(Mutex::new(EngineMsgStreamContext::new(msg_engine_rx))),

            // To keep the last message in case the cnx with upstream broke and that we need to re-emit the last message
            // on resume
            last_msg_memento: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_task(&mut self, task: Box<dyn Task>) {
        let task = Arc::new(task);
        let task_handle = tokio_utils::launch_blocking_task({
            let task = task.clone();
            move || {
                task.run();
            }
        });

        // We dont check before spawning the new task if the old one is terminated
        // because engine task drop the logger before the task is terminated
        // because pushing archive to s3 may take times.
        // So we may receive a new task for the deployment while the previous task is running (pushing stuff to s3)
        self.task = Some(TaskContext { task, task_handle });
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    pub fn get_task(&self) -> Option<&dyn Task> {
        self.task.as_ref().map(|t| &**t.task)
    }

    pub fn remove_task(&mut self) {
        self.task = None;
    }

    // Check if this deployment manager is actually running a deployment or not.
    // Even if there is no task running, this does not means a deployment is not ongoing.
    pub fn get_current_deployment(&self) -> Option<&DeploymentInfo> {
        if self.deployment_info.r#type != DeploymentType::Unknown as i32 {
            Some(&self.deployment_info)
        } else {
            None
        }
    }

    pub fn set_current_deployment(&mut self, deployment_info: DeploymentInfo) {
        METRICS_NB_RUNNING_TASKS.inc();
        self.deployment_info = deployment_info;
    }

    pub fn remove_current_deployment(&mut self) {
        METRICS_NB_RUNNING_TASKS.dec();
        self.deployment_info = Default::default();
        let (tx, rx) = mpsc::unbounded_channel::<EngineEvent>();
        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<EngineMsg>();
        self.log_tx = tx;
        self.log_rx = Arc::new(Mutex::new(EngineLogStreamContext::new(rx)));
        self.msg_tx = msg_tx;
        self.msg_rx = Arc::new(Mutex::new(EngineMsgStreamContext::new(msg_rx)));
        self.last_msg_memento = Arc::new(Mutex::new(None));
        self.remove_task();
    }

    pub fn is_task_terminated(&self) -> bool {
        if let Some(task) = &self.task {
            task.task_handle.is_finished()
        } else {
            true
        }
    }

    // We record the last message id we got from the gateway in order
    // to be able to restart exactly to the last message in case of resume from error
    pub fn set_last_message_id(&mut self, last_id: String) {
        self.deployment_info.last_message_id = last_id;
    }

    // Return a stream with a sender to feed the stream of event message.
    // The oneshot sender, is an abort handle to know and/or notify the stream to stop
    // and release ownership of the underlying channel.
    // Calling get_message_channel without releasing the oneshot sender will lead to
    // this async never returning
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
}

// This future allows to wait for the current task to finish
// If no task is present, it will never complete
impl Future for DeploymentManager {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        return match this.task.as_mut() {
            Some(handle) => Pin::new(&mut handle.task_handle).poll(cx),
            None => {
                this.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        };
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
    use crate::deployment_manager::{DeploymentManager, EngineMessageStream};
    use futures_util::StreamExt;
    use qovery_engine::errors::EngineError;
    use qovery_engine::events;
    use qovery_engine::events::{EngineEvent, EngineMsg, EngineMsgPayload, EnvironmentStep, Stage, Transmitter};
    use qovery_engine::io_models::QoveryIdentifier;
    use qovery_engine::metrics_registry::{StepLabel, StepName, StepRecord};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;
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
        let deployment = DeploymentManager::new();
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
        let deployment = DeploymentManager::new();
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
}
