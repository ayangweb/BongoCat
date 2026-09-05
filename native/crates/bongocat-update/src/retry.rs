use std::time::Duration;

pub const UPDATE_DOWNLOAD_MAX_ATTEMPTS: u8 = 3;
const INITIAL_RETRY_DELAY_MILLIS: u64 = 1_000;
const MAX_RETRY_DELAY_MILLIS: u64 = 30_000;

/// The path-free result of a single artifact download attempt.
///
/// The future HTTP adapter maps its transport error and response status into
/// this type without exposing client-library types to its caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateDownloadAttemptFailure {
    Cancelled,
    HttpStatus(u16),
    Integrity,
    Staging,
    Transport,
}

/// Fixed retry policy for the update download coordinator.
///
/// An attempt is a complete artifact stream written into a new staging file.
/// This policy deliberately has no range/resume state: retrying always begins
/// with a fresh HTTPS response and a fresh full-artifact verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateDownloadRetryPolicy {
    max_attempts: u8,
    initial_delay_millis: u64,
    max_delay_millis: u64,
}

impl Default for UpdateDownloadRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: UPDATE_DOWNLOAD_MAX_ATTEMPTS,
            initial_delay_millis: INITIAL_RETRY_DELAY_MILLIS,
            max_delay_millis: MAX_RETRY_DELAY_MILLIS,
        }
    }
}

impl UpdateDownloadRetryPolicy {
    pub const fn max_attempts(self) -> u8 {
        self.max_attempts
    }

    /// Return the delay before another attempt after `failed_attempts`
    /// completed failures. `None` means the coordinator must stop.
    pub fn retry_delay(
        self,
        failed_attempts: u8,
        failure: UpdateDownloadAttemptFailure,
    ) -> Option<Duration> {
        if failed_attempts == 0 || failed_attempts >= self.max_attempts || !is_retryable(failure) {
            return None;
        }

        let mut delay_millis = self.initial_delay_millis;
        for _ in 1..failed_attempts {
            delay_millis = delay_millis.saturating_mul(2).min(self.max_delay_millis);
        }
        Some(Duration::from_millis(delay_millis))
    }
}

const fn is_retryable(failure: UpdateDownloadAttemptFailure) -> bool {
    match failure {
        UpdateDownloadAttemptFailure::Transport => true,
        UpdateDownloadAttemptFailure::HttpStatus(status) => {
            status == 408 || status == 429 || (status >= 500 && status <= 599)
        }
        UpdateDownloadAttemptFailure::Cancelled
        | UpdateDownloadAttemptFailure::Integrity
        | UpdateDownloadAttemptFailure::Staging => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_failures_retry_with_bounded_exponential_delays() {
        let policy = UpdateDownloadRetryPolicy::default();
        assert_eq!(policy.max_attempts(), 3);
        assert_eq!(
            policy.retry_delay(1, UpdateDownloadAttemptFailure::Transport),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            policy.retry_delay(2, UpdateDownloadAttemptFailure::HttpStatus(503)),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            policy.retry_delay(3, UpdateDownloadAttemptFailure::Transport),
            None
        );
        assert_eq!(
            policy.retry_delay(u8::MAX, UpdateDownloadAttemptFailure::HttpStatus(429)),
            None
        );
    }

    #[test]
    fn only_transient_http_responses_retry() {
        let policy = UpdateDownloadRetryPolicy::default();
        for status in [408, 429, 500, 503, 599] {
            assert_eq!(
                policy.retry_delay(1, UpdateDownloadAttemptFailure::HttpStatus(status)),
                Some(Duration::from_secs(1)),
                "status {status} should retry"
            );
        }
        for status in [200, 400, 401, 403, 404, 499] {
            assert_eq!(
                policy.retry_delay(1, UpdateDownloadAttemptFailure::HttpStatus(status)),
                None,
                "status {status} should not retry"
            );
        }
    }

    #[test]
    fn cancellation_integrity_and_staging_failures_never_retry() {
        let policy = UpdateDownloadRetryPolicy::default();
        for failure in [
            UpdateDownloadAttemptFailure::Cancelled,
            UpdateDownloadAttemptFailure::Integrity,
            UpdateDownloadAttemptFailure::Staging,
        ] {
            assert_eq!(policy.retry_delay(1, failure), None);
        }
        assert_eq!(
            policy.retry_delay(0, UpdateDownloadAttemptFailure::Transport),
            None
        );
    }
}
