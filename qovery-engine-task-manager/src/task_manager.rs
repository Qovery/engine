use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, RecvError, Sender, TryRecvError};
use evmap::{ReadHandle, WriteHandle};
use serde::{Deserialize, Serialize};

use crate::utils::log_no_spam_builder;
use crate::utils::LogErrorOnDrop;
use core::fmt;
use prometheus::{self, IntGauge};
use qovery_engine::models::{ProgressLevel, ProgressScope};
use core::fmt::Formatter;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tracing;

pub type Id = String;
pub type GroupId = Id;
pub type Message = Result<InternalTask, Error>;

lazy_static! {
    static ref METRICS_NB_RUNNING_TASKS: IntGauge =
        register_int_gauge!("taskmanager_nb_running_tasks", "Number of tasks currently running").unwrap();
}

pub struct TaskManager {
    task_executor_tx: Sender<InternalTask>,
    task_executor_rx: Receiver<InternalTask>,
    should_stop: Arc<AtomicBool>,
    is_stopped: Arc<AtomicBool>,
    status_by_task_id_r: Mutex<ReadHandle<Id, Status>>,
    status_by_task_id_w: Arc<Mutex<WriteHandle<Id, Status>>>,
    task_status_tx: Sender<Message>,
    task_status_rx: Receiver<Message>,
    running_tasks: IntGauge,
    running: bool,
    // We use a Mutex to provide interior mutability
    // because we wrap TaskManager into a RwLock.
    // Consuming threads_handle should be a final states
    threads_handle: Mutex<Vec<(String, JoinHandle<()>)>>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (task_executor_tx, task_executor_rx) = unbounded::<InternalTask>();
        let (status_by_task_id_r, status_by_task_id_w) = evmap::new::<Id, Status>();
        let status_by_task_id_r = Mutex::new(status_by_task_id_r);
        let status_by_task_id_w = Arc::new(Mutex::new(status_by_task_id_w));
        let (task_status_tx, task_status_rx) = unbounded::<Message>();
        let should_stop = Arc::new(AtomicBool::new(false));
        let is_stopped = Arc::new(AtomicBool::new(false));

        TaskManager {
            task_executor_tx,
            task_executor_rx,
            should_stop,
            is_stopped,
            status_by_task_id_r,
            status_by_task_id_w,
            task_status_tx,
            task_status_rx,
            running_tasks: METRICS_NB_RUNNING_TASKS.clone(),
            running: false,
            threads_handle: Mutex::new(Vec::with_capacity(2)),
        }
    }

    pub fn add_task(&self, task: Box<dyn Task>) {
        add_task(
            &self.task_executor_tx,
            &self.task_status_tx,
            self.remaining_tasks_to_run(),
            task,
        );
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

    pub fn get_task_status(&self, id: &str) -> Option<Status> {
        self.status_by_task_id_r
            .lock()
            .unwrap()
            .get_one(id)
            .map(|status| status.as_ref().clone())
    }

    pub fn get_task_status_by_group_id(&self, group_id: &str) -> Option<Status> {
        // id and group_id should be unique (I know, this is crazy assumption, but let's do this:)),
        // so we use the same data structure to store them all
        self.get_task_status(group_id)
    }

    /// gracefully end the remaining tasks but stop accepting new ones
    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::Release)
    }

    /// run task manager - only a single instance will run
    pub fn run(&mut self) -> Result<Receiver<Message>, Error> {
        if self.running {
            return Err(Error::AlreadyRunning);
        }

        // only one run allowed
        self.running = true;

        let (task_status_forwarder_tx, task_status_forwarder_rx) = unbounded::<Message>();

        // Task Manager keeps track of task status internally.
        // So we subscribe to the notification channel and update our internal hashmap
        // Once done, we forward the task's status update to the subscriber of the taskManager (the one calling .run())
        {
            let is_stopped = self.is_stopped.clone();
            let task_status_rx = self.task_status_rx.clone();
            let status_by_task_id_w = self.status_by_task_id_w.clone();
            let thread_name = "tm-task-status-updater";
            let func = move || {
                let _drop_logger = LogErrorOnDrop::new(thread_name);
                while !is_stopped.load(Ordering::Acquire) || !task_status_rx.is_empty() {
                    if is_stopped.load(Relaxed) {
                        info!(
                            "{} should stop, but still have {} pending tasks to process",
                            thread_name,
                            task_status_rx.len()
                        );
                    }

                    let task_status = match task_status_rx.try_recv() {
                        Ok(task_status) => task_status,

                        // No task to process, sleep and retry later
                        Err(TryRecvError::Empty) => {
                            thread::sleep(Duration::from_secs(1));
                            continue;
                        }

                        // Channel is disconnected (dropped in the other end)
                        // We will not received any message anymore, exiting
                        Err(err) => {
                            error!("Cannot retrieve task status {}", err);
                            return;
                        }
                    };

                    if handle_task_status_update(task_status, &task_status_forwarder_tx, &status_by_task_id_w).is_err()
                    {
                        return;
                    }
                }
            };

            let th = thread::Builder::new()
                .name(thread_name.to_string())
                .spawn(func)
                .unwrap();
            self.threads_handle.lock().unwrap().push((thread_name.to_string(), th));
        };

        let is_stopped = self.is_stopped.clone();
        let should_stop = self.should_stop.clone();
        let task_status_tx = self.task_status_tx.clone();
        let task_executor_tx = self.task_executor_tx.clone();
        let task_executor_rx = self.task_executor_rx.clone();
        let nb_running_tasks = self.running_tasks.clone();
        let thread_name = "tm-task-processor";

        let th = thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(move || {
                let _drop_logger = LogErrorOnDrop::new(thread_name);
                let mut log_debug_waiting = log_no_spam_builder("no task to run, waiting...", 60);

                while !should_stop.load(Acquire) || !task_executor_rx.is_empty() {
                    if should_stop.load(Relaxed) {
                        info!(
                            "TaskManager should stop, but still have {} pending tasks to process",
                            task_executor_rx.len()
                        );
                    }

                    let internal_task = match task_executor_rx.try_recv() {
                        // Dequeue our task !
                        Ok(internal_task) => internal_task,

                        // No task to process, sleep and retry later
                        Err(TryRecvError::Empty) => {
                            log_debug_waiting();
                            thread::sleep(Duration::from_secs(1));
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
                    let task_span = span!(
                        tracing::Level::INFO,
                        "task",
                        execution_id = internal_task.status.context.execution_id_short().as_str(),
                    );
                    let _enter = task_span.enter();

                    match internal_task.task.pre_run() {
                        PreRun::Yes => {
                            let start_time = Instant::now();
                            internal_task.task.run(&task_status_tx);
                            info!(
                                "task {} took {} sec to be executed",
                                internal_task.task.id(),
                                start_time.elapsed().as_secs()
                            );
                            info!("it remains {} tasks to be run", task_executor_rx.len());
                        }

                        PreRun::NoAndQueueTail => {
                            // postpone the task
                            info!(
                                "postpone task group id {} with id {}",
                                internal_task.task.group_id(),
                                internal_task.task.id()
                            );

                            // re-add the task
                            add_task(
                                &task_executor_tx,
                                &task_status_tx,
                                task_executor_rx.len() + nb_running_tasks.get().max(0) as usize,
                                internal_task.task,
                            );
                        }

                        PreRun::NoAndRemove => {
                            warn!("Dropping task as PreRun asked to do it !");
                            internal_task.send_status(&task_status_tx);
                        }
                    }
                    nb_running_tasks.dec();
                }
                is_stopped.store(true, Release);
            })
            .unwrap();

        self.threads_handle.lock().unwrap().push((thread_name.to_string(), th));
        Ok(task_status_forwarder_rx)
    }
}

fn handle_task_status_update(
    msg: Message,
    task_status_forwarder_tx: &Sender<Message>,
    status_by_task_id_w: &Arc<Mutex<WriteHandle<Id, Status>>>,
) -> Result<(), ()> {
    match &msg {
        Ok(it) => match it.status.status {
            State::Error | State::Deployed | State::Deleted | State::DeploymentError | State::DeleteError => {
                status_by_task_id_w
                    .lock()
                    .expect("Could not lock status updater")
                    .empty(it.task.id().to_string())
                    .refresh();
            }

            _ => {
                status_by_task_id_w
                    .lock()
                    .expect("Could not lock status updater")
                    .empty(it.task.id().to_string())
                    .insert(it.task.id().to_string(), it.status.clone())
                    .refresh();
            }
        },

        Err(err) => {
            // FIXME When the task return an error, does it means no further processing will be done ?
            // If yes the HashMap is going to leak memory as containing garbage data that will never be cleaned
            error!("Task error received: {}", err);
        }
    };

    task_status_forwarder_tx
        .send(msg)
        .map_err(|err| error!("Cannot send task status update: {}", err))
}

fn add_task(
    task_processor_tx: &Sender<InternalTask>,
    task_status_tx: &Sender<Message>,
    remaining_tasks: usize,
    task: Box<dyn Task>,
) {
    let message = match remaining_tasks {
        0 => Some("Task is going to be executed !".to_string()),
        _ => {
            info!("Task is queued. {} remaining tasks.", remaining_tasks);
            Some(format!(
                "Task is queued ({} tasks left) and will start when a worker is available.",
                remaining_tasks
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

    let internal_task = InternalTask { task, status };

    // send status contained inside the internal task
    internal_task.send_status(task_status_tx);

    // add internal task to queue
    let _ = task_processor_tx
        .send(internal_task)
        .map_err(|err| error!("cannot enqueue task {}", err));
}

pub trait Task: Send {
    fn created_at(&self) -> &DateTime<Utc>;
    fn group_id(&self) -> &str;
    fn id(&self) -> &str;
    fn bytes_payload(&self) -> &Vec<u8>;
    fn send_status(&self, sender: &Sender<Message>, status: Status);
    /// return true if you want to run it now, or false if you want to run this task later.
    /// this function is called just before `run()` is called.
    fn pre_run(&self) -> PreRun;
    fn run(&self, sender: &Sender<Message>);
}

pub struct InternalTask {
    pub task: Box<dyn Task>,
    pub status: Status,
}

impl InternalTask {
    pub fn send_status(&self, sender: &Sender<Message>) {
        self.task.send_status(sender, self.status.clone());
    }
}

pub enum PreRun {
    Yes,
    NoAndQueueTail,
    NoAndRemove,
}

impl PreRun {
    pub fn add(left: PreRun, right: PreRun) -> Self {
        match left {
            PreRun::Yes => right,
            PreRun::NoAndQueueTail => match right {
                PreRun::NoAndQueueTail | PreRun::Yes => PreRun::NoAndQueueTail,
                PreRun::NoAndRemove => PreRun::NoAndRemove,
            },
            PreRun::NoAndRemove => PreRun::NoAndRemove,
        }
    }
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

    pub fn execution_id_short(&self) -> String {
        let max_execution_id_chars: usize = 7;
        match self.execution_id.char_indices().nth(max_execution_id_chars) {
            None => self.execution_id.to_string(),
            Some((idx, _)) => self.execution_id[..idx].to_string(),
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
}

impl State {
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self,
            State::DeploymentInProgress | State::PauseInProgress | State::DeleteInProgress
        )
    }
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
    use crate::task_manager::{ActionContext, InternalTask, Message, PreRun, State, Status, Task, TaskManager};
    use chrono::{DateTime, NaiveDateTime, Utc};
    use crossbeam_channel::Sender;
    use qovery_engine::models::{ProgressLevel, ProgressScope};
    use std::cmp;
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
    }

    impl WaitingTask {
        fn new() -> WaitingTask {
            WaitingTask {
                date: DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
                bytes: vec![],
                have_been_run: Arc::new(AtomicBool::new(false)),
                barrier_begin: Arc::new(Barrier::new(2)),
                barrier_end: Arc::new(Barrier::new(2)),
            }
        }
    }
    impl Task for WaitingTask {
        fn created_at(&self) -> &DateTime<Utc> {
            &self.date
        }

        fn group_id(&self) -> &str {
            "0"
        }

        fn id(&self) -> &str {
            "0"
        }

        fn bytes_payload(&self) -> &Vec<u8> {
            &self.bytes
        }

        fn send_status(&self, task_status_tx: &Sender<Message>, status: Status) {
            let it = InternalTask {
                task: Box::new(self.clone()),
                status,
            };
            let _ = task_status_tx.send(Ok(it));
        }

        fn pre_run(&self) -> PreRun {
            PreRun::Yes
        }

        fn run(&self, _: &Sender<Message>) {
            self.barrier_begin.wait();
            self.have_been_run.compare_and_swap(false, true, Ordering::Release);
            self.barrier_end.wait();
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
        fn group_id(&self) -> &str {
            "1"
        }
        fn id(&self) -> &str {
            "1"
        }
        fn bytes_payload(&self) -> &Vec<u8> {
            unimplemented!()
        }
        fn send_status(&self, _sender: &Sender<Message>, _status: Status) {}
        fn pre_run(&self) -> PreRun {
            PreRun::Yes
        }
        fn run(&self, _sender: &Sender<Message>) {}
    }

    #[test]
    fn test_taskmanager_run() {
        let mut tm = TaskManager::new();
        tm.running_tasks = prometheus::IntGauge::new("abc", "degf").unwrap();
        let task = WaitingTask::new();

        assert_eq!(tm.running_tasks.get(), 0);
        assert_eq!(task.have_been_run.load(Ordering::Acquire), false);
        tm.add_task(Box::new(task.clone()));

        let task_status_rx = tm.run().expect("Impossible to run task Manager");
        assert_eq!(task_status_rx.recv().unwrap().is_ok(), true);

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
        let mut tm = TaskManager::new();
        tm.running_tasks = prometheus::IntGauge::new("abcd", "degf").unwrap();
        let task = WaitingTask::new();
        let id = task.id().to_string();
        let task_status_rx = tm.run().expect("Impossible to run task Manager");
        tm.add_task(Box::new(task.clone()));

        assert_eq!(task_status_rx.recv().unwrap().unwrap().status.status, State::Waiting);
        assert_eq!(tm.get_task_status(&id).map(|s| s.status), Some(State::Waiting));

        task.barrier_begin.wait();
        task.barrier_end.wait();

        task.send_status(
            &tm.task_status_tx,
            Status {
                status: State::Deleted,
                message: None,
                context: ActionContext {
                    scope: ProgressScope::Queued,
                    level: ProgressLevel::Debug,
                    execution_id: "".to_string(),
                    task_created_at: DateTime::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
                },
            },
        );

        assert_eq!(task_status_rx.recv().unwrap().unwrap().status.status, State::Deleted);
        assert_eq!(tm.get_task_status(&id).map(|s| s.status), None);

        tm.stop();
        assert_eq!(tm.wait_shutdown(), ());
    }

    #[test]
    fn test_taskmanager_graceful_shutdown() {
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
        tm.run().expect("Impossible to run task Manager");

        assert_eq!(tm.wait_shutdown(), ());
        assert_eq!(tm.remaining_tasks_to_run(), 0);
    }

    #[test]
    fn test_auction_context_execution_id_short() {
        // setup:
        struct TestCase<'a> {
            execution_id: &'a str,
            expected_execution_id_short: &'a str,
            description: &'a str,
        }

        let test_cases: Vec<TestCase> = vec![
            TestCase {
                execution_id: "",
                expected_execution_id_short: "",
                description: "empty execution_id",
            },
            TestCase {
                execution_id: " ",
                expected_execution_id_short: " ",
                description: "whitespace execution_id",
            },
            TestCase {
                execution_id: "azertyuiopmlkjhgfdsqwxcvbn",
                expected_execution_id_short: "azertyu",
                description: "execution_id with more chars count than short version",
            },
            TestCase {
                execution_id: "azertyu",
                expected_execution_id_short: "azertyu",
                description: "execution_id with same chars count than short version",
            },
            TestCase {
                execution_id: "azerty",
                expected_execution_id_short: "azerty",
                description: "execution_id with less chars count than short version",
            },
        ];

        for tc in test_cases {
            // execute:
            let auction_context = ActionContext::new(
                ProgressScope::Infrastructure {
                    execution_id: tc.execution_id.to_string(),
                },
                ProgressLevel::Info,
                tc.execution_id.to_string(),
                Utc::now(),
            );
            let result = auction_context.execution_id_short();

            // verify:
            assert_eq!(
                cmp::min(7usize, tc.execution_id.len()),
                result.len(),
                "case: {}, execution_id: {:?}",
                tc.description,
                tc.execution_id,
            );
            assert_eq!(
                tc.expected_execution_id_short, result,
                "case: {}, execution_id: {:?}",
                tc.description, tc.execution_id,
            );
        }
    }
}
