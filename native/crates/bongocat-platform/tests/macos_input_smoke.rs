#![cfg(target_os = "macos")]

use bongocat_platform::{MacInputService, PlatformInputError};
use bongocat_runtime::{HandSide, InputBindings, PhysicalKey, RuntimeCommand, RuntimeOwner};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventType,
    CGMouseButton,
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

    let service = MacInputService::start(
        runtime.input_producer(),
        runtime.cursor_producer(),
        runtime.gamepad_axis_producer(),
    )
    .expect("input service");
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
fn synthetic_cursor_reaches_runtime_latest_value_snapshot() {
    let runtime = RuntimeOwner::start(true, 8);
    let client = runtime.client();
    client.wait_for_revision(1, TIMEOUT).expect("runtime ready");
    let service = MacInputService::start(
        runtime.input_producer(),
        runtime.cursor_producer(),
        runtime.gamepad_axis_producer(),
    )
    .expect("input service");
    let initial = client
        .wait_for_cursor_samples(1, TIMEOUT)
        .expect("initial cursor reached runtime");
    let baseline_consumed = initial.cursor.transport.consumed;
    let source = CGEventSource::new(CGEventSourceStateID::Private).expect("event source");
    let current = CGEvent::new(Some(&source)).expect("current cursor event");
    let point = CGEvent::location(Some(&current));
    let moved = CGEvent::new_mouse_event(
        Some(&source),
        CGEventType::MouseMoved,
        point,
        CGMouseButton::Left,
    )
    .expect("mouse moved event");
    CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&moved));

    let snapshot = client
        .wait_for_cursor_samples(baseline_consumed + 1, TIMEOUT)
        .expect("cursor reached runtime");
    let sample = snapshot.cursor.sample.expect("cursor sample");
    assert!((sample.position.x - point.x).abs() < f64::EPSILON);
    assert!((sample.position.y - point.y).abs() < f64::EPSILON);
    assert!((-1.0..=1.0).contains(&snapshot.model_input.pointer_x));
    assert!((-1.0..=1.0).contains(&snapshot.model_input.pointer_y));
    assert!((-1.0..=1.0).contains(&snapshot.model_input.pointer_z));

    let diagnostics = service.stop().expect("input service stop");
    assert!(diagnostics.cursor_captured >= 1);
    assert!(diagnostics.cursor_consumed >= 1);
    assert_eq!(diagnostics.cursor_display_lookup_failures, 0);
    assert_eq!(diagnostics.cursor_publish_rejections, 0);
    let stopped = runtime.shutdown(TIMEOUT).expect("runtime stop");
    assert!(stopped.cursor.transport.published >= 1);
    assert!(stopped.cursor.transport.consumed >= 1);
    assert_eq!(stopped.cursor.transport.pending, 0);
}

#[test]
#[ignore = "requires macOS Input Monitoring and Accessibility permissions"]
fn runtime_stop_cleans_up_tap_before_a_second_service_starts() {
    let runtime = RuntimeOwner::start(true, 8);
    let client = runtime.client();
    client
        .wait_for_revision(1, TIMEOUT)
        .expect("first runtime ready");
    let service = MacInputService::start(
        runtime.input_producer(),
        runtime.cursor_producer(),
        runtime.gamepad_axis_producer(),
    )
    .expect("first input service");
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
    let replacement_service = MacInputService::start(
        replacement_runtime.input_producer(),
        replacement_runtime.cursor_producer(),
        replacement_runtime.gamepad_axis_producer(),
    )
    .expect("replacement input service");
    let diagnostics = replacement_service
        .stop()
        .expect("replacement input service stop");
    assert!(diagnostics.clean_shutdown);
    replacement_runtime
        .shutdown(TIMEOUT)
        .expect("replacement runtime stop");
}
