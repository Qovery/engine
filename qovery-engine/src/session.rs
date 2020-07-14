use crate::config::Config;
use crate::transaction::Transaction;

pub struct Session<'a> {
    pub config: Config<'a>,
}

impl<'a> Session<'a> {
    pub fn transaction(self) -> Transaction<'a> {
        Transaction::new(self.config)
    }
}
