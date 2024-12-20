use atomic_enum::atomic_enum;
use itertools::Itertools;
use std::collections::HashSet;
use strum_macros::EnumIter;

#[atomic_enum]
#[derive(EnumIter, Eq, Hash, PartialEq)]
pub enum AbortStatus {
    None = 0,
    Requested,
    UserForceRequested,
}

impl AbortStatus {
    pub fn should_cancel(&self) -> bool {
        !matches!(self, AbortStatus::None)
    }

    pub fn should_force_cancel(&self) -> bool {
        matches!(self, AbortStatus::UserForceRequested)
    }

    pub fn merge(status_1: AbortStatus, status_2: AbortStatus) -> AbortStatus {
        let statuses = HashSet::from([status_1, status_2]);

        // returns the new status based on the higher priority

        if statuses.iter().contains(&AbortStatus::UserForceRequested) {
            // <- UserForceRequested is the highest priority
            return AbortStatus::UserForceRequested;
        }

        if statuses.iter().contains(&AbortStatus::Requested) {
            return AbortStatus::Requested;
        }

        AbortStatus::None
    }
}

pub trait Abort: Send + Sync {
    fn status(&self) -> AbortStatus;
}

impl<T: Fn() -> AbortStatus> Abort for T
where
    T: Sync + Send,
{
    fn status(&self) -> AbortStatus {
        self()
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::abort::AbortStatus;
    use itertools::Itertools;
    use strum::IntoEnumIterator;

    #[test]
    fn test_should_cancel() {
        // setup:
        for abort_status in AbortStatus::iter() {
            // execute & verify:
            assert_eq!(
                match abort_status {
                    AbortStatus::None => false,
                    AbortStatus::Requested => true,
                    AbortStatus::UserForceRequested => true,
                },
                abort_status.should_cancel()
            );
        }
    }

    #[test]
    fn test_merge() {
        // setup:
        for (status_1, status_2) in AbortStatus::iter().permutations(2_usize).map(|r| {
            assert_eq!(2, r.len());
            (r[0], r[1])
        }) {
            // execute:
            let result_1 = AbortStatus::merge(status_1, status_2);
            let result_2 = AbortStatus::merge(status_2, status_1);

            // verify:
            let expected_result = match (status_1, status_2) {
                (AbortStatus::UserForceRequested, _) | (_, AbortStatus::UserForceRequested) => {
                    AbortStatus::UserForceRequested
                }
                (AbortStatus::Requested, _) | (_, AbortStatus::Requested) => AbortStatus::Requested,
                _ => AbortStatus::None,
            };
            assert_eq!(expected_result, result_1);
            assert_eq!(expected_result, result_2);
        }
    }
}
