//! No-device smoke for the formal gilrs gamepad adapter.
//!
//! This intentionally uses the product input service rather than a second
//! backend probe. It verifies WGI context startup/shutdown on a Windows runner;
//! it does not claim physical controller coverage.

#[cfg(target_os = "windows")]
fn main() {
    use bongocat_platform::WindowsInputService;
    use bongocat_runtime::RuntimeOwner;
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(2);
    let runtime = RuntimeOwner::start(true, 64);
    let service = WindowsInputService::start(
        runtime.input_producer(),
        runtime.cursor_producer(),
        runtime.gamepad_axis_producer(),
    )
    .expect("Windows input service with gilrs gamepad adapter");
    std::thread::sleep(Duration::from_millis(250));
    let diagnostics = service.stop().expect("Windows input service shutdown");
    assert!(diagnostics.clean_shutdown);
    assert_eq!(diagnostics.gamepad_backend_failures, 0);
    assert_eq!(diagnostics.capture_queue_overflows, 0);
    println!(
        "gilrs-gamepad-smoke: windows started=true stopped=true backend_failures=0 capture_queue_overflows=0 clean_shutdown=true (no-device context only; physical WGI coverage remains a release gate)"
    );
    runtime.shutdown(TIMEOUT).expect("runtime shutdown");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("gilrs-gamepad-smoke: target OS is not Windows; probe skipped");
}
