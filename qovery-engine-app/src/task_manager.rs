use crossbeam_channel::{unbounded, Receiver, Sender};
use evmap::{ReadHandle, WriteHandle};
use spmc::RecvError;
use std::collections::HashMap;
use std::iter::Map;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use uuid::Uuid;

type Message = Result<InternalTask, Error>;

pub struct TaskManager {
    sender: Sender<InternalTask>,
    receiver: Receiver<InternalTask>,
    status_by_task_id_r: ReadHandle<Uuid, Status>,
    status_by_task_id_w: Arc<Mutex<WriteHandle<Uuid, Status>>>,
    running: bool,
}

impl TaskManager {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded::<InternalTask>();
        let (status_by_task_id_r, mut status_by_task_id_w) = evmap::new::<Uuid, Status>();
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
            .insert(task.id().clone(), Status::Waiting)
            .refresh();

        let _ = self.sender.send(InternalTask {
            task,
            status: Status::Waiting,
        });
    }

    pub fn get_task_status(&self, id: &Uuid) -> Option<Status> {
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
                        .empty(msg.task.id().clone())
                        .insert(msg.task.id().clone(), msg.status)
                        .refresh();
                }
                Err(err) => {} // FIXME: handle this?
            }
        });

        let _ = thread::spawn(move || loop {
            let internal_task = self_receiver.recv().unwrap();
            internal_task.task.run(tx.clone());
        });

        Ok(rx)
    }
}

pub trait Task: Send {
    fn id(&self) -> &Uuid;
    fn run(self: Box<Self>, sender: Sender<Message>);
}

#[derive(Clone)]
pub struct SimpleTask {
    id: Uuid,
}

impl SimpleTask {
    fn new() -> Self {
        SimpleTask { id: Uuid::new_v4() }
    }

    fn get_internal_task(self: Box<Self>, status: Status) -> InternalTask {
        InternalTask { task: self, status }
    }
}

impl Task for SimpleTask {
    fn id(&self) -> &Uuid {
        &self.id
    }

    fn run(self: Box<Self>, sender: Sender<Message>) {
        let it = self.clone().get_internal_task(Status::Running);
        let _ = sender.send(Ok(it));

        sleep(Duration::from_secs(1));

        let it = self.clone().get_internal_task(Status::Running);
        let _ = sender.send(Ok(it));

        sleep(Duration::from_secs(1));
        let it = self.clone().get_internal_task(Status::Done);
        let _ = sender.send(Ok(it));
    }
}

struct InternalTask {
    task: Box<dyn Task>,
    status: Status,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Status {
    Waiting,
    Running,
    Failed,
    Done,
}

impl evmap::ShallowCopy for Status {
    unsafe fn shallow_copy(&self) -> ManuallyDrop<Self> {
        ManuallyDrop::new(*self)
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
    use crate::task_manager::{SimpleTask, Status, Task, TaskManager};
    use std::str::FromStr;
    use std::thread;
    use std::thread::sleep;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn test() {
        let mut tm = TaskManager::new();
        let rx = tm.run().unwrap();

        thread::spawn(move || loop {
            match rx.recv() {
                Ok(msg) => println!("{}", msg.unwrap().task.id()),
                Err(err) => println!("{:?}", err),
            }
        });

        let st = Box::new(SimpleTask::new());
        let task_id_string = st.id.to_string();

        tm.add_task(st);
        sleep(Duration::from_secs(3));

        let task_id_uuid = Uuid::from_str(task_id_string.as_str()).unwrap();
        let last_status = tm.get_task_status(&task_id_uuid).unwrap();
        assert_eq!(last_status, Status::Done)
    }
}
