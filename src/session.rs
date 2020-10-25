use crate::engine::Engine;
use crate::transaction::Transaction;

pub struct Session<'a> {
    pub engine: &'a Engine,
}

impl<'a> Session<'a> {
    pub fn transaction(self) -> Transaction<'a> {
        Transaction::new(self.engine)
    }
}
