use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, RecvError, RecvTimeoutError, Sender};
use serde::{Deserialize, Serialize};

use crate::utils::log_no_spam_builder;
use crate::utils::LogErrorOnDrop;
use core::fmt;
use core::fmt::Formatter;
use prometheus::{self, IntGauge};
use qovery_engine::logger::Logger;
use qovery_engine::models::{ProgressLevel, ProgressScope};
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tracing;

lazy_static! {
    static ref METRICS_NB_RUNNING_TASKS: IntGauge =
        register_int_gauge!("taskmanager_nb_running_tasks", "Number of tasks currently running").unwrap();
}

pub struct TaskManager {
    task_executor_tx: Sender<Box<dyn Task>>,
    task_executor_rx: Receiver<Box<dyn Task>>,
    should_stop: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    task_status_tx: Sender<Status>,
    task_status_rx: Receiver<Status>,
    running_tasks: IntGauge,
    running: bool,
    // We use a Mutex to provide interior mutability
    // because we wrap TaskManager into a RwLock.
    // Consuming threads_handle should be a final states
    threads_handle: Mutex<Vec<(String, JoinHandle<()>)>>,
    current_task: Arc<RwLock<Option<Box<dyn Task>>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (task_executor_tx, task_executor_rx) = unbounded::<Box<dyn Task>>();
        let (task_status_tx, task_status_rx) = unbounded::<Status>();
        let should_stop = Arc::new(AtomicBool::new(false));
        let is_stopped = Arc::new(AtomicBool::new(false));

        TaskManager {
            task_executor_tx,
            task_executor_rx,
            should_stop,
            is_stopped,
            task_status_tx,
            task_status_rx,
            running_tasks: METRICS_NB_RUNNING_TASKS.clone(),
            running: false,
            threads_handle: Mutex::new(Vec::with_capacity(2)),
            current_task: Arc::new(RwLock::new(None)),
        }
    }

    pub fn get_task_status_tx(&self) -> &Sender<Status> {
        &self.task_status_tx
    }
    pub fn get_task_status_rx(&self) -> &Receiver<Status> {
        &self.task_status_rx
    }

    pub fn add_task(&self, task: Box<dyn Task>) {
        let message = match self.remaining_tasks_to_run() {
            0 => Some("Task is going to be executed !".to_string()),
            nb_tasks => {
                info!("Task is queued. {} remaining tasks.", nb_tasks);
                Some(format!(
                    "Task is queued ({} tasks left) and will start when a worker is available.",
                    nb_tasks
                ))
            }
        };

        let status = Status::new(
            State::Waiting,
            message,
            ActionContext::new(
                ProgressScope::Queued,
                ProgressLevel::Info,
                task.id().to_string(),
                *task.created_at(),
            ),
        );

        task.send_status(status);

        // add internal task to queue
        let _ = self
            .task_executor_tx
            .send(task)
            .map_err(|err| error!("cannot enqueue task {}", err));
    }

    pub fn remaining_tasks_to_run(&self) -> usize {
        self.task_executor_rx.len() + self.running_tasks.get().max(0) as usize
    }

    pub fn wait_shutdown(&self) {
        while let Some((thread_name, handle)) = self.threads_handle.lock().unwrap().pop() {
            info!("Waiting for {} to shutdown", thread_name);
            let _ = handle
                .join()
                .map_err(|err| error!("Cannot join thread {}: {:?}", thread_name, err));
        }
    }
    pub fn cancel_current_task(&self) {
        let lock = self.current_task.read().unwrap();
        match &*lock {
            Some(task) => task.cancel(),
            None => {}
        }
    }

    /// gracefully end the remaining tasks but stop accepting new ones
    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::Release)
    }

    /// run task manager - only a single instance will run
    pub fn run(&mut self, logger: Box<dyn Logger>) -> Result<(), Error> {
        if self.running {
            return Err(Error::AlreadyRunning);
        }

        // only one run allowed
        self.running = true;
        let is_stopped = self.is_stopped.clone();
        let should_stop = self.should_stop.clone();
        let task_executor_rx = self.task_executor_rx.clone();
        let nb_running_tasks = self.running_tasks.clone();
        let current_task_lock = self.current_task.clone();
        let thread_name = "tm-task-processor";

        let th = thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(move || {
                let _drop_logger = LogErrorOnDrop::new(thread_name);
                let mut log_debug_waiting = log_no_spam_builder("no task to run, waiting...".to_string(), 60);

                while !should_stop.load(Acquire) || !task_executor_rx.is_empty() {
                    if should_stop.load(Relaxed) {
                        info!(
                            "TaskManager should stop, but still have {} pending tasks to process",
                            task_executor_rx.len()
                        );
                    }

                    let task = match task_executor_rx.recv_timeout(Duration::from_secs(1)) {
                        // Dequeue our task !
                        Ok(internal_task) => internal_task,

                        // No task to process, sleep and retry later
                        Err(RecvTimeoutError::Timeout) => {
                            log_debug_waiting();
                            continue;
                        }

                        // Channel is disconnected (dropped in the other end)
                        // We will not received any message anymore, exiting
                        Err(err) => {
                            error!("Cannot retrieve task to execute {}", err);
                            is_stopped.store(true, Release);
                            return;
                        }
                    };

                    nb_running_tasks.inc();

                    // Activate a tracing span with max level to add task log elements to tracing
                    // events of all levels
                    let task_span = span!(tracing::Level::INFO, "task", execution_id = task.id());
                    let _enter = task_span.enter();

                    let start_time = Instant::now();
                    {
                        let mut current_task = current_task_lock.write().unwrap();
                        current_task.replace(task);
                    }

                    let current_task = current_task_lock.read().unwrap();
                    let task = current_task.as_ref().unwrap();
                    task.run(logger.clone());
                    info!(
                        "task {} took {} sec to be executed",
                        &task.id(),
                        start_time.elapsed().as_secs()
                    );
                    info!("it remains {} tasks to be run", task_executor_rx.len());
                    nb_running_tasks.dec();
                }
                is_stopped.store(true, Release);
            })
            .unwrap();

        self.threads_handle.lock().unwrap().push((thread_name.to_string(), th));
        Ok(())
    }
}

pub trait Task: Send + Sync {
    fn created_at(&self) -> &DateTime<Utc>;
    fn id(&self) -> &str;
    fn send_status(&self, status: Status);
    fn run(&self, logger: Box<dyn Logger>);
    fn cancel(&self);
    fn cancel_checker(&self) -> Box<dyn Fn() -> bool>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ActionContext {
    pub scope: ProgressScope,
    pub level: ProgressLevel,
    pub execution_id: String,
    pub task_created_at: DateTime<Utc>,
}

impl ActionContext {
    pub fn new(
        scope: ProgressScope,
        level: ProgressLevel,
        execution_id: String,
        task_created_at: DateTime<Utc>,
    ) -> Self {
        ActionContext {
            scope,
            level,
            execution_id,
            task_created_at,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Status {
    pub status: State,
    pub message: Option<String>,
    pub context: ActionContext,
}

impl Status {
    pub fn new(status: State, message: Option<String>, context: ActionContext) -> Self {
        Status {
            status,
            message,
            context,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    Waiting,
    DeploymentInProgress,
    PauseInProgress,
    DeleteInProgress,
    Error,
    Deployed,
    Paused,
    Deleted,
    DeploymentError,
    PauseError,
    DeleteError,
    Canceled,
}

impl evmap::shallow_copy::ShallowCopy for Status {
    unsafe fn shallow_copy(&self) -> ManuallyDrop<Self> {
        ManuallyDrop::new(self.clone())
    }
}

#[derive(Debug)]
pub enum Error {
    AlreadyRunning,
    Recv(RecvError),
}

impl From<RecvError> for Error {
    fn from(err: RecvError) -> Self {
        Error::Recv(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::AlreadyRunning => write!(f, "Already Running"),
            Error::Recv(err) => write!(f, "RecvError {}", err),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::task_manager::task_manager::{ActionContext, State, Status, Task, TaskManager};
    use chrono::{DateTime, NaiveDateTime, Utc};
    use crossbeam_channel::Sender;
    use qovery_engine::logger::{Logger, StdIoLogger};
    use qovery_engine::models::{ProgressLevel, ProgressScope};
    use std::sync::atomic::Ordering::Acquire;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    #[derive(Clone)]
    pub struct WaitingTask {
        pub date: DateTime<Utc>,
        pub bytes: Vec<u8>,
        pub have_been_run: Arc<AtomicBool>,
        pub barrier_begin: Arc<Barrier>,
        pub barrier_end: Arc<Barrier>,
        pub task_status_tx: Sender<Status>,
    }

    impl WaitingTask {
        fn new(sender: Sender<Status>) -> WaitingTask {
            WaitingTask {
                date: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
                bytes: vec![],
                have_been_run: Arc::new(AtomicBool::new(false)),
                barrier_begin: Arc::new(Barrier::new(2)),
                barrier_end: Arc::new(Barrier::new(2)),
                task_status_tx: sender,
            }
        }
    }
    impl Task for WaitingTask {
        fn created_at(&self) -> &DateTime<Utc> {
            &self.date
        }

        fn id(&self) -> &str {
            "0"
        }

        fn send_status(&self, status: Status) {
            let _ = self.task_status_tx.send(status);
        }

        fn run(&self, _logger: Box<dyn Logger>) {
            self.barrier_begin.wait();
            let _ = self
                .have_been_run
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst);
            self.barrier_end.wait();
        }

        fn cancel(&self) {}

        fn cancel_checker(&self) -> Box<dyn Fn() -> bool> {
            Box::new(|| false)
        }
    }

    #[derive(Clone)]
    struct DummyTask {
        pub date: DateTime<Utc>,
    }
    impl Task for DummyTask {
        fn created_at(&self) -> &DateTime<Utc> {
            &self.date
        }
        fn id(&self) -> &str {
            "1"
        }
        fn send_status(&self, _status: Status) {}
        fn run(&self, _logger: Box<dyn Logger>) {}
        fn cancel(&self) {}

        fn cancel_checker(&self) -> Box<dyn Fn() -> bool> {
            Box::new(|| false)
        }
    }

    #[test]
    fn test_taskmanager_run() {
        let logger = StdIoLogger::new();
        let mut tm = TaskManager::new();
        tm.running_tasks = prometheus::IntGauge::new("abc", "degf").unwrap();
        let task = WaitingTask::new(tm.task_status_tx.clone());

        assert_eq!(tm.running_tasks.get(), 0);
        assert_eq!(task.have_been_run.load(Ordering::Acquire), false);
        tm.add_task(Box::new(task.clone()));

        let task_status_rx = tm.task_status_rx.clone();
        tm.run(Box::new(logger)).expect("Impossible to run task Manager");
        assert_eq!(task_status_rx.recv_timeout(Duration::from_secs(10)).is_ok(), true);

        task.barrier_begin.wait();
        assert_eq!(tm.running_tasks.get(), 1);
        task.barrier_end.wait();

        tm.stop();
        assert_eq!(tm.should_stop.load(Acquire), true);
        // Wait for task to be completed
        let mut nb_iter = 0;
        while tm.remaining_tasks_to_run() > 0 {
            std::thread::sleep(Duration::from_secs(1));
            nb_iter += 1;
            assert_ne!(nb_iter, 5);
        }
        assert_eq!(task.have_been_run.load(Ordering::Acquire), true);
        assert_eq!(tm.running_tasks.get(), 0);

        // Test that we clean the Internal Hashmap
    }

    #[test]
    fn test_taskmanager_cleanup() {
        let logger = StdIoLogger::new();
        let mut tm = TaskManager::new();
        tm.running_tasks = prometheus::IntGauge::new("abcd", "degf").unwrap();
        let task = WaitingTask::new(tm.task_status_tx.clone());
        let task_status_rx = tm.get_task_status_rx().clone();
        tm.run(Box::new(logger)).expect("Impossible to run task Manager");
        tm.add_task(Box::new(task.clone()));

        assert_eq!(task_status_rx.recv().unwrap().status, State::Waiting);

        task.barrier_begin.wait();
        task.barrier_end.wait();

        task.send_status(Status {
            status: State::Deleted,
            message: None,
            context: ActionContext {
                scope: ProgressScope::Queued,
                level: ProgressLevel::Debug,
                execution_id: "".to_string(),
                task_created_at: DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
            },
        });

        assert_eq!(task_status_rx.recv().unwrap().status, State::Deleted);

        tm.stop();
        assert_eq!(tm.wait_shutdown(), ());
    }

    #[test]
    fn test_taskmanager_graceful_shutdown() {
        let logger = StdIoLogger::new();
        let mut tm = TaskManager::new();
        tm.running_tasks = prometheus::IntGauge::new("abcde", "degf").unwrap();

        let task = DummyTask {
            date: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
        };
        tm.add_task(Box::new(task.clone()));
        tm.add_task(Box::new(task.clone()));
        tm.add_task(Box::new(task));

        assert_eq!(tm.remaining_tasks_to_run(), 3);
        tm.stop();
        tm.run(Box::new(logger)).expect("Impossible to run task Manager");

        assert_eq!(tm.wait_shutdown(), ());
        assert_eq!(tm.remaining_tasks_to_run(), 0);
    }
}
