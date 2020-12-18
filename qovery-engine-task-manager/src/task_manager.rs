use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread;
use std::thread::sleep;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use evmap::{ReadHandle, WriteHandle};
use serde::{Deserialize, Serialize};

use qovery_engine::models::{ProgressLevel, ProgressScope};

pub type Id = String;
pub type GroupId = Id;
pub type Message = Result<InternalTask, Error>;

pub struct TaskManager {
    it_sender: Sender<InternalTask>,
    // Arc + Mutex used to avoid to get 2 receivers and miss messages (Receiver is not a Bus).
    // Because the receiver is passed to another thread
    it_receiver: Arc<Mutex<Receiver<InternalTask>>>,
    end_task_sig_sender: Sender<bool>,
    end_task_sig_receiver: Receiver<bool>,
    task_terminated_sender: Sender<bool>,
    task_terminated_receiver: Receiver<bool>,
    status_by_task_id_r: ReadHandle<Id, Status>,
    status_by_task_id_w: Arc<Mutex<WriteHandle<Id, Status>>>,
    running: bool,
}

impl TaskManager {
    pub fn new() -> Self {
        let (it_sender, it_receiver) = unbounded::<InternalTask>();
        let (end_task_sig_sender, end_task_sig_receiver) = unbounded::<bool>();
        let (task_terminated_sender, task_terminated_receiver) = unbounded::<bool>();
        let (status_by_task_id_r, status_by_task_id_w) = evmap::new::<Id, Status>();
        let status_by_task_id_w = Arc::new(Mutex::new(status_by_task_id_w));

        TaskManager {
            it_sender,
            it_receiver: Arc::new(Mutex::new(it_receiver)),
            end_task_sig_sender,
            end_task_sig_receiver,
            task_terminated_sender,
            task_terminated_receiver,
            status_by_task_id_r,
            status_by_task_id_w,
            running: false,
        }
    }

    pub fn add_task(&self, task: Box<dyn Task>) {
        let self_it_receiver = match self.it_receiver.lock() {
            Ok(it) => it,
            Err(_) => {
                warn!("error while getting lock to add a task - let's retry in 1s");
                thread::sleep(Duration::from_secs(1));
                return self.add_task(task);
            }
        };

        add_task(
            &self.end_task_sig_receiver,
            &self.status_by_task_id_w,
            &self.it_sender,
            &self_it_receiver,
            task,
        );
    }

    pub fn remaining_tasks_to_run(&self) -> usize {
        //TODO: rewrite me to retry locking
        self.it_receiver.lock().expect("Could not lock internal task receiver when trying to get the number of remaining tasks").len()
    }

    pub fn is_terminated(&self) -> Receiver<bool> {
        self.task_terminated_receiver.clone()
    }

    pub fn get_task_status(&self, id: &Id) -> Option<Status> {
        match self.status_by_task_id_r.get_one(id) {
            Some(status) => Some(status.as_ref().clone()),
            _ => None,
        }
    }

    pub fn get_task_status_by_group_id(&self, group_id: &GroupId) -> Option<Status> {
        // id and group_id should be unique (I know, this is crazy assumption, but let's do this:)),
        // so we use the same data structure to store them all
        self.get_task_status(group_id)
    }

    /// gracefully end the remaining tasks but stop accepting new ones
    pub fn stop(&self) {
        self.end_task_sig_sender.send(true);
    }

    /// run task manager - only a single instance will run
    pub fn run(&mut self) -> Result<Receiver<Message>, Error> {
        if self.running {
            return Err(Error::AlreadyRunning);
        }

        // only one run allowed
        self.running = true;

        let (tx_run_msg, rx_run_msg) = unbounded::<Message>();
        let (tx_run_msg_2, rx_run_msg_2) = unbounded::<Message>();
        let self_it_sender = self.it_sender.clone();
        let self_it_receiver = self.it_receiver.clone();
        let self_end_task_sig_receiver = self.end_task_sig_receiver.clone();
        let self_task_terminated_sender = self.task_terminated_sender.clone();

        let status_by_task_id_w_1 = self.status_by_task_id_w.clone();
        let status_by_task_id_w_2 = self.status_by_task_id_w.clone();

        thread::spawn(move || {
            let tx_run_msg_2 = tx_run_msg_2;

            loop {
                match rx_run_msg.recv() {
                    Ok(msg) => {
                        match msg {
                            Ok(it) => {
                                // update task status
                                // TODO: handle lock failure
                                status_by_task_id_w_1
                                    .lock()
                                    .expect("Could not lock status updater")
                                    .empty(it.task.id().to_string())
                                    .insert(it.task.id().to_string(), it.status.clone())
                                    .refresh();

                                tx_run_msg_2.send(Ok(it));
                            }
                            Err(err) => {
                                tx_run_msg_2.send(Err(err));
                            }
                        }
                    }
                    Err(_) => {} // FIXME: handle this?
                }
            }
        });

        let _ = thread::spawn(move || loop {
            let _ = match self_it_receiver.try_lock() {
                Ok(self_it_receiver) => {
                    let _ = match self_it_receiver.try_recv() {
                        Ok(internal_task) => {
                            // does the task is validated to be run?
                            match internal_task.task.pre_run() {
                                PreRun::Yes => {
                                    // run task
                                    let start_time = Instant::now();
                                    let task_id = String::from(internal_task.task.id());

                                    // send status contained inside the internal task
                                    internal_task.send_status(&tx_run_msg);

                                    // exec task
                                    internal_task.task.run(tx_run_msg.clone());

                                    info!(
                                        "task {} took {:?} to be executed",
                                        task_id,
                                        start_time.elapsed()
                                    );

                                    let remaining_tasks = self_it_receiver.len();

                                    if remaining_tasks == 0
                                        && self_end_task_sig_receiver.try_recv().is_ok()
                                    {
                                        info!("no remaining task to run - shutdown task manager");
                                        self_task_terminated_sender.send(true);
                                    } else if remaining_tasks > 0 {
                                        info!("it remains {} tasks to run", remaining_tasks);
                                    }
                                }
                                _ => {
                                    // postpone the task
                                    info!(
                                        "postpone task group id {} with id {}",
                                        internal_task.task.group_id(),
                                        internal_task.task.id()
                                    );

                                    // send status contained inside the internal task
                                    internal_task.send_status(&tx_run_msg);

                                    // re-add the task
                                    add_task(
                                        &self_end_task_sig_receiver,
                                        &status_by_task_id_w_2,
                                        &self_it_sender,
                                        &self_it_receiver,
                                        internal_task.task,
                                    );

                                    // wait a few seconds
                                    thread::sleep(Duration::from_secs(5))
                                }
                            }
                        }
                        _ => {}
                    };
                }
                Err(_) => {}
            };

            debug!("no task to run, wait for 1 sec");
            sleep(Duration::from_secs(1));
            if self_end_task_sig_receiver.try_recv().is_ok() {
                info!("shutdown task manager");
                self_task_terminated_sender.send(true);
            }
        });

        Ok(rx_run_msg_2)
    }
}

fn add_task(
    end_task_sig_receiver: &Receiver<bool>,
    status_by_task_id_w: &Arc<Mutex<WriteHandle<Id, Status>>>,
    it_sender: &Sender<InternalTask>,
    it_receiver: &Receiver<InternalTask>,
    task: Box<dyn Task>,
) {
    let _ = match end_task_sig_receiver.try_recv() {
        Ok(x) => {
            if x {
                // stop accepting new task
                return;
            } else {
                ()
            }
        }
        Err(_) => (),
    };

    // notify task has been queued
    let task_id = task.id().to_string();
    let remaining_tasks = it_receiver.len();

    let message = match remaining_tasks {
        0 => {
            // the task is going to be executed right away
            info!("add received task and execute it right away, there is no pending task");
            None
        }
        1 => {
            info!(
                "add received task but wait until the task manager is able to execute it. \
            1 remaining task."
            );

            Some(format!(
                "Your task is queued ({} task remained) and will start as soon as a worker is available.",
                remaining_tasks
            ))
        }
        _ => {
            info!(
                "add received task but wait until the task manager is able to execute it. \
            {} remaining tasks.",
                remaining_tasks
            );

            Some(format!(
                "Your task is queued ({} tasks remained) and will start as soon as a worker is available.",
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
            task_id.to_string(),
            task.created_at().clone(),
        ),
    );

    status_by_task_id_w
        .lock()
        //TODO: handle lock failure
        .expect("could not lock task status writer while trying to add task")
        .insert(task_id.to_string(), status.clone())
        .refresh();

    let _ = it_sender.send(InternalTask { task, status });
}

pub trait Task: Send {
    fn created_at(&self) -> &DateTime<Utc>;
    fn group_id(&self) -> &str;
    fn id(&self) -> &str;
    fn send_status(&self, sender: &Sender<Message>, status: Status);
    /// return true if you want to run it now, or false if you want to run this task later.
    /// this function is called just before `run()` is called.
    fn pre_run(&self) -> PreRun;
    fn run(&self, sender: Sender<Message>);
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

impl evmap::ShallowCopy for Status {
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

#[cfg(test)]
mod tests {
    #[test]
    fn test() {}
}
