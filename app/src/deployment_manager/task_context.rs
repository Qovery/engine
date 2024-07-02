use crate::tokio_utils;
use qovery_engine::engine_task::Task;
use std::sync::Arc;
use std::time::Duration;
use tracing::Span;

/// State when the engine is executing a task
pub struct TaskContext {
    pub task: Arc<dyn Task>,
    _handle: tokio::task::JoinHandle<()>,
}

impl TaskContext {
    pub fn spawn_new_task(task: Arc<dyn Task>) -> TaskContext {
        let span = Span::current();

        let task_handle = tokio_utils::launch_blocking_task({
            let task = task.clone();
            move || {
                let _guard = scopeguard::guard(span, |span| {
                    let _span = span.enter();
                    debug!("Task {} has terminated to run from blocking pool", task.id());
                });

                task.run();
            }
        });

        TaskContext {
            task,
            _handle: task_handle,
        }
    }

    pub async fn terminate_task(self) {
        if self.task.is_terminated() {
            return;
        }

        warn!("Canceling current task");
        self.task.cancel(false);
        info!("Task canceled, waiting for task to terminate");
        self.await_task_termination().await;
    }

    pub async fn await_task_termination(self) {
        if self.task.is_terminated() {
            return;
        }

        info!("Waiting for task to terminate");
        while (tokio::time::timeout(Duration::from_secs(10), self.task.await_terminated().recv()).await).is_err() {
            info!("Waiting for task to terminate");
        }
        info!("Task terminated");
    }
}

impl Drop for TaskContext {
    fn drop(&mut self) {
        info!("Dropping task context");
    }
}
