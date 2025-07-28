use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::logger::Logger;
use std::thread;
use std::time::Duration;
use tracing::Span;

pub fn send_progress_on_long_task_with_message<R, F>(
    logger: Box<dyn Logger>,
    event_details: EventDetails,
    waiting_message: Option<String>,
    long_task: F,
    message_interval: Duration,
    max_duration: Option<Duration>,
) -> R
where
    F: FnOnce() -> R,
{
    let logger = logger.clone_dyn();

    let (tx, rx) = oneshot::channel();
    let span = Span::current();

    // monitor thread to notify user while the blocking task is executed
    let handle = thread::Builder::new().name("task-monitor".to_string()).spawn(move || {
        // stop the thread when the blocking task is done
        let _span = span.enter();
        let waiting_message = waiting_message.unwrap_or_else(|| "no message ...".to_string());
        let event_message = EventMessage::new_from_safe(waiting_message);

        let start_time = std::time::Instant::now();

        loop {
            // check if we've exceeded the maximum duration
            if let Some(max_dur) = max_duration {
                if start_time.elapsed() >= max_dur {
                    let timeout_message =
                        EventMessage::new_from_safe("Task exceeded maximum duration, stopping monitoring".to_string());
                    logger.log(EngineEvent::Info(event_details.clone(), timeout_message));
                    break;
                }
            }

            // do notify users here
            logger.log(EngineEvent::Info(event_details.clone(), event_message.clone()));

            match rx.recv_timeout(message_interval) {
                Ok(_) => break, // task completed
                Err(oneshot::RecvTimeoutError::Timeout) => {
                    // timeout reached, continue the loop to send another progress message
                    continue;
                }
                Err(oneshot::RecvTimeoutError::Disconnected) => break, // sender dropped
            }
        }
    });

    let blocking_task_result = long_task();
    let _ = tx.send(()); // Send completion signal
    let _ = handle.map(|it| it.join());

    blocking_task_result
}
