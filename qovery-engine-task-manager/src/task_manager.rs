use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::iter::Map;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::{sleep, JoinHandle};
use std::time::Duration;

use crate::models::Request;
use crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use evmap::{ReadHandle, WriteHandle};

pub type Id = String;
pub type Message = Result<InternalTask, Error>;

pub struct TaskManager {
    sender: Sender<InternalTask>,
    receiver: Receiver<InternalTask>,
    status_by_task_id_r: ReadHandle<Id, Status>,
    status_by_task_id_w: Arc<Mutex<WriteHandle<Id, Status>>>,
    running: bool,
}

impl TaskManager {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded::<InternalTask>();
        let (status_by_task_id_r, status_by_task_id_w) = evmap::new::<Id, Status>();
        let status_by_task_id_w = Arc::new(Mutex::new(status_by_task_id_w));

        TaskManager {
            sender,
            receiver,
            status_by_task_id_r,
            status_by_task_id_w,
            running: false,
        }
    }

    pub fn add_task(&mut self, task: Box<dyn Task>) {
        self.status_by_task_id_w
            .lock()
            .unwrap()
            .insert(task.id().to_string(), Status::Waiting { message: None })
            .refresh();

        let _ = self.sender.send(InternalTask {
            task,
            status: Status::Waiting { message: None },
        });
    }

    pub fn get_task_status(&self, id: &Id) -> Option<Status> {
        match self.status_by_task_id_r.get_one(id) {
            Some(status) => Some(status.as_ref().clone()),
            _ => None,
        }
    }

    pub fn run(&mut self) -> Result<Receiver<Message>, Error> {
        if self.running {
            return Err(Error::AlreadyRunning);
        }

        // only one run allowed
        self.running = true;

        let (tx, rx) = unbounded::<Message>();
        let self_receiver = self.receiver.clone();

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
            let internal_task = self_receiver.recv().unwrap();
            internal_task.task.run(tx.clone());

            if self_receiver.is_empty() {
                info!("no remaining task to run - waiting for a new one...");
            } else {
                info!("it remains {} tasks to run", self_receiver.len());
            }
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
