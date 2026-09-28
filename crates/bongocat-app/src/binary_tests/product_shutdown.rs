//! The quit protocol and the startup appearance.

use super::*;

#[test]
fn persisted_theme_resolves_for_process_startup_without_a_settings_window() {
    assert_eq!(
        native_theme_for_startup(bongocat_config::Theme::System),
        None
    );
    assert_eq!(
        native_theme_for_startup(bongocat_config::Theme::Light),
        Some(bongocat_platform::AppTheme::Light)
    );
    assert_eq!(
        native_theme_for_startup(bongocat_config::Theme::Dark),
        Some(bongocat_platform::AppTheme::Dark)
    );
}

#[test]
fn frame_source_shutdown_acknowledges_only_after_the_run_guard_drops() {
    let shutdown = FrameSourceShutdown::default();
    let guard = shutdown.run_guard();

    shutdown.request_stop();
    assert!(shutdown.stop_requested());
    assert!(!shutdown.is_stopped());

    drop(guard);
    assert!(shutdown.is_stopped());
}
