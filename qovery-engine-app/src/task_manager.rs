use spmc::RecvError;
use std::collections::HashMap;
use std::iter::Map;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, SendError, Sender};
use std::thread;
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use uuid::Uuid;

type Message = Result<InternalTask, Error>;

pub struct TaskManager {
    sender: spmc::Sender<InternalTask>,
    receiver: spmc::Receiver<InternalTask>,
    receiver_by_task_id: HashMap<Uuid, Receiver<Message>>,
    running: bool,
}

impl TaskManager {
    fn new() -> Self {
        let (sender, receiver) = spmc::channel::<InternalTask>();
        TaskManager {
            sender,
            receiver,
            receiver_by_task_id: HashMap::new(),
            running: false,
        }
    }

    fn add_task(&mut self, task: Box<dyn Task>) {
        let _ = self.sender.send(InternalTask {
            task,
            status: Status::Waiting,
        });
    }

    fn run(&mut self) -> Result<Receiver<Message>, Error> {
        if self.running {
            return Err(Error::AlreadyRunning);
        }

        // only one run allowed
        self.running = true;

        let (tx, rx) = mpsc::channel::<Message>();
        let self_receiver = self.receiver.clone();

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

impl InternalTask {
    fn set_status(&mut self, status: Status) {
        self.status = status;
    }
}

#[derive(Debug, Clone)]
pub enum Status {
    Waiting,
    Running,
    Failed,
    Done,
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
    use crate::task_manager::{SimpleTask, TaskManager};
    use std::thread;
    use std::thread::sleep;
    use std::time::Duration;

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

        loop {
            tm.add_task(Box::new(SimpleTask::new()));
            sleep(Duration::from_secs(1));
        }
    }
}
