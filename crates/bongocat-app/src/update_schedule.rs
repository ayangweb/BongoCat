//! When the automatic update check runs.
//!
//! The check is opt-in and must never compete with startup, so the first attempt
//! waits, and a changed interval is observed on a short poll rather than by
//! rebuilding the whole settings snapshot, which scans the model catalog.

use super::*;

/// How long the product is given to finish starting before the first automatic check.
///
/// The check is opt-in and must never compete with startup for the network or the
/// window server.
pub(crate) const AUTOMATIC_UPDATE_CHECK_STARTUP_DELAY: Duration = Duration::from_secs(10);

/// Convert the persisted whole-hour setting into the scheduler's duration.
pub(crate) fn check_for_updates_interval(interval_hours: u16) -> Duration {
    const SECONDS_PER_HOUR: u64 = 60 * 60;
    Duration::from_secs(u64::from(interval_hours) * SECONDS_PER_HOUR)
}

/// A short poll lets a changed interval or switch re-arm the schedule without
/// rebuilding the full settings snapshot (which scans the model catalog).
pub(crate) const AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL: Duration = Duration::from_secs(60);

pub(crate) const AUTOMATIC_UPDATE_SETTINGS_RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Return the next scheduler delay, capped so persisted setting changes are
/// observed promptly. A missing `last_dispatch` means the first check is due.
pub(crate) fn automatic_update_schedule_delay(
    settings: AutomaticUpdateSettings,
    last_dispatch: Option<Instant>,
    now: Instant,
) -> Duration {
    if !settings.enabled {
        return AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL;
    }
    let Some(last_dispatch) = last_dispatch else {
        return Duration::ZERO;
    };
    check_for_updates_interval(settings.interval_hours)
        .saturating_sub(now.saturating_duration_since(last_dispatch))
        .min(AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL)
}

/// How long the automatic check waits for its own result to be published.
pub(crate) const AUTOMATIC_UPDATE_CHECK_SETTLE_ATTEMPTS: u32 = 120;

pub(crate) const AUTOMATIC_UPDATE_CHECK_SETTLE_INTERVAL: Duration = Duration::from_millis(500);
