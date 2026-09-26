//! When the automatic check runs and when the restart fires.

use super::*;

#[test]
fn automatic_update_check_interval_uses_the_configured_whole_hours() {
    assert_eq!(check_for_updates_interval(1), Duration::from_secs(60 * 60));
    assert_eq!(
        check_for_updates_interval(bongocat_config::DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS),
        Duration::from_secs(24 * 60 * 60)
    );
    assert_eq!(
        check_for_updates_interval(48),
        Duration::from_secs(48 * 60 * 60)
    );
}

#[test]
fn automatic_update_schedule_rearms_from_the_last_dispatch_and_caps_polling() {
    let origin = Instant::now();
    let enabled = AutomaticUpdateSettings {
        enabled: true,
        interval_hours: 1,
    };
    assert_eq!(
        automatic_update_schedule_delay(enabled, None, origin),
        Duration::ZERO
    );
    assert_eq!(
        automatic_update_schedule_delay(enabled, Some(origin), origin + Duration::from_secs(30)),
        AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL
    );
    assert_eq!(
        automatic_update_schedule_delay(enabled, Some(origin), origin + Duration::from_secs(3600)),
        Duration::ZERO
    );
    assert_eq!(
        automatic_update_schedule_delay(
            AutomaticUpdateSettings {
                enabled: true,
                interval_hours: bongocat_config::MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS,
            },
            Some(origin),
            origin + Duration::from_secs(60),
        ),
        AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL
    );
    assert_eq!(
        automatic_update_schedule_delay(
            AutomaticUpdateSettings {
                enabled: false,
                interval_hours: 24,
            },
            Some(origin),
            origin,
        ),
        AUTOMATIC_UPDATE_SETTINGS_POLL_INTERVAL
    );
}

/// The restart waits long enough to be seen, then happens whether or not the
/// window is still open.
#[cfg(target_os = "macos")]
#[test]
fn the_post_install_restart_waits_for_the_delay_then_fires() {
    let observed_at = Instant::now();
    assert!(!restart_delay_elapsed(observed_at, observed_at));
    assert!(!restart_delay_elapsed(
        observed_at,
        observed_at + UPDATE_RESTART_DELAY - Duration::from_millis(1)
    ));
    assert!(restart_delay_elapsed(
        observed_at,
        observed_at + UPDATE_RESTART_DELAY
    ));
    // A monotonic clock that appears to move backwards must not restart early.
    assert!(!restart_delay_elapsed(
        observed_at + UPDATE_RESTART_DELAY,
        observed_at
    ));
}
