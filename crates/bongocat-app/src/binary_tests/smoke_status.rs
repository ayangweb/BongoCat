//! The opt-in smoke flags and the single-instance marker.

use super::*;

#[test]
fn settings_window_smoke_is_opt_in() {
    let options = RunOptions::parse([
        "--settings-window-smoke".to_owned(),
        "--run-seconds".to_owned(),
        "4".to_owned(),
    ])
    .expect("settings window smoke options");
    assert!(options.settings_window_smoke);
    assert!(!options.models_page_smoke);
    assert!(!options.hidden_model_switch_smoke);
    assert_eq!(options.run_duration(), Duration::from_secs(4));
    assert!(options.opens_settings_window_on_start());
}

#[test]
fn settings_window_open_smoke_only_opens_the_window() {
    let options = RunOptions::parse(["--settings-window-open-smoke".to_owned()])
        .expect("settings window open smoke options");
    assert!(options.settings_window_open_smoke);
    assert!(!options.settings_window_smoke);
    assert!(!options.models_page_smoke);
    assert!(!options.hidden_model_switch_smoke);
    assert!(options.opens_settings_window_on_start());
}

#[test]
fn models_page_smoke_is_opt_in() {
    let options = RunOptions::parse(["--models-page-smoke".to_owned()])
        .expect("model library page smoke options");
    assert!(options.models_page_smoke);
    assert!(options.settings_window_smoke);
    assert!(!options.hidden_model_switch_smoke);
    assert!(!options.system_menu_smoke);
    #[cfg(target_os = "macos")]
    assert!(!options.application_reopen_smoke);
    #[cfg(target_os = "macos")]
    assert!(!options.startup_item_smoke);
    #[cfg(target_os = "windows")]
    assert!(!options.single_instance_smoke);
}

#[test]
fn hidden_model_switch_smoke_is_opt_in() {
    let options = RunOptions::parse(["--hidden-model-switch-smoke".to_owned()])
        .expect("hidden model switch smoke options");
    assert!(options.hidden_model_switch_smoke);
    assert!(!options.settings_window_smoke);
    assert!(!options.models_page_smoke);
}

#[cfg(feature = "storage-test-injection")]
#[test]
fn settings_window_state_smoke_is_opt_in() {
    let options = RunOptions::parse(["--settings-window-state-smoke".to_owned()])
        .expect("settings window state smoke options");
    assert!(options.settings_window_state_smoke);
    assert!(!options.settings_window_smoke);
}

#[cfg(feature = "storage-test-injection")]
#[test]
fn panic_diagnostics_smoke_and_private_child_are_opt_in() {
    let options = RunOptions::parse(["--panic-diagnostics-smoke".to_owned()])
        .expect("panic diagnostics smoke options");
    assert!(options.panic_diagnostics_smoke);
    assert!(!options.panic_diagnostics_smoke_child);
    assert!(usage().contains("panic-diagnostics-smoke"));
    // The child is spawned by its parent, so it stays out of the help text.
    assert!(!usage().contains("panic-diagnostics-smoke-child"));

    let child = RunOptions::parse(["--panic-diagnostics-smoke-child".to_owned()])
        .expect("panic diagnostics child options");
    assert!(!child.panic_diagnostics_smoke);
    assert!(child.panic_diagnostics_smoke_child);
}

#[cfg(feature = "storage-test-injection")]
#[test]
fn diagnostics_export_smoke_is_opt_in() {
    let options = RunOptions::parse(["--diagnostics-export-smoke".to_owned()])
        .expect("diagnostics export smoke options");
    assert!(options.diagnostics_export_smoke);
    assert!(!options.settings_window_smoke);
    assert!(!options.panic_diagnostics_smoke);
    assert!(usage().contains("diagnostics-export-smoke"));
}

#[cfg(feature = "storage-test-injection")]
#[test]
fn diagnostics_export_failure_smoke_is_opt_in() {
    let options = RunOptions::parse(["--diagnostics-export-failure-smoke".to_owned()])
        .expect("diagnostics export failure smoke options");
    assert!(options.diagnostics_export_failure_smoke);
    assert!(!options.diagnostics_export_smoke);
    assert!(usage().contains("diagnostics-export-failure-smoke"));
}

#[test]
fn startup_permission_smoke_is_opt_in_and_non_interactive() {
    let options = RunOptions::parse(["--startup-permission-smoke".to_owned()])
        .expect("startup permission smoke options");
    assert!(options.startup_permission_smoke);
    assert!(!options.settings_window_smoke);
    assert!(!options.opens_settings_window_on_start());
    assert!(options.automated_verification());
    assert!(usage().contains("startup-permission-smoke"));
}

#[test]
fn system_menu_smoke_is_opt_in() {
    let options =
        RunOptions::parse(["--system-menu-smoke".to_owned()]).expect("system menu smoke options");
    assert!(options.system_menu_smoke);
    assert!(!options.settings_window_smoke);
}

#[cfg(target_os = "macos")]
#[test]
fn application_reopen_smoke_is_opt_in() {
    let options = RunOptions::parse(["--application-reopen-smoke".to_owned()])
        .expect("application-reopen smoke options");
    assert!(options.application_reopen_smoke);
    assert!(!options.settings_window_smoke);
    assert!(options.opens_settings_window_on_start());
}

#[cfg(target_os = "macos")]
#[test]
fn startup_item_smoke_is_opt_in() {
    let options =
        RunOptions::parse(["--startup-item-smoke".to_owned()]).expect("startup-item smoke options");
    assert!(options.startup_item_smoke);
    assert!(!options.settings_window_smoke);
    assert!(!options.application_reopen_smoke);
}

#[cfg(target_os = "windows")]
#[test]
fn single_instance_smoke_is_opt_in() {
    let options = RunOptions::parse(["--single-instance-smoke".to_owned()])
        .expect("single-instance smoke options");
    assert!(options.single_instance_smoke);
    assert!(!options.settings_window_smoke);
    assert!(options.opens_settings_window_on_start());
    assert!(options.automated_verification());
    assert_eq!(options.single_instance_ready_file, None);
    assert_eq!(options.single_instance_result_file, None);
}

/// Naming a marker file is a harness run even though no smoke flag is set.
///
/// `clap` requires the companion flag, so the marker arrives alongside it; what
/// matters here is that the marker itself is part of the harness set rather than a
/// separate case.
#[cfg(target_os = "windows")]
#[test]
fn a_single_instance_marker_file_also_marks_a_harness_run() {
    let options = RunOptions::parse([
        "--single-instance-smoke".to_owned(),
        "--single-instance-ready-file".to_owned(),
        r"C:\runner\primary.ready".to_owned(),
    ])
    .expect("single-instance marker options");
    assert!(options.automated_verification());
}

#[cfg(target_os = "windows")]
#[test]
fn single_instance_marker_options_are_parsed_with_the_smoke_flag() {
    let options = RunOptions::parse([
        "--single-instance-smoke".to_owned(),
        "--single-instance-ready-file".to_owned(),
        r"C:\runner\primary.ready".to_owned(),
        "--single-instance-result-file".to_owned(),
        r"C:\runner\primary.result".to_owned(),
    ])
    .expect("single-instance marker options");
    assert_eq!(
        options.single_instance_ready_file,
        Some(PathBuf::from(r"C:\runner\primary.ready"))
    );
    assert_eq!(
        options.single_instance_result_file,
        Some(PathBuf::from(r"C:\runner\primary.result"))
    );
}

#[cfg(target_os = "windows")]
#[test]
fn single_instance_marker_options_require_the_smoke_flag() {
    for arguments in [
        vec![
            "--single-instance-ready-file".to_owned(),
            r"C:\runner\primary.ready".to_owned(),
        ],
        vec![
            "--single-instance-result-file".to_owned(),
            r"C:\runner\primary.result".to_owned(),
        ],
    ] {
        assert!(RunOptions::parse(arguments).is_err());
    }
}

#[cfg(target_os = "windows")]
#[test]
fn single_instance_marker_options_require_non_empty_values() {
    for arguments in [
        vec![
            "--single-instance-smoke".to_owned(),
            "--single-instance-ready-file".to_owned(),
        ],
        vec![
            "--single-instance-smoke".to_owned(),
            "--single-instance-result-file".to_owned(),
        ],
        vec![
            "--single-instance-smoke".to_owned(),
            "--single-instance-ready-file".to_owned(),
            String::new(),
        ],
        vec![
            "--single-instance-smoke".to_owned(),
            "--single-instance-result-file".to_owned(),
            String::new(),
        ],
    ] {
        assert!(RunOptions::parse(arguments).is_err());
    }
}
