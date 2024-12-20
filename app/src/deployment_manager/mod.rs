mod deployment_context;
mod engine_message_stream;
mod task_context;
mod upstream_gtw_context;

use crate::deployment_manager::deployment_context::DeploymentContext;
use crate::deployment_manager::task_context::TaskContext;
use crate::deployment_manager::upstream_gtw_context::UpstreamGatewayContext;
use crate::grpc::engine::engine_message_rx;
use crate::grpc::engine::DeploymentRequest;
use crate::grpc::engine::{DeploymentInfo, DeploymentType};
use crate::grpc::GrpcEngineClient;
use crate::metrics::METRICS_NB_RUNNING_TASKS;
use crate::models::TaskSelector;
use futures_util::StreamExt;
use qovery_engine::engine_task::Task;
use qovery_engine::events::EngineEvent;
use qovery_engine::log_file_writer::LogFileWriter;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tonic::Code;
use tracing::{error, field, Instrument, Level};

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

impl DeploymentContext {}

type MkEngineTask = Box<
    dyn Fn(
            String,
            &DeploymentInfo,
            &GrpcEngineClient,
            Box<dyn Logger>,
            Box<dyn MetricsRegistry>,
            LogFileWriter,
        ) -> Result<Arc<dyn Task>, EngineEvent>
        + Send,
>;

pub struct DeploymentManager {
    default_wait_time: Duration,
    deadline_for_new_task: Duration,
    deployment_request: DeploymentRequest,
    engine_client: GrpcEngineClient,
    should_shutdown: Arc<AtomicBool>,
    is_connected_to_gtw: Arc<AtomicBool>,
    mk_engine_task: MkEngineTask,
    log_file_writer: LogFileWriter,
}

impl DeploymentManager {
    pub fn new(
        task_type: &TaskSelector,
        engine_client: GrpcEngineClient,
        should_shutdown: Arc<AtomicBool>,
        is_connected_to_gtw: Arc<AtomicBool>,
        mk_engine_task: MkEngineTask,
        log_file_writer: LogFileWriter,
    ) -> Self {
        METRICS_NB_RUNNING_TASKS.set(0);
        let deployment_request = match task_type {
            TaskSelector::Infrastructure => DeploymentRequest {
                deployment_type: DeploymentType::Infrastructure as i32,
            },
            TaskSelector::Environment => DeploymentRequest {
                deployment_type: DeploymentType::Environment as i32,
            },
        };

        Self {
            default_wait_time: Duration::from_secs(1),
            deadline_for_new_task: Duration::from_secs(15),
            deployment_request,
            engine_client,
            should_shutdown,
            is_connected_to_gtw,
            mk_engine_task,
            log_file_writer,
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

    /// Engine has nothing to do, pool the gtw to get a new deployment to execute
    async fn seek_new_deployment(&mut self) -> (DeploymentManagerState, Option<Duration>) {
        let deployment_info = match self.engine_client.get_new_deployment(self.deployment_request).await {
            Ok(deployment_info) => deployment_info.into_inner(),
            Err(err) => {
                if err.code() == Code::NotFound {
                    self.is_connected_to_gtw.store(true, Ordering::Relaxed);
                    info!("No deployment found, waiting for a new one");
                } else {
                    self.is_connected_to_gtw.store(false, Ordering::Relaxed);
                    error!("Error while getting new deployment: {}", err);
                }
                return (DeploymentManagerState::SeekingNewDeployment {}, Some(self.default_wait_time));
            }
        };

        self.is_connected_to_gtw.store(true, Ordering::Relaxed);
        info!("Got new deployment for: {:?}", deployment_info);
        let next_state = DeploymentManagerState::ExecutingDeployment {
            deployment: DeploymentContext::new(deployment_info, Duration::from_secs(1), self.log_file_writer.clone()),
        };

        (next_state, None)
    }

    /// We have a new deployment to execute, but no task yet. Contact the gateway to claim the deployment and get a task to execute
    async fn execute_deployment(
        &mut self,
        mut deployment: DeploymentContext,
    ) -> (DeploymentManagerState, Option<Duration>) {
        info!("Starting to execute deployment");
        let (mut msg_stream, close_upstream_tx, logger, metrics_registry, log_file_writer) =
            match deployment.execute_deployment(&mut self.engine_client).await {
                Ok(upstream_msg) => upstream_msg,
                Err(err) => {
                    return match err.code() {
                        Code::NotFound | Code::DeadlineExceeded => {
                            error!(
                                "Deployment cannot be executed due to {:?} for {:?}",
                                err, &deployment.deployment_info
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
                            let task = (self.mk_engine_task)(payload, &deployment.deployment_info, &self.engine_client, logger.clone(), metrics_registry.clone(), log_file_writer.clone());
                            let upstream = UpstreamGatewayContext::new(msg_stream, close_upstream_tx, logger, metrics_registry, log_file_writer);
                            match task {
                                Ok(task) => {
                                    let next_step = DeploymentManagerState::ExecutingDeploymentTask {
                                        deployment,
                                        task: TaskContext::spawn_new_task(task),
                                        upstream_gtw: upstream,
                                    };
                                    (next_step, None)
                                }
                                Err(err) => {
                                    deployment.hard_abort_deployment(err).await;
                                    upstream.await_termination().await;
                                    (DeploymentManagerState::SeekingNewDeployment {}, None)
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
            Ok((msg_stream, close_upstream_tx, logger, metrics_registry, log_file_writer)) => {
                info!("Resumed connectivity with gtw");
                let next_step = DeploymentManagerState::ExecutingDeploymentTask {
                    deployment,
                    task,
                    upstream_gtw: UpstreamGatewayContext::new(
                        msg_stream,
                        close_upstream_tx,
                        logger,
                        metrics_registry,
                        log_file_writer,
                    ),
                };
                (next_step, None)
            }

            Err(err) => match err.code() {
                Code::NotFound | Code::DeadlineExceeded => {
                    if task.task.is_terminated() {
                        info!("Deployment not found anymore and task is already terminated");
                    } else {
                        error!(
                            "Deployment cannot be executed anymore to resume task ??? {:?} for {:?}",
                            err, &deployment.deployment_info
                        );
                    }
                    task.terminate_task().await;
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
                    info!("No new message after {}s, assuming deployment is terminated", self.deadline_for_new_task.as_secs());
                    task.terminate_task().await;
                    deployment.terminate_deployment().await;
                    upstream.await_termination().await;

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
                                    let new_task = (self.mk_engine_task)(payload, &deployment.deployment_info, &self.engine_client, upstream.logger(), upstream.metrics_registry(), upstream.log_file_writer().clone());
                                    match new_task {
                                        Ok(new_task) => {
                                            task.await_task_termination().await;
                                            task = TaskContext::spawn_new_task(new_task);
                                        }
                                        Err(err) => {
                                            task.terminate_task().await;
                                            deployment.hard_abort_deployment(err).await;
                                            upstream.await_termination().await;

                                            return (DeploymentManagerState::SeekingNewDeployment {}, None);
                                        }
                                    }
                                }
                                Some(engine_message_rx::Request::DeploymentCancelRequest(cancel_type)) => {
                                    info!("Received cancel request: {:?}", msg);
                                    task.task.cancel(match cancel_type {
                                       0 => false, // CancelType::Standard
                                       1 => true, // CancelType::Forced
                                       _ => {
                                           error!("Invalid variant for deployment_cancel_request received from grpc server. Update the protobuf !");
                                           false // Unknown  CancelType::Standard
                                       },
                                    });
                                }
                                Some(engine_message_rx::Request::Terminated(_)) => {
                                    info!("Received terminated message for deployment: {:?}", msg);
                                    task.terminate_task().await;
                                    deployment.terminate_deployment().await;
                                    upstream.await_termination().await;

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
}

#[cfg(test)]
mod test {
    use crate::deployment_manager::DeploymentManager;
    use crate::grpc::engine::engine_server::{Engine, EngineServer};
    use crate::grpc::engine::{
        engine_message_rx, ClusterCredentialsUpdate, DeploymentInfo, DeploymentRequest, EngineMessageRx,
        EngineMessageTx, GitTokenRequest, GitTokenResponse, ServiceVersionRequest, ServiceVersionResponse,
    };
    use crate::grpc::test::new_engine_client_test;
    use crate::models::TaskSelector;
    use futures_util::{pin_mut, stream, Stream, StreamExt};
    use qovery_engine::engine_task::Task;

    use qovery_engine::events::EngineEvent;

    use qovery_engine::environment::models::abort::{Abort, AbortStatus, AtomicAbortStatus};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, RwLock};
    use std::thread;
    use std::time::Duration;
    use tokio::sync::broadcast;
    use tokio::sync::broadcast::Receiver;
    use tokio::time::timeout;
    use tonic::transport::Server;
    use tonic::{Request, Response, Status, Streaming};
    use uuid::Uuid;

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

        async fn update_cluster_credentials(
            &self,
            _request: Request<ClusterCredentialsUpdate>,
        ) -> Result<Response<()>, Status> {
            Err(Status::unimplemented("Not implemented"))
        }
    }

    #[derive(Clone)]
    pub struct EngineTaskTest {
        #[allow(clippy::type_complexity)]
        is_terminated: Arc<(RwLock<Option<broadcast::Sender<()>>>, broadcast::Receiver<()>)>,
        should_shutdown: Arc<AtomicAbortStatus>,
        is_running: Arc<AtomicBool>,
    }

    impl EngineTaskTest {
        pub fn new() -> Self {
            Self {
                is_terminated: {
                    let (tx, rx) = broadcast::channel(1);
                    Arc::new((RwLock::new(Some(tx)), rx))
                },
                should_shutdown: Arc::new(AtomicAbortStatus::new(AbortStatus::None)),
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

            while !self.should_shutdown.load(Ordering::Relaxed).should_cancel() {
                thread::sleep(Duration::from_millis(100));
            }
            info!("Task terminated to run");
        }

        fn cancel(&self, force_requested: bool) -> bool {
            self.should_shutdown.store(
                match force_requested {
                    true => AbortStatus::UserForceRequested,
                    false => AbortStatus::Requested,
                },
                Ordering::Relaxed,
            );
            true
        }

        fn cancel_checker(&self) -> Box<dyn Abort> {
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

        let task = TaskSelector::Environment;
        let should_shutdown = Arc::new(AtomicBool::new(false));
        let is_connected_to_gtw = Arc::new(AtomicBool::new(false));
        let mk_engine_task = |_, _: &_, _: &_, _, _, _| {
            let task: Arc<dyn Task> = Arc::new(EngineTaskTest::new());
            Ok::<_, EngineEvent>(task)
        };

        let mut deployment_mngr = DeploymentManager::new(
            &task,
            client,
            should_shutdown.clone(),
            is_connected_to_gtw.clone(),
            Box::new(mk_engine_task),
            Default::default(),
        );
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
                execution_start_deadline: None,
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
        let is_connected_to_gtw = Arc::new(AtomicBool::new(false));
        let task = EngineTaskTest::new();
        let task_is_running = task.is_running.clone();
        let task_cancel = task.should_shutdown.clone();

        let mk_engine_task = move |_, _: &_, _: &_, _, _, _| {
            let task: Arc<dyn Task> = Arc::new(task.clone());
            Ok::<_, EngineEvent>(task)
        };

        let task = TaskSelector::Environment;
        let mut deployment_mngr = DeploymentManager::new(
            &task,
            client,
            should_shutdown.clone(),
            is_connected_to_gtw.clone(),
            Box::new(mk_engine_task),
            Default::default(),
        );
        deployment_mngr.default_wait_time = Duration::from_millis(200);
        deployment_mngr.deadline_for_new_task = Duration::from_secs(1);
        let fut = deployment_mngr.run();
        pin_mut!(fut);

        assert!(timeout(Duration::from_secs(2), &mut fut).await.is_err());
        assert!(task_is_running.load(Ordering::Relaxed));
        assert!(is_connected_to_gtw.load(Ordering::Relaxed));

        task_cancel.store(AbortStatus::Requested, Ordering::Relaxed);
        should_shutdown.store(true, Ordering::Relaxed);
        assert!(timeout(Duration::from_secs(7), &mut fut).await.is_ok());
        assert!(!task_is_running.load(Ordering::Relaxed));
    }

    struct EngineGtwTestThatDisconnect {
        deployments: Mutex<Vec<Result<Response<DeploymentInfo>, Status>>>,
        msgs: Vec<Result<EngineMessageRx, Status>>,
        nb_disconnect: AtomicUsize,
        disconnect_after: Duration,
        nb_exec_deployment_called: Arc<AtomicUsize>,
    }

    #[tonic::async_trait]
    impl Engine for EngineGtwTestThatDisconnect {
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
            self.nb_exec_deployment_called.fetch_add(1, Ordering::Relaxed);
            if self.nb_disconnect.fetch_add(1, Ordering::Relaxed) <= 1 {
                // freeze the cnx and disconnect
                tokio::time::sleep(self.disconnect_after).await;
                return Err(Status::unavailable("Disconnected"));
            }

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

        async fn update_cluster_credentials(
            &self,
            _request: Request<ClusterCredentialsUpdate>,
        ) -> Result<Response<()>, Status> {
            Err(Status::unimplemented("Not implemented"))
        }
    }

    #[tokio::test]
    async fn test_deployment_manager_not_executing_task_after_deadline() {
        let (client, server) = tokio::io::duplex(1024);

        // The start should be claimed before 1sec
        let execution_start_deadline = Duration::from_secs(2);
        let nb_exec_deployment_called = Arc::new(AtomicUsize::new(0));
        let engine_gateway = EngineGtwTestThatDisconnect {
            deployments: Mutex::new(vec![Ok(Response::new(DeploymentInfo {
                organization_id: "".to_string(),
                cluster_id: "".to_string(),
                execution_id: Uuid::new_v4().to_string(),
                request_id: "".to_string(),
                r#type: 0,
                last_message_id: "".to_string(),
                execution_start_deadline: Some(prost_types::Duration::try_from(execution_start_deadline).unwrap()),
            }))]),
            msgs: vec![Ok(EngineMessageRx {
                message_id: "".to_string(),
                request: Some(engine_message_rx::Request::DeploymentRequest("".to_string())),
            })],
            nb_disconnect: AtomicUsize::new(0),
            disconnect_after: execution_start_deadline - Duration::from_secs(1),
            nb_exec_deployment_called: nb_exec_deployment_called.clone(),
        };

        tokio::spawn(async move {
            Server::builder()
                .add_service(EngineServer::new(engine_gateway))
                .serve_with_incoming(tokio_stream::iter(vec![Ok::<_, std::io::Error>(server)]))
                .await
        });

        let client = new_engine_client_test(Some(client)).await;

        let task = EngineTaskTest::new();
        let _task = task.clone();
        let task_is_running = task.is_running.clone();

        let mk_engine_task = move |_, _: &_, _: &_, _, _, _| {
            let task: Arc<dyn Task> = Arc::new(task.clone());
            Ok::<_, EngineEvent>(task)
        };

        let task = TaskSelector::Environment;
        let mut deployment_mngr = DeploymentManager::new(
            &task,
            client,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Box::new(mk_engine_task),
            Default::default(),
        );
        deployment_mngr.default_wait_time = Duration::from_millis(100);
        deployment_mngr.deadline_for_new_task = Duration::from_secs(1);
        let fut = deployment_mngr.run();
        pin_mut!(fut);

        // Drive the deployment for 5secs
        assert!(timeout(execution_start_deadline * 3, &mut fut).await.is_err());

        // Be sure that our task is not running and has not been executed
        assert!(!task_is_running.load(Ordering::Relaxed));
        assert!(!_task.is_terminated());
        // it should have been called twice, and no more as deadline should have elapsed after
        assert_eq!(nb_exec_deployment_called.load(Ordering::Relaxed), 2);
    }
}
