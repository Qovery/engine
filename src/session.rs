use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::transaction::Transaction;

pub struct Session {
    pub config: Config,
}

impl<'a> Session {
    pub fn transaction(self) -> Transaction {
        Transaction::new(self.config)
    }
}
