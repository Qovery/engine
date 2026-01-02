use crate::events::EngineMsg;
use tokio::sync::mpsc::UnboundedSender;

pub trait MsgPublisher: Send + Sync {
    fn send(&self, msg: EngineMsg);
    fn clone_dyn(&self) -> Box<dyn MsgPublisher>;
}

impl MsgPublisher for UnboundedSender<EngineMsg> {
    fn send(&self, msg: EngineMsg) {
        match self.send(msg) {
            Ok(_) => {}
            Err(_) => {
                error!("Unable to send engine msg to msg channel");
            }
        }
    }

    fn clone_dyn(&self) -> Box<dyn MsgPublisher> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct StdMsgPublisher {}

impl StdMsgPublisher {
    pub fn new() -> Self {
        StdMsgPublisher {}
    }
}

impl Default for StdMsgPublisher {
    fn default() -> Self {
        StdMsgPublisher::new()
    }
}

impl MsgPublisher for StdMsgPublisher {
    fn send(&self, _msg: EngineMsg) {
        // TODO should we log message in log?
    }

    fn clone_dyn(&self) -> Box<dyn MsgPublisher> {
        Box::new(self.clone())
    }
}
