use crate::config::Config;
use crate::transaction::Transaction;

pub struct Session<'a> {
    pub config: &'a Config,
}

impl<'a> Session<'a> {
    pub fn transaction(self) -> Transaction<'a> {
        Transaction::new(self.config)
    }
}
