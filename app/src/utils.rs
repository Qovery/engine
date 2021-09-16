extern crate prometheus;

pub type CloudProvider = String;
pub type Region = String;
pub type Organization = String;

#[derive(Clone, Debug)]
pub enum Mode {
    Local,
    Cloud(Organization, CloudProvider, Region),
}

use prometheus::IntGauge;

// TODO: For debugging purpose to catch threads that exit
// Remove when stabilized
lazy_static! {
    static ref METRICS_NB_THREAD_TERMINATED: IntGauge =
        register_int_gauge!("engine_nb_threads_terminated", "Number of threads that have exited").unwrap();
}

pub struct LogErrorOnDrop<'a> {
    msg: &'a str,
}

impl<'a> LogErrorOnDrop<'a> {
    pub fn new(msg: &'a str) -> LogErrorOnDrop<'a> {
        LogErrorOnDrop { msg }
    }
}

impl<'a> Drop for LogErrorOnDrop<'a> {
    fn drop(&mut self) {
        METRICS_NB_THREAD_TERMINATED.inc();
        if std::thread::panicking() {
            eprintln!("THREAD PANIC: {}", self.msg);
        } else {
            eprintln!("THREAD EXIT: {}", self.msg);
        }
    }
}

pub fn log_no_spam_builder(msg: &str, every_n_times: u32) -> Box<dyn FnMut()> {
    let mut loop_counter = 0;
    let msg = msg.to_string();
    Box::new(move || {
        if loop_counter % every_n_times == 0 {
            debug!("{}", msg);
            loop_counter = 0;
        }
        loop_counter += 1;
    })
}
