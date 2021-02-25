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
    pub fn new(name: &str, nats_server: &str, credentials: Option<(String, String)>) -> io::Result<Connection> {
        let cnx = match credentials {
            None => nats::Options::new().with_name(name).connect(nats_server)?,
            Some((login, password)) => nats::Options::with_user_pass(login.as_str(), password.as_str())
                .with_name(name)
                .tls_required(true)
                .connect(nats_server)?,
        };

        Ok(Connection { cnx })
    }

    pub fn queue_subscribe(&self, subject: &Subject) -> io::Result<Subscription> {
        info!("Subscribing to queue {:?}", subject);
        self.cnx.queue_subscribe(subject.name.as_str(), QUEUE_GROUP)
    }

    pub fn publish(&self, subject: &Subject, payload: &[u8]) -> io::Result<()> {
        self.cnx.publish(subject.name.as_str(), payload)
    }

    pub fn request_timeout(&self, subject: &Subject, payload: &[u8], timeout: Duration) -> io::Result<Message> {
        self.cnx.request_timeout(subject.name.as_str(), payload, timeout)
    }

    pub fn drain(&self) -> io::Result<()> {
        self.cnx.drain()
    }
}
