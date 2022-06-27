use std::sync::mpsc::RecvTimeoutError;
use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::Duration;

pub mod application;
pub mod database;
pub mod router;
mod utils;

pub trait DeploymentReporter: Send {
    fn before_deployment_start(&self);
    fn deployment_in_progress(&self);
    fn report_frequency(&self) -> Duration {
        Duration::from_secs(10)
    }
}

pub fn execute_long_deployment<R, F>(deployment_reporter: impl DeploymentReporter + 'static, long_task: F) -> R
where
    F: Fn() -> R,
{
    // stop the thread when the blocking task is done
    let (tx, rx) = mpsc::channel();
    let deployment_start = Arc::new(Barrier::new(2));

    // monitor thread to notify user while the blocking task is executed
    let th_handle = thread::Builder::new().name("deployment-monitor".to_string()).spawn({
        let deployment_start = deployment_start.clone();
        let report_frequency = deployment_reporter.report_frequency();

        move || {
            // Before the launch of the deployment
            deployment_reporter.before_deployment_start();

            // Wait the start of the deployment
            deployment_start.wait();

            // Send deployment progress report every x secs
            while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(report_frequency) {
                deployment_reporter.deployment_in_progress();
            }
        }
    });

    // Wait for our watcher thread to be ready before starting
    let _ = deployment_start.wait();
    let blocking_task_result = long_task();
    let _ = tx.send(()); // send signal to thread to terminate
    let _ = th_handle.map(|th| th.join()); // wait for the thread to terminate

    blocking_task_result
}

#[cfg(test)]
mod test {
    use crate::deployment_report::{execute_long_deployment, DeploymentReporter};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    pub struct DeploymentReporterTest {
        pub before_deployment: Arc<AtomicBool>,
        pub deployment_in_progress: Arc<AtomicBool>,
        pub thread_dead: Arc<AtomicBool>,
        pub is_task_started: Arc<AtomicBool>,
    }

    impl Drop for DeploymentReporterTest {
        fn drop(&mut self) {
            self.thread_dead.store(true, Ordering::SeqCst)
        }
    }

    impl DeploymentReporter for DeploymentReporterTest {
        fn before_deployment_start(&self) {
            assert!(!self.is_task_started.load(Ordering::SeqCst));
            self.before_deployment.store(true, Ordering::SeqCst)
        }

        fn deployment_in_progress(&self) {
            self.deployment_in_progress.store(true, Ordering::SeqCst)
        }

        fn report_frequency(&self) -> Duration {
            Duration::from_secs(1)
        }
    }

    #[test]
    fn test_execute_long_deployment() {
        let reporter = DeploymentReporterTest {
            before_deployment: Arc::new(AtomicBool::new(false)),
            deployment_in_progress: Arc::new(AtomicBool::new(false)),
            thread_dead: Arc::new(AtomicBool::new(false)),
            is_task_started: Arc::new(AtomicBool::new(false)),
        };

        let before_deployment = reporter.before_deployment.clone();
        let deployment_in_progress = reporter.deployment_in_progress.clone();
        let thread_dead = reporter.thread_dead.clone();
        let is_task_started = reporter.is_task_started.clone();

        execute_long_deployment(reporter, || {
            is_task_started.store(true, Ordering::SeqCst);
            thread::sleep(Duration::from_secs(2));
        });

        // Check that our method have been called
        assert!(before_deployment.load(Ordering::SeqCst));
        assert!(deployment_in_progress.load(Ordering::SeqCst));
        assert!(thread_dead.load(Ordering::SeqCst));
    }
}
