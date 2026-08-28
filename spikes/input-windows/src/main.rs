use bongocat_input_windows_spike::{RawKeyboardPacket, decode_keyboard_packet};

fn main() {
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
