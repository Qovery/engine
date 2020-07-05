use crate::task_manager::{InternalTask, Message, Status, Task};
use crossbeam_channel::Sender;
use uuid::Uuid;

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
        // TODO
        let it = self.clone().get_internal_task(Status::Running);
        let _ = sender.send(Ok(it));

        let it = self.clone().get_internal_task(Status::Running);
        let _ = sender.send(Ok(it));

        let it = self.clone().get_internal_task(Status::Done);
        let _ = sender.send(Ok(it));
    }
}
