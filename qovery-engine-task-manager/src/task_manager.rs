use std::collections::HashMap;
use std::iter::Map;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::thread::{current, sleep, spawn, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use evmap::{ReadHandle, WriteHandle};
use serde::{Deserialize, Serialize};

use qovery_engine::cloud_provider::service::ServiceError;
use qovery_engine::models::{ProgressLevel, ProgressScope, ProgressStep};

use crate::models::Request;
use qovery_engine::error::ConfigurationError;

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

    pub fn add_task(&mut self, task: Box<dyn Task>) {
        add_task(
            &self.end_task_sig_receiver,
            &self.status_by_task_id_w,
            &self.it_sender,
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
        // id and group_id should be unique, so we use the same data structure to store them all
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
                    Err(err) => {} // FIXME: handle this?
                }
            }
        });

        let _ = thread::spawn(move || loop {
            // TODO: Handle lock failure
            let self_it_receiver = self_it_receiver
                .lock()
                .expect("Could not lock internal task receiver");
            let _ = match self_it_receiver.try_recv() {
                Ok(internal_task) => {
                    // does the task is validated to be run?
                    if internal_task.task.pre_run() {
                        let start_time = Instant::now();
                        // run task
                        let task_id = String::from(internal_task.task.id());
                        let i_task = Arc::new(Mutex::new(internal_task));
                        let thread_task = i_task.clone();
                        let thread_tx_run_msg = tx_run_msg.clone();
                        // prevent exec failure - run task in another thread
                        // TODO: return an appropriate error, compatible with dyn std::error::Error + 'static.
                        //       This should make error reporting easier
                        let join_handle = thread::spawn(move || {
                            thread_task
                                .lock()
                                .expect("Could not lock task for execution!")
                                .task
                                .run(thread_tx_run_msg);
                        });

                        let join_handle_result = join_handle.join();
                        match join_handle_result {
                            Ok(r) => {} /*
                            match r {
                            Ok(ok) => {}
                            Err(err) => {
                            unimplemented!();
                            }
                            }    // task ok or handled error */
                            Err(err) => {
                                error!("A task caused a panic while executing!");
                                match i_task.lock() {
                                    Ok(it) => {
                                        it.task.update_status(
                                            &tx_run_msg,
                                            Status::TerminatedWithError {
                                                message: Some(format!(
                                                    "task caused a panic!: {:?}",
                                                    err
                                                )),
                                                context: ActionContext::new(
                                                    // TODO: create a more appropriate scope?
                                                    ProgressScope::Queued,
                                                    ProgressStep::Final,
                                                    ProgressLevel::Error,
                                                    task_id,
                                                ),
                                            },
                                        );
                                    }
                                    Err(e) => {
                                        // Mutex poisoning is rare, but let's be carefull
                                        error!("==== /!\\ ==== /!\\ ====   Manual action required  ==== /!\\ ==== /!\\ ====");
                                        error!("==== /!\\ Could not lock a task (which previously caused a panic) /!\\ ====");
                                        error!("==== /!\\     so that we could report failure to the core!        /!\\ ====");
                                    }
                                }

                                return;
                            } // task with unhandled error - good luck,
                        };

                        info!(
                            "task {} took {:?} to be executed",
                            task_id,
                            start_time.elapsed()
                        );

                        if self_it_receiver.len() == 0
                            && self_end_task_sig_receiver.try_recv().is_ok()
                        {
                            info!("no remaining task to run - shutdown task manager");
                            self_task_terminated_sender.send(true);
                        } else if self_it_receiver.len() > 0 {
                            info!("it remains {} tasks to run", self_it_receiver.len());
                        }
                    } else {
                        // postpone the task
                        info!(
                            "postpone task group id {} with id {}",
                            internal_task.task.group_id(),
                            internal_task.task.id()
                        );

                        // re-add the task
                        add_task(
                            &self_end_task_sig_receiver,
                            &status_by_task_id_w_2,
                            &self_it_sender,
                            internal_task.task,
                        );

                        // wait a few seconds
                        thread::sleep(Duration::from_secs(5))
                    }
                }
                Err(err) => {
                    debug!("no task to run, wait for 1 sec");
                    sleep(Duration::from_secs(1));
                    if self_end_task_sig_receiver.try_recv().is_ok() {
                        info!("shutdown task manager");
                        self_task_terminated_sender.send(true);
                    }
                }
            };
        });

        Ok(rx_run_msg_2)
    }
}

fn add_task(
    end_task_sig_receiver: &Receiver<bool>,
    status_by_task_id_w: &Arc<Mutex<WriteHandle<Id, Status>>>,
    it_sender: &Sender<InternalTask>,
    task: Box<dyn Task>,
) {
    // TODO notify task has been queued

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

    let task_id = task.id().to_string();
    status_by_task_id_w
        .lock()
        //TODO: handle lock failure
        .expect("could not lock task status writer whin trying to add task")
        .insert(
            task_id.to_string(),
            Status::Waiting {
                message: None,
                context: ActionContext::new(
                    ProgressScope::Queued,
                    ProgressStep::Init,
                    ProgressLevel::Info,
                    task_id.to_string(),
                ),
            },
        )
        .refresh();

    let _ = it_sender.send(InternalTask {
        task,
        status: Status::Waiting {
            message: None,
            context: ActionContext::new(
                ProgressScope::Queued,
                ProgressStep::Init,
                ProgressLevel::Info,
                task_id.to_string(),
            ),
        },
    });
}

pub trait Task: Send {
    fn group_id(&self) -> &str;
    fn id(&self) -> &str;
    fn update_status(&self, sender: &Sender<Message>, status: Status);
    /// return true if you want to run it now, or false if you want to run this task later.
    /// this function is called just before `run()` is called.
    fn pre_run(&self) -> bool;
    fn run(&self, sender: Sender<Message>);
}

pub struct InternalTask {
    pub task: Box<dyn Task>,
    pub status: Status,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ActionContext {
    pub scope: ProgressScope,
    pub step: ProgressStep,
    pub level: ProgressLevel,
    pub execution_id: String,
}

impl ActionContext {
    pub fn new(
        scope: ProgressScope,
        step: ProgressStep,
        level: ProgressLevel,
        execution_id: String,
    ) -> Self {
        ActionContext {
            scope,
            step,
            level,
            execution_id,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Waiting {
        message: Option<String>,
        context: ActionContext,
    },
    Running {
        message: Option<String>,
        context: ActionContext,
    },
    Error {
        message: Option<String>,
        context: ActionContext,
    },
    Terminated {
        message: Option<String>,
        context: ActionContext,
    },
    TerminatedWithError {
        message: Option<String>,
        context: ActionContext,
    },
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
