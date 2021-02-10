pub mod subjects;
pub use nats::{Message, Subscription}; // re-export nats internal

use std::io;
use std::time::Duration;
use subjects::Subject;

const QUEUE_GROUP: &str = "engine_queue";

#[derive(Clone, Debug)]
pub struct Connection {
    cnx: nats::Connection,
}

impl Connection {
    pub fn new(name: &str, nats_server: &str) -> io::Result<Connection> {
        let cnx = nats::Options::new().with_name(name).connect(nats_server)?;
        Ok(Connection { cnx })
    }

    pub fn queue_subscribe(&self, subject: &Subject) -> io::Result<Subscription> {
        self.cnx.queue_subscribe(subject.name.as_str(), QUEUE_GROUP)
    }

    pub fn publish(&self, subject: &Subject, payload: &[u8]) -> io::Result<()> {
        self.cnx.publish(subject.name.as_str(), payload)
    }

    pub fn request_timeout(&self, subject: &Subject, payload: &[u8], timeout: Duration) -> io::Result<Message> {
        self.cnx.request_timeout(subject.name.as_str(), payload, timeout)
    }
}
