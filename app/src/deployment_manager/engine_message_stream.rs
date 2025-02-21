use crate::grpc::engine::StepRecord as GrpcStepRecord;
use crate::grpc::engine::{EngineMessageTx, Metrics, engine_message_tx};
use chrono::{DateTime, Utc};
use futures_util::{Stream, StreamExt, stream};
use prost_types::Timestamp;
use qovery_engine::events::io::EngineEvent as EngineEventIo;
use qovery_engine::events::{EngineEvent, EngineMsg, EngineMsgPayload};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{OwnedMutexGuard, watch};
use tokio::time::Instant;
use tracing::Span;
use tracing_futures::Instrument;

// Represent the context of the msg stream that the engine use to communicate with the gateway
// For now there is only metrics
pub struct EngineMsgStreamContext {
    msg_receiver: UnboundedReceiver<EngineMsg>,
    should_stop: Option<watch::Receiver<()>>,
}

impl EngineMsgStreamContext {
    pub fn new(msg_receiver: UnboundedReceiver<EngineMsg>) -> Self {
        EngineMsgStreamContext {
            msg_receiver,
            should_stop: None,
        }
    }
}

// Represent the context of the log stream that the engine use to communicate with the gateway
pub struct EngineLogStreamContext {
    log_receiver: UnboundedReceiver<EngineEvent>,
    log_buffer: Vec<EngineEventIo>,
    log_buffer_duration: Duration,
    should_stop: Option<watch::Receiver<()>>,
}

impl EngineLogStreamContext {
    pub fn new(log_receiver: UnboundedReceiver<EngineEvent>, buffer_duration: Duration) -> Self {
        EngineLogStreamContext {
            log_receiver,
            log_buffer: Vec::with_capacity(1024),
            log_buffer_duration: buffer_duration,
            should_stop: None,
        }
    }
}

// This is the stream that is used by the engine to communicate with the gateway
// It contains the log stream and the msg stream.
// The main complexity is to be able to resume the stream in case of failure, in order to re-mit last logs and messages.
// We use a mutex that is own by the stream. If the cnx with the gateway is lost, the stream is dropped and the mutex is released.
// As it is the mutex that old the state, we can re-create a new stream without losing the state.

// [engine gtw] <---GRPC---- [EngineMessageStream] <---channels--- [Engine Task]
// on cnx drop, the stream is dropped and the mutex is released
pub struct EngineMessageStream {
    stream: Box<dyn Stream<Item = EngineMessageTx> + Send + 'static>,
    span: Span,
}

impl EngineMessageStream {
    pub fn new(
        mut log_stream_context: OwnedMutexGuard<EngineLogStreamContext>,
        mut msg_stream_context: OwnedMutexGuard<EngineMsgStreamContext>,
        mut last_msg_memento: OwnedMutexGuard<Option<EngineMessageTx>>,
    ) -> (Self, watch::Sender<()>) {
        let (abort_handle_tx, abort_handle_rx) = watch::channel(());
        log_stream_context.should_stop = Some(abort_handle_rx.clone());
        msg_stream_context.should_stop = Some(abort_handle_rx);

        // We re-emit the last message in case of failure
        let (remit_last_msg, start_time_anchor): (Pin<Box<dyn Stream<Item = EngineMessageTx> + Send>>, DateTime<Utc>) =
            if let Some(last_msg) = last_msg_memento.clone() {
                let msg_id = last_msg.message_id.as_ref().unwrap();
                let start_time = DateTime::from_timestamp(msg_id.seconds, msg_id.nanos as u32).unwrap();
                (Box::pin(stream::once(std::future::ready(last_msg))), start_time)
            } else {
                (Box::pin(stream::empty()), Utc::now())
            };

        // Normal flow were we dequeue engine message
        let normal_flow = stream::select(
            stream::unfold(log_stream_context, Self::on_next_log),
            stream::unfold(msg_stream_context, Self::on_next_msg),
        )
        // We inspect each message to store the last_msg to re-emit it in case of failure
        // We set the message id of each msg to have a global order of the message
        .map({
            let start_time = Instant::now();
            move |mut msg| {
                let ts = start_time_anchor + start_time.elapsed();
                msg.message_id = Some(Timestamp {
                    seconds: ts.timestamp(),
                    nanos: ts.timestamp_subsec_nanos() as i32,
                });
                last_msg_memento.replace(msg.clone());
                msg
            }
        });

        let s = Self {
            stream: Box::new(remit_last_msg.chain(normal_flow).instrument(Span::current())),
            span: Span::current(),
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

            msg = ctx.msg_receiver.recv() => {
                msg.map(Self::convert_msg_to_grpc_message) },

             // Deployment asked to be aborted, we leave to release the mutex of the channel
            _ = should_stop.changed() => None
        };

        opt_engine_message_tx.map(|engine_message_tx| {
            ctx.should_stop = Some(should_stop);
            (engine_message_tx, ctx)
        })
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
                    let deadline = Instant::now() + ctx.log_buffer_duration;
                    // Buffer msg to avoid flooding the gateway
                    ctx.log_buffer.clear();
                    ctx.log_buffer.push(EngineEventIo::from(engine_event));

                    while ctx.log_buffer.len() < ctx.log_buffer.capacity() {
                        let engine_event = match ctx.log_receiver.try_recv() {
                            // msg is directly available dequeue it
                            Ok(engine_event) => engine_event,
                            // no message available, wait for our allocated budget if any
                            Err(TryRecvError::Empty) => match tokio::time::timeout_at(deadline, ctx.log_receiver.recv()).await {
                                    Ok(Some(engine_event)) => engine_event,
                                    Ok(None) => break,
                                    Err(_timeout) => break,
                            }
                            Err(TryRecvError::Disconnected) => break,
                        };

                        ctx.log_buffer.push(EngineEventIo::from(engine_event));
                    }

                    Some(Self::convert_logs_to_grpc_message(&ctx.log_buffer))
                }
                None => None,
            },

            // Deployment asked to be aborted, we leave to release the mutex of the channel
            _ = should_stop.changed() => None
        };

        opt_engine_message_tx.map(|engine_message_tx| {
            ctx.should_stop = Some(should_stop);
            (engine_message_tx, ctx)
        })
    }
}

impl Stream for EngineMessageStream {
    type Item = EngineMessageTx;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        unsafe { self.map_unchecked_mut(|s| s.stream.as_mut()).poll_next(cx) }
    }
}

impl Drop for EngineMessageStream {
    fn drop(&mut self) {
        let _span = self.span.enter();
        info!("engine message stream to gateway terminated");
    }
}

#[cfg(test)]
mod test {
    use crate::deployment_manager::deployment_context::DeploymentContext;

    use crate::grpc::engine::DeploymentInfo;

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
        let buffer_duration = Duration::from_millis(100);
        let buffer_deadline = buffer_duration + Duration::from_millis(50);
        let mut deployment = DeploymentContext::new(DeploymentInfo::default(), buffer_duration, Default::default());

        // Dropping the handle should terminate the stream
        let (_, _, mut stream, abort_handle) = deployment.get_message_stream().await;
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());
        drop(abort_handle);
        assert!(matches!(timeout(buffer_deadline, stream.next()).await, Ok(None)));

        drop(stream);
        // Log sent should be received and buffered
        let (log_tx, _, mut stream, abort_handle) = deployment.get_message_stream().await;
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
        let (_, _, mut stream, _abort_handle) = deployment.get_message_stream().await;
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
        let buffer_duration = Duration::from_millis(100);
        let buffer_deadline = buffer_duration + Duration::from_millis(50);
        let mut deployment = DeploymentContext::new(DeploymentInfo::default(), buffer_duration, Default::default());
        let (_, msg_tx, mut stream, abort_handle) = deployment.get_message_stream().await;

        let _ = msg_tx.send(engine_msg.clone());
        let msg = timeout(buffer_deadline * 2, stream.next()).await;
        assert!(matches!(msg, Ok(Some(_))));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());

        // Resuming the stream should re-send the previous messages
        drop(abort_handle);
        drop(stream);
        let (_, _, mut stream, _abort_handle) = deployment.get_message_stream().await;
        assert!(matches!(
            timeout(buffer_deadline, stream.next()).await,
            Ok(Some(m)) if m.message_id.as_ref().unwrap() == &msg.unwrap().unwrap().message_id.unwrap()
        ));
        assert!(timeout(buffer_deadline, stream.next()).await.is_err());
    }
}
