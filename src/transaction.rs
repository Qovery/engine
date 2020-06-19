use crate::cloud_provider::Kubernetes;
use crate::config::Config;

pub struct Transaction<'a, K>
where
    K: Kubernetes,
{
    pub config: Config<'a, K>,
}

impl<'a, K> Transaction<'a, K>
where
    K: Kubernetes,
{
    pub fn build(&self, callback: Box<dyn ProgressCallback>) {}
    pub fn push(&self) {}
    pub fn deploy(&self) {}
    pub fn commit(&self) {}
}

pub struct ProgressInfo {
    percent: u8,
    message: String,
}

pub trait ProgressCallback {
    fn on_progress(&self, info: &ProgressInfo);
    fn on_complete(&self, info: &ProgressInfo);
    fn on_error(&self, info: &ProgressInfo);
}
