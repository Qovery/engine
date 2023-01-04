use prometheus::{self, IntGauge};

lazy_static! {
    pub static ref METRICS_NB_RUNNING_TASKS: IntGauge =
        register_int_gauge!("taskmanager_nb_running_tasks", "Number of tasks currently running").unwrap();
}
