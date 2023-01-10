use crate::grpc::engine::{engine_message_tx, DeploymentInfo, DeploymentType, EngineMessageTx};
use crate::metrics::METRICS_NB_RUNNING_TASKS;
use crate::tokio_utils;
use futures_util::{stream, Stream};
use qovery_engine::engine_task::Task;
use qovery_engine::events::io::EngineEvent as EngineEventIo;
use qovery_engine::events::EngineEvent;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, oneshot, Mutex};
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
    tx: UnboundedSender<EngineEvent>,
    rx: Arc<Mutex<UnboundedReceiver<EngineEvent>>>,
}

struct TaskContext {
    task: Arc<Box<dyn Task>>,    // the engine task
    task_handle: JoinHandle<()>, // the handle for the "thread"/tokio task where is runinng the task
}

impl DeploymentManager {
    pub fn new() -> Self {
        METRICS_NB_RUNNING_TASKS.set(0);
        let (engine_tx, engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
        Self {
            deployment_info: Default::default(),
            task: None,
            waker: None,
            tx: engine_tx,
            rx: Arc::new(Mutex::new(engine_rx)),
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
        self.tx = tx;
        self.rx = Arc::new(Mutex::new(rx));
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
    pub async fn get_message_channel(
        &mut self,
    ) -> (
        UnboundedSender<EngineEvent>,                         // To feed the stream
        impl Stream<Item = EngineMessageTx> + Send + 'static, // The stream that receive the EngineEvent
        oneshot::Sender<()>, // To know/stop the stream and release the rx side of the channel
    ) {
        let on_drop = scopeguard::guard((), |_| {
            info!("engine message forwarder to gateway terminated");
        });
        let (abort_deployment_tx, abort_deployment_rx) = oneshot::channel::<()>();
        let engine_msg_rx = self.rx.clone().lock_owned().await;
        let msg_to_upstream = stream::unfold(
            (engine_msg_rx, abort_deployment_rx, on_drop),
            |(mut engine_msg_rx, mut should_stop, on_drop)| async move {
                tokio::select! {
                    biased;

                    msg = engine_msg_rx.recv() => match msg {
                        Some(engine_event) => {
                            let event_io = EngineEventIo::from(engine_event);
                            let grpc_message = EngineMessageTx {
                                message: Some(engine_message_tx::Message::Log(
                                    serde_json::to_string(&event_io).unwrap_or_default(),
                                )),
                            };
                            Some((grpc_message, (engine_msg_rx, should_stop, on_drop)))
                        }
                        None => None,
                    },

                    // Deployment asked to be aborted, we leave to release the mutex of the channel
                    _ = &mut should_stop => None
                }
            },
        );
        (self.tx.clone(), msg_to_upstream, abort_deployment_tx)
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
