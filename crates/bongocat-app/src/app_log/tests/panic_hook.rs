//! A panic writes one line and never waits for the lock.

use super::*;
use std::panic;

#[test]
fn panic_hook_writes_only_a_stable_event_and_restores_the_previous_hook() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    let hook = handle.install_panic_hook();
    let panic =
        std::thread::spawn(|| panic!("private payload /Users/example/secret-model/model3.json"))
            .join();
    assert!(panic.is_err());
    drop(hook);

    let contents = fs::read_to_string(only_log(directory.path())).expect("log contents");
    assert!(contents.contains("ERROR [application] application/panicked | Application panicked"));
    assert!(!contents.contains("secret-model"));
    assert!(handle.diagnostics().written >= 1);
}

#[test]
fn panic_record_drops_instead_of_waiting_for_the_log_lock() {
    let directory = tempdir().expect("temporary directory");
    let handle = ApplicationLogHandle::install(directory.path()).expect("application log");
    let state = handle.sink.state.lock().expect("state lock");
    handle.sink.try_record(ApplicationLogEvent::panicked());
    assert_eq!(state.diagnostics.written, 0);
}
