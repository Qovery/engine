use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::iter::Map;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};

use crate::models::Request;
use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use evmap::{ReadHandle, WriteHandle};

pub type Id = String;
pub type Message = Result<InternalTask, Error>;

pub struct TaskManager {
    it_sender: Sender<InternalTask>,
    it_receiver: Receiver<InternalTask>,
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
            it_receiver,
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
        let _ = match self.end_task_sig_receiver.try_recv() {
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

        self.status_by_task_id_w
            .lock()
            .unwrap()
            .insert(task.id().to_string(), Status::Waiting { message: None })
            .refresh();

        let _ = self.it_sender.send(InternalTask {
            task,
            status: Status::Waiting { message: None },
        });
    }

    pub fn remaining_tasks_to_run(&self) -> usize {
        self.it_receiver.len()
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

    /// gracefully end the remaining tasks but stop accepting new ones
    pub fn stop(&self) {
        self.end_task_sig_sender.send(true);
    }

    pub fn run(&mut self) -> Result<Receiver<Message>, Error> {
        if self.running {
            return Err(Error::AlreadyRunning);
        }

        // only one run allowed
        self.running = true;

        let (tx, rx) = unbounded::<Message>();
        let self_it_receiver = self.it_receiver.clone();
        let self_end_task_receiver = self.end_task_sig_receiver.clone();
        let self_task_terminated_sender = self.task_terminated_sender.clone();

        let receiver = rx.clone();
        let w = self.status_by_task_id_w.clone();

        thread::spawn(move || loop {
            match receiver.recv() {
                Ok(msg) => {
                    let msg = msg.unwrap();
                    // update task status
                    w.lock()
                        .unwrap()
                        .empty(msg.task.id().to_string())
                        .insert(msg.task.id().to_string(), msg.status)
                        .refresh();
                }
                Err(err) => {} // FIXME: handle this?
            }
        });

        let _ = thread::spawn(move || loop {
            let _ = match self_it_receiver.try_recv() {
                Ok(internal_task) => {
                    let start_time = Instant::now();

                    // run task
                    internal_task.task.run(tx.clone());

                    info!(
                        "task {} took {:?} to be executed",
                        internal_task.task.id(),
                        start_time.elapsed()
                    );

                    if self_it_receiver.is_empty() && self_end_task_receiver.try_recv().is_ok() {
                        info!("no remaining task to run - shutdown task manager");
                        self_task_terminated_sender.send(true);
                    } else if self_it_receiver.len() > 0 {
                        info!("it remains {} tasks to run", self_it_receiver.len());
                    }
                }
                Err(err) => {
                    sleep(Duration::from_secs(1));
                    if self_end_task_receiver.try_recv().is_ok() {
                        info!("shutdown task manager");
                        self_task_terminated_sender.send(true);
                    }
                }
            };
        });

        Ok(rx)
    }
}

pub trait Task: Send {
    fn id(&self) -> &str;
    fn update_status(&self, sender: &Sender<Message>, status: Status);
    fn run(&self, sender: Sender<Message>);
}

pub struct InternalTask {
    pub task: Box<dyn Task>,
    pub status: Status,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Status {
    Waiting { message: Option<String> },
    Running { message: Option<String> },
    Warning { message: Option<String> },
    Error { message: Option<String> },
    Failed { message: Option<String> },
    Done { message: Option<String> },
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
