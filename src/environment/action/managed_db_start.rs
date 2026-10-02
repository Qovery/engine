//! Starts an AWS managed database (RDS instance, DocumentDB cluster) and waits until it is available.
//!
//! The start call can be refused (permissions, capacity, instance still stopping), and a start AWS accepts can
//! still fall back to `stopped` when no capacity is found. Both used to be invisible: the answer was dropped and
//! the deploy waited out its whole timeout.

use crate::environment::action::deploy_database::{
    DB_READY_STATE, DB_STOPPED_STATE, get_managed_database_status, start_stop_managed_database,
};
use crate::infrastructure::models::cloud_provider::service;
use std::thread;
use std::time::{Duration, Instant};

pub(super) trait ManagedDbOps {
    /// Current state as AWS reports it (`available`, `stopped`, `starting`...); `Err` holds the AWS answer.
    fn status(&self) -> Result<String, String>;
    /// Asks AWS to start the database; `Err` holds the AWS answer.
    fn start(&self) -> Result<(), String>;
}

pub(super) struct AwsManagedDb<'a> {
    pub db_type: service::DatabaseType,
    pub id: &'a str,
    pub credentials: &'a [(&'a str, &'a str)],
}

impl ManagedDbOps for AwsManagedDb<'_> {
    fn status(&self) -> Result<String, String> {
        get_managed_database_status(self.db_type, self.id, self.credentials).map_err(aws_answer)
    }

    fn start(&self) -> Result<(), String> {
        start_stop_managed_database(self.db_type, self.id, self.credentials, false).map_err(aws_answer)
    }
}

fn aws_answer((error, output): (crate::cmd::command::CommandError, String)) -> String {
    if output.is_empty() { error.to_string() } else { output }
}

pub(super) trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, duration: Duration);
}

pub(super) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        thread::sleep(duration)
    }
}

pub(super) struct StartPolicy {
    pub timeout: Duration,
    pub poll_interval: Duration,
    /// Give up as soon as AWS cannot tell the state, once the first start was sent (native managed DBs always did).
    pub fail_on_unreadable_state: bool,
}

pub(super) const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Wait before asking again while the database stays stopped: AWS capacity usually comes back within minutes.
const START_RETRY_BACKOFF: [Duration; 4] = [
    Duration::from_secs(60),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(10 * 60),
];

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StartFailure {
    Rejected(String),
    Unreadable(String),
    TimedOut {
        last_state: Option<String>,
        last_aws_error: Option<String>,
    },
    Aborted,
}

impl StartFailure {
    /// AWS no longer knows the instance, e.g. a destroy already removed it.
    pub(super) fn instance_is_gone(&self) -> bool {
        match self {
            StartFailure::Rejected(answer) => {
                answer.contains("DBInstanceNotFound") || answer.contains("DBClusterNotFound")
            }
            StartFailure::Unreadable(_) | StartFailure::TimedOut { .. } | StartFailure::Aborted => false,
        }
    }

    pub(super) fn describe(&self, database_id: &str, timeout: Duration) -> String {
        match self {
            StartFailure::Rejected(answer) => format!("AWS refused to start database `{database_id}`: {answer}"),
            StartFailure::Unreadable(answer) => format!("Cannot read the state of database `{database_id}`: {answer}"),
            StartFailure::TimedOut {
                last_state,
                last_aws_error,
            } => {
                let state = last_state.as_deref().unwrap_or("unknown");
                let minutes = timeout.as_secs() / 60;
                match last_aws_error {
                    Some(answer) => format!(
                        "Database `{database_id}` is still {state} after {minutes} min; last AWS answer to the start: {answer}"
                    ),
                    None => format!("Database `{database_id}` is still {state} after {minutes} min"),
                }
            }
            StartFailure::Aborted => {
                format!("Aborted while waiting for database `{database_id}` to be {DB_READY_STATE}")
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StartErrorKind {
    /// Retrying cannot help (permissions, missing instance, broken network or key): fail now with the AWS answer.
    Fatal,
    /// Known to clear up on its own (capacity, instance still stopping, throttling).
    Retryable,
    /// Not recognised: keep trying as before, the answer is logged and reported.
    Unknown,
}

// Matched on the error code AWS prints in the CLI message, e.g. "An error occurred (AccessDenied) when calling ...".
const FATAL_START_ERRORS: [&str; 15] = [
    "AccessDenied",
    "UnauthorizedOperation",
    "AuthFailure",
    "AuthorizationNotFound",
    "InvalidClientTokenId",
    "ExpiredToken",
    "UnrecognizedClientException",
    "SignatureDoesNotMatch",
    "DBInstanceNotFound",
    "DBClusterNotFound",
    "DBSubnetGroupNotFound",
    "KMSKeyNotAccessible",
    "InvalidVPCNetworkState",
    "InvalidSubnet",
    "StorageQuotaExceeded",
];
const RETRYABLE_START_ERRORS: [&str; 5] = [
    "InsufficientDBInstanceCapacity",
    "InvalidDBInstanceState",
    "InvalidDBClusterStateFault",
    "Throttling",
    "RequestLimitExceeded",
];

pub(super) fn classify_start_error(aws_answer: &str) -> StartErrorKind {
    if FATAL_START_ERRORS.iter().any(|code| aws_answer.contains(code)) {
        return StartErrorKind::Fatal;
    }
    if RETRYABLE_START_ERRORS.iter().any(|code| aws_answer.contains(code)) {
        return StartErrorKind::Retryable;
    }
    StartErrorKind::Unknown
}

/// Starts the database unless it is already available, then polls until it is, asking AWS again with backoff while
/// it stays stopped. Fails at once on a start refusal retrying cannot fix.
pub(super) fn start_until_available(
    ops: &dyn ManagedDbOps,
    clock: &dyn Clock,
    policy: &StartPolicy,
    is_aborted: &dyn Fn() -> bool,
    log: &mut dyn FnMut(String),
) -> Result<(), StartFailure> {
    let began = clock.now();
    let mut last_state: Option<String> = None;
    let mut last_aws_error: Option<String> = None;
    let mut starts_requested = 0;
    let mut last_start_at: Option<Instant> = None;

    loop {
        if is_aborted() {
            return Err(StartFailure::Aborted);
        }

        let (state, unreadable) = match ops.status() {
            // AWS answers "" when it lists nothing for the id: as good as unknown.
            Ok(state) if state.is_empty() => (None, None),
            Ok(state) => (Some(state), None),
            Err(answer) if classify_start_error(&answer) == StartErrorKind::Fatal => {
                return Err(StartFailure::Rejected(answer));
            }
            Err(answer) => (None, Some(answer)),
        };
        if state.as_deref() == Some(DB_READY_STATE) {
            return Ok(());
        }
        if state.is_some() {
            last_state.clone_from(&state);
        }
        if let Some(answer) = &unreadable {
            last_aws_error = Some(answer.clone());
        }
        let timed_out = clock.now().duration_since(began) >= policy.timeout;

        // An unreadable state still gets a first start, as before; afterwards only a stopped database is restarted.
        // A start sent now would not be waited for: give up instead.
        let start_due = !timed_out
            && match last_start_at {
                None => true,
                Some(at) => {
                    state.as_deref() == Some(DB_STOPPED_STATE)
                        && clock.now().duration_since(at) >= backoff_after(starts_requested)
                }
            };
        if start_due {
            starts_requested += 1;
            last_start_at = Some(clock.now());
            log(format!(
                "is {}, requesting a start (attempt {starts_requested})",
                state.as_deref().unwrap_or("in an unknown state")
            ));
            match ops.start() {
                // Accepted: an earlier refusal no longer explains anything.
                Ok(()) => last_aws_error = None,
                Err(answer) if classify_start_error(&answer) == StartErrorKind::Fatal => {
                    return Err(StartFailure::Rejected(answer));
                }
                Err(answer) => {
                    log(format!("start refused by AWS, will retry: {answer}"));
                    last_aws_error = Some(answer);
                }
            }
        }
        if let Some(answer) = unreadable.filter(|_| policy.fail_on_unreadable_state) {
            return Err(StartFailure::Unreadable(answer));
        }

        if timed_out {
            return Err(StartFailure::TimedOut {
                last_state,
                last_aws_error,
            });
        }
        if !start_due {
            log(format!(
                "is {}, waiting for {DB_READY_STATE}",
                state.as_deref().unwrap_or("in an unknown state")
            ));
        }
        clock.sleep(policy.poll_interval);
    }
}

fn backoff_after(starts_requested: usize) -> Duration {
    let step = starts_requested.saturating_sub(1).min(START_RETRY_BACKOFF.len() - 1);
    START_RETRY_BACKOFF[step]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    struct FakeClock {
        now: Cell<Instant>,
    }

    impl FakeClock {
        fn new() -> Self {
            FakeClock {
                now: Cell::new(Instant::now()),
            }
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.now.get()
        }
        fn sleep(&self, duration: Duration) {
            self.now.set(self.now.get() + duration);
        }
    }

    /// Scripted AWS: each `status` call pops the next state (the last one repeats); each `start` pops the next answer.
    struct FakeDb {
        statuses: RefCell<VecDeque<Result<&'static str, &'static str>>>,
        start_answers: RefCell<VecDeque<Result<(), String>>>,
        start_calls: Cell<usize>,
    }

    impl FakeDb {
        fn with_status_errors(
            statuses: Vec<Result<&'static str, &'static str>>,
            start_answers: Vec<Result<(), String>>,
        ) -> Self {
            FakeDb {
                statuses: RefCell::new(statuses.into()),
                start_answers: RefCell::new(start_answers.into()),
                start_calls: Cell::new(0),
            }
        }

        fn new(statuses: &[&'static str], start_answers: Vec<Result<(), String>>) -> Self {
            FakeDb {
                statuses: RefCell::new(statuses.iter().map(|state| Ok(*state)).collect()),
                start_answers: RefCell::new(start_answers.into()),
                start_calls: Cell::new(0),
            }
        }
    }

    impl ManagedDbOps for FakeDb {
        fn status(&self) -> Result<String, String> {
            let mut statuses = self.statuses.borrow_mut();
            let next = if statuses.len() > 1 {
                statuses.pop_front()
            } else {
                statuses.front().copied()
            };
            next.unwrap_or(Ok("stopped"))
                .map(str::to_string)
                .map_err(str::to_string)
        }
        fn start(&self) -> Result<(), String> {
            self.start_calls.set(self.start_calls.get() + 1);
            self.start_answers.borrow_mut().pop_front().unwrap_or(Ok(()))
        }
    }

    const CAPACITY: &str = "An error occurred (InsufficientDBInstanceCapacity) when calling the StartDBInstance operation: \
        Insufficient instance capacity for instance type db.t3.micro in availability zone eu-west-1a";
    const DENIED: &str = "An error occurred (AccessDenied) when calling the StartDBInstance operation: \
        User is not authorized to perform: rds:StartDBInstance";

    fn policy() -> StartPolicy {
        StartPolicy {
            timeout: Duration::from_secs(70 * 60),
            poll_interval: Duration::from_secs(30),
            fail_on_unreadable_state: false,
        }
    }

    fn run(db: &FakeDb, clock: &FakeClock, aborted: bool) -> (Result<(), StartFailure>, Vec<String>) {
        run_with(db, clock, &policy(), aborted)
    }

    fn run_with(
        db: &FakeDb,
        clock: &FakeClock,
        policy: &StartPolicy,
        aborted: bool,
    ) -> (Result<(), StartFailure>, Vec<String>) {
        let mut logs = vec![];
        let result = start_until_available(db, clock, policy, &|| aborted, &mut |line| logs.push(line));
        (result, logs)
    }

    #[test]
    fn does_not_start_an_instance_that_is_already_available() {
        let db = FakeDb::new(&["available"], vec![]);

        let (result, _) = run(&db, &FakeClock::new(), false);

        assert_eq!(result, Ok(()));
        assert_eq!(db.start_calls.get(), 0);
    }

    #[test]
    fn fails_fast_with_the_aws_answer_when_the_start_is_rejected_for_good() {
        let db = FakeDb::new(&["stopped"], vec![Err(DENIED.to_string())]);
        let clock = FakeClock::new();
        let begin = clock.now();

        let (result, _) = run(&db, &clock, false);

        assert_eq!(result, Err(StartFailure::Rejected(DENIED.to_string())));
        assert!(
            clock.now().duration_since(begin) < Duration::from_secs(60),
            "must not wait out the timeout"
        );
    }

    #[test]
    fn retries_the_start_after_a_capacity_rejection() {
        let db = FakeDb::new(
            &["stopped", "stopped", "stopped", "stopped", "starting", "available"],
            vec![Err(CAPACITY.to_string()), Ok(())],
        );

        let (result, logs) = run(&db, &FakeClock::new(), false);

        assert_eq!(result, Ok(()));
        assert_eq!(db.start_calls.get(), 2);
        assert!(
            logs.iter().any(|line| line.contains("InsufficientDBInstanceCapacity")),
            "AWS answer must be logged: {logs:?}"
        );
    }

    #[test]
    fn restarts_an_instance_that_fell_back_to_stopped_after_an_accepted_start() {
        // AWS accepts the start, then gives up (no capacity) and the instance goes back to stopped.
        let db = FakeDb::new(
            &[
                "stopped",
                "starting",
                "stopping",
                "stopped",
                "stopped",
                "stopped",
                "starting",
                "available",
            ],
            vec![Ok(()), Ok(())],
        );

        let (result, _) = run(&db, &FakeClock::new(), false);

        assert_eq!(result, Ok(()));
        assert_eq!(db.start_calls.get(), 2);
    }

    #[test]
    fn times_out_with_the_last_aws_answer_when_the_instance_never_starts() {
        let db = FakeDb::new(&["stopped"], (0..20).map(|_| Err(CAPACITY.to_string())).collect());

        let (result, _) = run(&db, &FakeClock::new(), false);

        assert_eq!(
            result,
            Err(StartFailure::TimedOut {
                last_state: Some("stopped".to_string()),
                last_aws_error: Some(CAPACITY.to_string()),
            })
        );
        assert!(
            db.start_calls.get() > 2,
            "the start must be re-issued while the instance stays stopped"
        );
    }

    #[test]
    fn stops_waiting_when_the_deployment_is_aborted() {
        let db = FakeDb::new(&["stopped"], vec![]);

        let (result, _) = run(&db, &FakeClock::new(), true);

        assert_eq!(result, Err(StartFailure::Aborted));
    }

    const EXPIRED: &str = "An error occurred (ExpiredToken) when calling the DescribeDBInstances operation: The security token included in the request is expired";

    #[test]
    fn reports_the_unreadable_state_when_describe_keeps_failing() {
        let db = FakeDb::with_status_errors(vec![Err("connection reset by peer")], vec![]);

        let (result, _) = run(&db, &FakeClock::new(), false);

        assert_eq!(
            result,
            Err(StartFailure::TimedOut {
                last_state: None,
                last_aws_error: Some("connection reset by peer".to_string()),
            })
        );
    }

    #[test]
    fn fails_on_an_unreadable_state_when_the_policy_asks_for_it() {
        let db = FakeDb::with_status_errors(vec![Err("connection reset by peer")], vec![]);
        let policy = StartPolicy {
            fail_on_unreadable_state: true,
            ..policy()
        };

        let (result, _) = run_with(&db, &FakeClock::new(), &policy, false);

        assert_eq!(result, Err(StartFailure::Unreadable("connection reset by peer".to_string())));
        assert_eq!(
            db.start_calls.get(),
            1,
            "an unreadable state still gets its first start, as before"
        );
    }

    #[test]
    fn fails_fast_on_expired_credentials_while_reading_the_state() {
        let db = FakeDb::with_status_errors(vec![Err(EXPIRED)], vec![]);

        let (result, _) = run(&db, &FakeClock::new(), false);

        assert_eq!(result, Err(StartFailure::Rejected(EXPIRED.to_string())));
    }

    #[test]
    fn forgets_an_earlier_refusal_once_aws_accepts_the_start() {
        let db = FakeDb::new(
            &["stopped", "stopped", "stopped", "starting"],
            vec![Err(CAPACITY.to_string()), Ok(())],
        );

        let (result, _) = run(&db, &FakeClock::new(), false);

        assert_eq!(
            result,
            Err(StartFailure::TimedOut {
                last_state: Some("starting".to_string()),
                last_aws_error: None,
            })
        );
    }

    #[test]
    fn treats_an_empty_state_as_unknown() {
        let db = FakeDb::new(&[""], vec![]);

        let (result, logs) = run(&db, &FakeClock::new(), false);

        assert!(
            matches!(result, Err(StartFailure::TimedOut { last_state: None, .. })),
            "{result:?}"
        );
        assert!(logs.iter().all(|line| !line.starts_with("is ,")), "{logs:?}");
    }

    #[test]
    fn never_requests_a_start_it_will_not_wait_for() {
        let db = FakeDb::new(&["stopped"], vec![]);
        let policy = StartPolicy {
            timeout: Duration::from_secs(60),
            ..policy()
        };

        let (_, _) = run_with(&db, &FakeClock::new(), &policy, false);

        assert_eq!(
            db.start_calls.get(),
            1,
            "the backoff start at 60 s lands on the timeout and must not be sent"
        );
    }

    #[test]
    fn knows_when_the_instance_no_longer_exists() {
        assert!(StartFailure::Rejected("An error occurred (DBInstanceNotFound) ...".to_string()).instance_is_gone());
        assert!(
            StartFailure::Rejected("An error occurred (DBClusterNotFoundFault) ...".to_string()).instance_is_gone()
        );
        assert!(!StartFailure::Rejected(DENIED.to_string()).instance_is_gone());
        assert!(!StartFailure::Aborted.instance_is_gone());
    }

    #[test]
    fn classifies_aws_start_answers() {
        assert_eq!(classify_start_error(DENIED), StartErrorKind::Fatal);
        assert_eq!(
            classify_start_error("An error occurred (DBInstanceNotFound) ..."),
            StartErrorKind::Fatal
        );
        assert_eq!(
            classify_start_error("An error occurred (KMSKeyNotAccessibleFault) ..."),
            StartErrorKind::Fatal
        );
        assert_eq!(classify_start_error(CAPACITY), StartErrorKind::Retryable);
        assert_eq!(
            classify_start_error("An error occurred (InvalidDBInstanceState) ... is not in stopped state"),
            StartErrorKind::Retryable
        );
        assert_eq!(
            classify_start_error("An error occurred (Throttling) ..."),
            StartErrorKind::Retryable
        );
        assert_eq!(classify_start_error(EXPIRED), StartErrorKind::Fatal);
        assert_eq!(
            classify_start_error("An error occurred (InvalidClientTokenId) ..."),
            StartErrorKind::Fatal
        );
        assert_eq!(
            classify_start_error("An error occurred (DBSubnetGroupNotFoundFault) ..."),
            StartErrorKind::Fatal
        );
        assert_eq!(classify_start_error("connection reset by peer"), StartErrorKind::Unknown);
    }
}
