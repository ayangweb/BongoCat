#![cfg(target_os = "macos")]

use bongocat_platform::{MacInputService, PlatformInputError};
use bongocat_runtime::{HandSide, InputBindings, PhysicalKey, RuntimeCommand, RuntimeOwner};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventType,
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(2);

#[test]
#[ignore = "requires macOS Input Monitoring and Accessibility permissions"]
fn synthetic_shift_reaches_runtime_and_releases_cleanly() {
    let runtime = RuntimeOwner::start(true, 64);
    let client = runtime.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let shift = PhysicalKey::from_hid_usage(0xe1);
    let bindings = InputBindings::new(BTreeMap::from([(shift, HandSide::Left)]));
    let binding_sequence = client
        .send(RuntimeCommand::SetInputBindings(Arc::new(bindings)))
        .expect("binding command");
    client
        .wait_for_command(binding_sequence, TIMEOUT)
        .expect("bindings applied");

    let service = MacInputService::start(runtime.input_producer()).expect("input service");
    let source = CGEventSource::new(CGEventSourceStateID::Private).expect("event source");
    let down = CGEvent::new_keyboard_event(Some(&source), 56, true).expect("shift down");
    CGEvent::set_type(Some(&down), CGEventType::FlagsChanged);
    CGEvent::set_flags(Some(&down), CGEventFlags::MaskShift);
    let up = CGEvent::new_keyboard_event(Some(&source), 56, false).expect("shift up");
    CGEvent::set_type(Some(&up), CGEventType::FlagsChanged);
    CGEvent::set_flags(Some(&up), CGEventFlags::empty());

    let before_down = client.snapshot().revision;
    CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&down));
    let pressed = client
        .wait_for_revision(before_down + 1, TIMEOUT)
        .expect("shift down reached runtime");
    assert!(pressed.model_input.left_hand_down);

    let before_up = pressed.revision;
    CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&up));
    let released = client
        .wait_for_revision(before_up + 1, TIMEOUT)
        .expect("shift up reached runtime");
    assert!(!released.model_input.left_hand_down);

    let diagnostics = service.stop().expect("input service stop");
    assert_eq!(diagnostics.callback_panics, 0);
    assert_eq!(diagnostics.capture_queue_overflows, 0);
    assert_eq!(diagnostics.runtime_queue_overflows, 0);
    assert!(diagnostics.consumed_edges >= 2);
    assert!(diagnostics.clean_shutdown);
    let stopped = runtime.shutdown(TIMEOUT).expect("runtime stop");
    assert!(!stopped.model_input.left_hand_down);
}

#[test]
#[ignore = "requires macOS Input Monitoring and Accessibility permissions"]
fn runtime_stop_cleans_up_tap_before_a_second_service_starts() {
    let runtime = RuntimeOwner::start(true, 8);
    let client = runtime.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("first runtime ready");
    let service = MacInputService::start(runtime.input_producer()).expect("first input service");
    runtime.shutdown(TIMEOUT).expect("first runtime stop");

    let source = CGEventSource::new(CGEventSourceStateID::Private).expect("event source");
    let down = CGEvent::new_keyboard_event(Some(&source), 0, true).expect("key down");
    CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&down));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(service.stop(), Err(PlatformInputError::RuntimeStopped));

    let replacement_runtime = RuntimeOwner::start(true, 8);
    let replacement_client = replacement_runtime.client();
    replacement_client
        .wait_for_revision(1, TIMEOUT)
        .expect("replacement runtime ready");
    let replacement_service = MacInputService::start(replacement_runtime.input_producer())
        .expect("replacement input service");
    let diagnostics = replacement_service
        .stop()
        .expect("replacement input service stop");
    assert!(diagnostics.clean_shutdown);
    replacement_runtime
        .shutdown(TIMEOUT)
        .expect("replacement runtime stop");
}
