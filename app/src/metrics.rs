use once_cell::sync::Lazy;
use prometheus::{self, IntGauge};

pub static METRICS_NB_RUNNING_TASKS: Lazy<IntGauge> =
    Lazy::new(|| register_int_gauge!("taskmanager_nb_running_tasks", "Number of tasks currently running").unwrap());
