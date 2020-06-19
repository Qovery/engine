use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::transaction::Transaction;

pub struct Session<'a, K>
where
    K: Kubernetes,
{
    pub config: Config<'a, K>,
}

impl<'a, K> Session<'a, K>
where
    K: Kubernetes,
{
    pub fn transaction(self) -> Transaction<K> {
        Transaction {
            config: self.config,
        }
    }
}
