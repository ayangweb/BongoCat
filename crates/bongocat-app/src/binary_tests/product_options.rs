//! The command line: run modes, durations and refusals.

use super::*;

#[test]
fn run_options_default_to_an_unbounded_product_lifetime() {
    let options = RunOptions::parse(Vec::new()).expect("default options");
    assert_eq!(
        options,
        RunOptions {
            run_seconds: 0,
            #[cfg(target_os = "windows")]
            single_instance_ready_file: None,
            #[cfg(target_os = "windows")]
            single_instance_result_file: None,
            settings_window_smoke: false,
            settings_window_open_smoke: false,
            models_page_smoke: false,
            hidden_model_switch_smoke: false,
            #[cfg(feature = "storage-test-injection")]
            settings_window_state_smoke: false,
            #[cfg(feature = "storage-test-injection")]
            panic_diagnostics_smoke: false,
            #[cfg(feature = "storage-test-injection")]
            panic_diagnostics_smoke_child: false,
            #[cfg(feature = "storage-test-injection")]
            diagnostics_export_smoke: false,
            #[cfg(feature = "storage-test-injection")]
            diagnostics_export_failure_smoke: false,
            system_menu_smoke: false,
            startup_permission_smoke: false,
            #[cfg(target_os = "macos")]
            application_reopen_smoke: false,
            #[cfg(target_os = "macos")]
            startup_item_smoke: false,
            #[cfg(target_os = "windows")]
            single_instance_smoke: false,
        }
    );
    assert_eq!(options.run_duration(), Duration::ZERO);
    assert!(!options.opens_settings_window_on_start());
    assert!(!options.automated_verification());
}

#[test]
fn positive_seconds_select_a_bounded_diagnostic_run() {
    assert_eq!(
        RunOptions::parse(["--run-seconds".to_owned(), "30".to_owned()])
            .expect("bounded options")
            .run_duration(),
        Duration::from_secs(30)
    );
}

#[test]
fn zero_seconds_remains_an_explicit_unbounded_run() {
    assert_eq!(
        RunOptions::parse(["--run-seconds".to_owned(), "0".to_owned()])
            .expect("explicit unbounded options")
            .run_duration(),
        Duration::ZERO
    );
}

/// `--flag=value` is the other spelling of the same command line.
///
/// It costs nothing to accept and it is what every other command-line tool accepts,
/// so a script that reaches for it should not be told the flag does not exist.
#[test]
fn a_run_duration_can_be_attached_to_its_flag() {
    assert_eq!(
        RunOptions::parse(["--run-seconds=30".to_owned()])
            .expect("attached value options")
            .run_duration(),
        Duration::from_secs(30)
    );
}

/// A harness run is exactly a run that named a flag other than `--run-seconds`.
///
/// This is the rule the permission prompt hangs on, and it is now derived from the
/// flag declaration rather than from a hand-written argument scan. The two checks
/// below keep it honest: the declared flags and the flags this test calls harnesses
/// have to be the same set, and every one of them has to flip the derived value. A
/// flag added above without being wired into `automated_verification` fails here
/// instead of quietly skipping the permission prompt in CI.
#[test]
fn only_the_bounded_run_duration_keeps_a_start_interactive() {
    use std::collections::BTreeSet;

    let product = RunOptions::parse(["--run-seconds".to_owned(), "0".to_owned()])
        .expect("product run options");
    assert!(!product.automated_verification());
    assert!(!product.settings_window_smoke);

    let harness: BTreeSet<&str> = [
        "--settings-window-smoke",
        "--settings-window-open-smoke",
        "--models-page-smoke",
        "--hidden-model-switch-smoke",
        "--system-menu-smoke",
        "--startup-permission-smoke",
        #[cfg(feature = "storage-test-injection")]
        "--settings-window-state-smoke",
        #[cfg(feature = "storage-test-injection")]
        "--panic-diagnostics-smoke",
        #[cfg(feature = "storage-test-injection")]
        "--panic-diagnostics-smoke-child",
        #[cfg(feature = "storage-test-injection")]
        "--diagnostics-export-smoke",
        #[cfg(feature = "storage-test-injection")]
        "--diagnostics-export-failure-smoke",
        #[cfg(target_os = "macos")]
        "--application-reopen-smoke",
        #[cfg(target_os = "macos")]
        "--startup-item-smoke",
        #[cfg(target_os = "windows")]
        "--single-instance-smoke",
        #[cfg(target_os = "windows")]
        "--single-instance-ready-file",
        #[cfg(target_os = "windows")]
        "--single-instance-result-file",
    ]
    .into_iter()
    .collect();
    let declared: BTreeSet<String> = <RunOptions as clap::CommandFactory>::command()
        .get_arguments()
        .filter_map(|argument| argument.get_long())
        .filter(|flag| *flag != "run-seconds")
        .map(str::to_owned)
        .collect();
    assert_eq!(
        declared,
        harness
            .iter()
            .map(|flag| flag.trim_start_matches("--").to_owned())
            .collect::<BTreeSet<String>>(),
        "the declared flags and the flags this test treats as harnesses are different sets"
    );

    for flag in &harness {
        // The single-instance markers need a value and a companion flag, so parsing
        // one on its own would only prove `clap` rejects it. They are covered by the
        // dedicated tests below instead.
        if flag.starts_with("--single-instance-") {
            continue;
        }
        assert!(
            RunOptions::parse([(*flag).to_owned()])
                .unwrap_or_else(|error| panic!("{flag} was rejected: {error}"))
                .automated_verification(),
            "{flag} did not mark the run as a harness"
        );
    }
    assert!(
        RunOptions::parse([
            "--run-seconds".to_owned(),
            "4".to_owned(),
            "--settings-window-smoke".to_owned(),
        ])
        .expect("harness run options")
        .automated_verification()
    );
}

/// `--help` is not a harness run; it prints and exits.
///
/// A help request used to travel the same error path as a bad command line, and this
/// is what keeps the two apart: the help text goes to stdout and the process leaves
/// normally, while a bad flag is an error.
#[test]
fn help_is_rendered_on_stdout_and_is_not_a_harness_run() {
    let error = RunOptions::parse(["--help".to_owned()]).expect_err("help is not options");
    assert!(error.help, "--help must not read as a bad command line");
    assert!(
        error.message.contains("--run-seconds"),
        "--help did not render the flag list: {}",
        error.message
    );
    assert!(
        !error.message.contains("error:"),
        "--help must not be rendered as a diagnostic: {}",
        error.message
    );

    let bad = RunOptions::parse(["--not-a-flag".to_owned()]).expect_err("unknown flag");
    assert!(!bad.help, "an unknown flag must not read as a help request");
    assert!(
        bad.message.contains("--not-a-flag"),
        "the diagnostic does not name the offending flag: {}",
        bad.message
    );
}

#[cfg(not(feature = "storage-test-injection"))]
#[test]
fn product_options_reject_storage_test_injection() {
    for (flag, harness) in [
        ("--settings-window-state-smoke", "state storage injection"),
        ("--panic-diagnostics-smoke", "panic storage injection"),
        (
            "--diagnostics-export-smoke",
            "diagnostics storage injection",
        ),
        ("--panic-diagnostics-smoke-child", "panic child injection"),
    ] {
        let error = RunOptions::parse([flag.to_owned()])
            .expect_err("default product options must reject this harness");
        assert!(
            error.message.contains(flag),
            "rejecting {harness} did not name the flag: {}",
            error.message
        );
        assert!(!error.help);
        assert!(
            !usage().contains(flag),
            "{harness} is advertised in the help of a build that rejects it"
        );
    }
}

#[test]
fn run_options_reject_missing_invalid_and_unknown_values() {
    for arguments in [
        vec!["--run-seconds".to_owned()],
        vec!["--run-seconds".to_owned(), "-1".to_owned()],
        vec!["--model".to_owned(), "standard".to_owned()],
        vec!["--environment".to_owned(), "production".to_owned()],
        vec!["--BONGOCAT_BUILD_ENV=production".to_owned()],
        vec!["--storage-root".to_owned(), "/production".to_owned()],
    ] {
        assert!(RunOptions::parse(arguments).is_err());
    }
}

#[test]
fn product_failures_exit_code_is_zero_only_without_failures() {
    let empty = Arc::new(Mutex::new(Vec::new()));
    assert_eq!(product_failures_exit_code(&empty), 0);

    let failed = Arc::new(Mutex::new(Vec::new()));
    record_failure(&failed, "runtime crashed");
    assert_eq!(product_failures_exit_code(&failed), 1);
}

#[test]
fn product_failures_exit_code_reads_the_final_shared_list() {
    // Mirrors the macOS quit path: a snapshot taken before `finish()` would have
    // reported success, so the code must be derived from the shared list the
    // shutdown future mutates (TODO P7-MACOS-SMOKE-EXIT-CODE).
    let failures = Arc::new(Mutex::new(Vec::new()));
    let at_quit_request = product_failures_exit_code(&failures);
    assert_eq!(at_quit_request, 0);

    record_failure(&failures, "product overlay presented no frames");
    assert_eq!(product_failures_exit_code(&failures), 1);
}
