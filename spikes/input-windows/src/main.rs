use bongocat_input_windows_spike::{RawKeyboardPacket, decode_keyboard_packet};

#[cfg(target_os = "windows")]
mod windows_capture;

fn main() {
    #[cfg(target_os = "windows")]
    if let Some(milliseconds) =
        argument_value("--register-smoke-ms").and_then(|value| value.parse::<u64>().ok())
    {
        let report =
            windows_capture::run_registration_smoke(std::time::Duration::from_millis(milliseconds))
                .expect("Raw Input registration smoke failed");
        assert!(report.registered, "Raw Input devices were not registered");
        assert!(
            report.clean_shutdown,
            "Raw Input window did not shut down cleanly"
        );
        assert_eq!(report.decode_errors, 0, "Raw Input decoding failed");
        assert_eq!(report.callback_panics, 0, "Raw Input callback panicked");
        println!(
            "input-windows-spike: registered={} clean_shutdown={} raw_messages={} keyboard_edges={} decode_errors={} callback_panics={}",
            report.registered,
            report.clean_shutdown,
            report.raw_messages,
            report.keyboard_edges,
            report.decode_errors,
            report.callback_panics,
        );
        return;
    }

    let down = decode_keyboard_packet(RawKeyboardPacket {
        make_code: 0x1e,
        flags: 0,
    });
    let up = decode_keyboard_packet(RawKeyboardPacket {
        make_code: 0x1e,
        flags: 0x0001,
    });
    println!(
        "input-windows-spike: decoded_edges={} down={} up={}",
        2, down.pressed, up.pressed
    );
}

#[cfg(target_os = "windows")]
fn argument_value(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}
