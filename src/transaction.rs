use crate::cloud_provider::Kubernetes;
use crate::config::Config;

pub struct Transaction<'a, K>
where
    K: Kubernetes,
{
    pub config: Config<'a, K>,
    steps: Vec<Step>,
    build_listeners: Vec<Box<dyn ProgressListener>>,
    push_listeners: Vec<Box<dyn ProgressListener>>,
    deploy_listeners: Vec<Box<dyn ProgressListener>>,
}

impl<'a, K> Transaction<'a, K>
where
    K: Kubernetes,
{
    pub fn new(config: Config<'a, K>) -> Self {
        Transaction {
            config,
            steps: vec![],
            build_listeners: vec![],
            push_listeners: vec![],
            deploy_listeners: vec![],
        }
    }

    pub fn build(&mut self) {
        self.steps.push(Step::Build);
    }

    pub fn add_build_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.build_listeners.push(listener);
    }

    pub fn push(&mut self) {
        self.steps.push(Step::Push);
    }

    pub fn add_push_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.push_listeners.push(listener);
    }

    pub fn deploy(&mut self) {
        self.steps.push(Step::Deploy);
    }

    pub fn add_deploy_listener(&mut self, listener: Box<dyn ProgressListener>) {
        self.deploy_listeners.push(listener);
    }

    pub fn commit(&self) {
        self.steps.iter().for_each(|step| match step {
            Step::Build => {
                println!("build");
            }
            Step::Push => {
                println!("push");
            }
            Step::Deploy => {
                println!("deploy");
            }
        })
    }
}

enum Step {
    Build,
    Push,
    Deploy,
}

pub struct ProgressInfo {
    percent: u8,
    message: String,
}

pub trait ProgressListener {
    fn on_progress(&self, info: &ProgressInfo);
    fn on_complete(&self, info: &ProgressInfo);
    fn on_error(&self, info: &ProgressInfo);
}
