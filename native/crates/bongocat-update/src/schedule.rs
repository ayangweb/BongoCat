use std::{fmt, time::Duration};

pub const AUTOMATIC_UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomaticUpdateCheckReason {
    Startup,
    Interval,
    Reenabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateScheduleErrorCode {
    MonotonicTimeRegressed,
}

impl UpdateScheduleErrorCode {
    pub const ALL: [Self; 1] = [Self::MonotonicTimeRegressed];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MonotonicTimeRegressed => "update_schedule_monotonic_time_regressed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateScheduleError {
    pub code: UpdateScheduleErrorCode,
}

impl fmt::Display for UpdateScheduleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for UpdateScheduleError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NextAutomaticCheck {
    Immediate(AutomaticUpdateCheckReason),
    At(Duration),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutomaticUpdateCheckScheduler {
    enabled: bool,
    last_observed: Option<Duration>,
    next: Option<NextAutomaticCheck>,
}

impl AutomaticUpdateCheckScheduler {
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            last_observed: None,
            next: if enabled {
                Some(NextAutomaticCheck::Immediate(
                    AutomaticUpdateCheckReason::Startup,
                ))
            } else {
                None
            },
        }
    }

    pub const fn is_enabled(self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.next = enabled.then_some(NextAutomaticCheck::Immediate(
            AutomaticUpdateCheckReason::Reenabled,
        ));
    }

    pub fn take_due(
        &mut self,
        now: Duration,
    ) -> Result<Option<AutomaticUpdateCheckReason>, UpdateScheduleError> {
        if self.last_observed.is_some_and(|previous| now < previous) {
            self.last_observed = Some(now);
            self.next = self.enabled.then(|| next_after(now)).flatten();
            return Err(UpdateScheduleError {
                code: UpdateScheduleErrorCode::MonotonicTimeRegressed,
            });
        }
        self.last_observed = Some(now);

        let reason = match self.next {
            Some(NextAutomaticCheck::Immediate(reason)) => Some(reason),
            Some(NextAutomaticCheck::At(deadline)) if now >= deadline => {
                Some(AutomaticUpdateCheckReason::Interval)
            }
            Some(NextAutomaticCheck::At(_)) | None => None,
        };
        if reason.is_some() {
            self.next = next_after(now);
        }
        Ok(reason)
    }
}

const fn next_after(now: Duration) -> Option<NextAutomaticCheck> {
    match now.checked_add(AUTOMATIC_UPDATE_CHECK_INTERVAL) {
        Some(next) => Some(NextAutomaticCheck::At(next)),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable_and_unique() {
        let mut codes = UpdateScheduleErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .all(|code| code.starts_with("update_schedule_"))
        );
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), UpdateScheduleErrorCode::ALL.len());
    }

    #[test]
    fn enabled_schedule_checks_at_startup_then_every_twenty_four_hours() {
        let mut scheduler = AutomaticUpdateCheckScheduler::new(true);
        let start = Duration::from_secs(10);
        assert_eq!(
            scheduler.take_due(start).expect("startup poll"),
            Some(AutomaticUpdateCheckReason::Startup)
        );
        assert_eq!(scheduler.take_due(start).expect("same instant"), None);
        assert_eq!(
            scheduler
                .take_due(start + AUTOMATIC_UPDATE_CHECK_INTERVAL - Duration::from_nanos(1))
                .expect("before interval"),
            None
        );
        let interval = start + AUTOMATIC_UPDATE_CHECK_INTERVAL;
        assert_eq!(
            scheduler.take_due(interval).expect("interval poll"),
            Some(AutomaticUpdateCheckReason::Interval)
        );
        assert_eq!(scheduler.take_due(interval).expect("one shot"), None);
    }

    #[test]
    fn disabling_suppresses_checks_and_reenabling_checks_once_immediately() {
        let mut scheduler = AutomaticUpdateCheckScheduler::new(false);
        assert!(!scheduler.is_enabled());
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(1))
                .expect("disabled poll"),
            None
        );

        scheduler.set_enabled(true);
        scheduler.set_enabled(true);
        assert!(scheduler.is_enabled());
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(2))
                .expect("reenabled poll"),
            Some(AutomaticUpdateCheckReason::Reenabled)
        );
        scheduler.set_enabled(false);
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(2) + AUTOMATIC_UPDATE_CHECK_INTERVAL)
                .expect("disabled interval"),
            None
        );
    }

    #[test]
    fn monotonic_regression_is_diagnostic_and_rebases_without_an_immediate_retry() {
        let mut scheduler = AutomaticUpdateCheckScheduler::new(true);
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(100))
                .expect("startup poll"),
            Some(AutomaticUpdateCheckReason::Startup)
        );
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(99))
                .expect_err("clock regression")
                .code,
            UpdateScheduleErrorCode::MonotonicTimeRegressed
        );
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(99))
                .expect("rebased poll"),
            None
        );
        assert_eq!(
            scheduler
                .take_due(Duration::from_secs(99) + AUTOMATIC_UPDATE_CHECK_INTERVAL)
                .expect("rebased interval"),
            Some(AutomaticUpdateCheckReason::Interval)
        );
    }

    #[test]
    fn deadline_overflow_stops_future_checks_without_retrying() {
        let mut scheduler = AutomaticUpdateCheckScheduler::new(true);
        let near_maximum = Duration::MAX - Duration::from_secs(1);
        assert_eq!(
            scheduler.take_due(near_maximum).expect("startup poll"),
            Some(AutomaticUpdateCheckReason::Startup)
        );
        assert_eq!(
            scheduler.take_due(Duration::MAX).expect("maximum poll"),
            None
        );
        assert_eq!(
            scheduler.take_due(Duration::MAX).expect("repeated poll"),
            None
        );
    }
}
