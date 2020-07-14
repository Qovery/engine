use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::CloudProvider;
use crate::config::Config;
use crate::transaction::Transaction;

pub struct Session<'a> {
    pub config: Config<'a>,
}

impl<'a> Session<'a> {
    pub fn transaction<C, K>(self) -> Transaction<'a, C, K>
    where
        C: CloudProvider,
        K: Kubernetes<C>,
    {
        Transaction::new(self.config)
    }
}
