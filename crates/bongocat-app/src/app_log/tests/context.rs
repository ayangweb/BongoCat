//! Context is bounded, and record-once is per code and context.

use super::*;
#[test]
fn typed_context_stays_on_one_bounded_line() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    handle.record(
        ApplicationLogEvent::new(ApplicationLogCode::RuntimeUnavailable)
            .with_context(ApplicationLogContext::Operation("load_model")),
    );
    let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
    assert_eq!(contents.lines().count(), 1);
    assert!(contents.contains("operation=load_model"));
}

#[test]
fn record_once_suppresses_only_the_same_code_and_context() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    let event = ApplicationLogEvent::new(ApplicationLogCode::RuntimeUnavailable)
        .with_context(ApplicationLogContext::Operation("start"));
    handle.record_once(event.clone());
    handle.record_once(event.clone());
    handle.record_once(event.clone());
    handle.record_once(event.with_context(ApplicationLogContext::Operation("stop")));
    assert_eq!(handle.diagnostics().written, 2);
}
