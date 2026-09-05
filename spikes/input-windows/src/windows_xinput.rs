use bongocat_input_queue_spike::LatestValuesDiagnostics;
use bongocat_input_windows_spike::{XInputProducer, XInputProducerDiagnostics, XInputSnapshot};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::ERROR_DEVICE_NOT_CONNECTED,
    UI::Input::XboxController::{XINPUT_STATE, XInputEnable, XInputGetState},
};

const XINPUT_SLOT_COUNT: u32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XInputProbeReport {
    pub started: bool,
    pub service_enabled: bool,
    pub service_disabled: bool,
    pub api_calls: u64,
    pub peak_connected: u64,
    pub reliable_events: u64,
    pub axis_samples: u64,
    pub clean_shutdown: bool,
    pub producer: XInputProducerDiagnostics,
    pub axes: LatestValuesDiagnostics,
}

pub fn run_xinput_probe(duration: Duration) -> XInputProbeReport {
    // SAFETY: this process owns the spike's XInput service lifetime. Input is
    // enabled before the first query and disabled after the final poll.
    unsafe { XInputEnable(true) };
    let mut producer = XInputProducer::new(256);
    let mut api_calls = 0u64;
    let mut peak_connected = 0u64;
    let mut reliable_events = 0u64;
    let mut axis_samples = 0u64;
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        for slot in 0..XINPUT_SLOT_COUNT {
            let mut state = XINPUT_STATE::default();
            // SAFETY: `state` is writable for the duration of the call and
            // slot is limited to XInput's documented 0..4 user index range.
            let result = unsafe { XInputGetState(slot, &mut state) };
            api_calls += 1;
            if result == 0 {
                producer.observe_slot(slot as u8, Some(snapshot_from_state(state)));
            } else if result == ERROR_DEVICE_NOT_CONNECTED.0 {
                producer.observe_slot(slot as u8, None);
            } else {
                producer.record_query_error();
            }
        }
        peak_connected = peak_connected.max(producer.active_connections().len() as u64);
        reliable_events += producer.drain_events().len() as u64;
        axis_samples += producer.drain_axes().len() as u64;
        std::thread::sleep(Duration::from_millis(8));
    }
    // SAFETY: no further XInput query is issued after this owner disables the
    // process-global service.
    unsafe { XInputEnable(false) };
    producer.close();
    reliable_events += producer.drain_events().len() as u64;
    axis_samples += producer.drain_axes().len() as u64;
    let producer_diagnostics = producer.diagnostics();
    let axis_diagnostics = producer.axis_diagnostics();
    let clean_shutdown = producer.active_connections().is_empty()
        && producer.axes_fully_accounted()
        && producer_diagnostics.rejected_after_close == 0;
    XInputProbeReport {
        started: true,
        service_enabled: true,
        service_disabled: true,
        api_calls,
        peak_connected,
        reliable_events,
        axis_samples,
        clean_shutdown,
        producer: producer_diagnostics,
        axes: axis_diagnostics,
    }
}

fn snapshot_from_state(state: XINPUT_STATE) -> XInputSnapshot {
    XInputSnapshot {
        button_bits: state.Gamepad.wButtons.0,
        left_trigger: state.Gamepad.bLeftTrigger,
        right_trigger: state.Gamepad.bRightTrigger,
        left_x: state.Gamepad.sThumbLX,
        left_y: state.Gamepad.sThumbLY,
        right_x: state.Gamepad.sThumbRX,
        right_y: state.Gamepad.sThumbRY,
    }
}
