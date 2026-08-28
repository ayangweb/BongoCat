#![forbid(unsafe_code)]

pub const RI_KEY_BREAK: u16 = 0x0001;
pub const RI_KEY_E0: u16 = 0x0002;
pub const RI_KEY_E1: u16 = 0x0004;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PhysicalKey {
    Escape,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Digit0,
    Minus,
    Equal,
    Backspace,
    Tab,
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    BracketLeft,
    BracketRight,
    Enter,
    ControlLeft,
    ControlRight,
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    Semicolon,
    Apostrophe,
    Grave,
    ShiftLeft,
    Backslash,
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    ShiftRight,
    AltLeft,
    AltRight,
    Space,
    CapsLock,
    PrintScreen,
    Pause,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    MetaLeft,
    MetaRight,
    Unknown { scan_code: u16, flags: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawKeyboardPacket {
    pub make_code: u16,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardEdge {
    pub key: PhysicalKey,
    pub pressed: bool,
}

pub fn decode_keyboard_packet(packet: RawKeyboardPacket) -> KeyboardEdge {
    KeyboardEdge {
        key: map_scan_code(packet.make_code, packet.flags),
        pressed: packet.flags & RI_KEY_BREAK == 0,
    }
}

pub fn map_scan_code(make_code: u16, flags: u16) -> PhysicalKey {
    let extended = flags & RI_KEY_E0 != 0;
    let e1 = flags & RI_KEY_E1 != 0;
    if e1 && make_code == 0x1d {
        return PhysicalKey::Pause;
    }
    if extended {
        return match make_code {
            0x1c => PhysicalKey::Enter,
            0x1d => PhysicalKey::ControlRight,
            0x35 => PhysicalKey::Slash,
            0x37 => PhysicalKey::PrintScreen,
            0x38 => PhysicalKey::AltRight,
            0x47 => PhysicalKey::Home,
            0x48 => PhysicalKey::ArrowUp,
            0x49 => PhysicalKey::PageUp,
            0x4b => PhysicalKey::ArrowLeft,
            0x4d => PhysicalKey::ArrowRight,
            0x4f => PhysicalKey::End,
            0x50 => PhysicalKey::ArrowDown,
            0x51 => PhysicalKey::PageDown,
            0x52 => PhysicalKey::Insert,
            0x53 => PhysicalKey::Delete,
            0x5b => PhysicalKey::MetaLeft,
            0x5c => PhysicalKey::MetaRight,
            _ => PhysicalKey::Unknown {
                scan_code: make_code,
                flags,
            },
        };
    }
    match make_code {
        0x01 => PhysicalKey::Escape,
        0x02 => PhysicalKey::Digit1,
        0x03 => PhysicalKey::Digit2,
        0x04 => PhysicalKey::Digit3,
        0x05 => PhysicalKey::Digit4,
        0x06 => PhysicalKey::Digit5,
        0x07 => PhysicalKey::Digit6,
        0x08 => PhysicalKey::Digit7,
        0x09 => PhysicalKey::Digit8,
        0x0a => PhysicalKey::Digit9,
        0x0b => PhysicalKey::Digit0,
        0x0c => PhysicalKey::Minus,
        0x0d => PhysicalKey::Equal,
        0x0e => PhysicalKey::Backspace,
        0x0f => PhysicalKey::Tab,
        0x10 => PhysicalKey::Q,
        0x11 => PhysicalKey::W,
        0x12 => PhysicalKey::E,
        0x13 => PhysicalKey::R,
        0x14 => PhysicalKey::T,
        0x15 => PhysicalKey::Y,
        0x16 => PhysicalKey::U,
        0x17 => PhysicalKey::I,
        0x18 => PhysicalKey::O,
        0x19 => PhysicalKey::P,
        0x1a => PhysicalKey::BracketLeft,
        0x1b => PhysicalKey::BracketRight,
        0x1c => PhysicalKey::Enter,
        0x1d => PhysicalKey::ControlLeft,
        0x1e => PhysicalKey::A,
        0x1f => PhysicalKey::S,
        0x20 => PhysicalKey::D,
        0x21 => PhysicalKey::F,
        0x22 => PhysicalKey::G,
        0x23 => PhysicalKey::H,
        0x24 => PhysicalKey::J,
        0x25 => PhysicalKey::K,
        0x26 => PhysicalKey::L,
        0x27 => PhysicalKey::Semicolon,
        0x28 => PhysicalKey::Apostrophe,
        0x29 => PhysicalKey::Grave,
        0x2a => PhysicalKey::ShiftLeft,
        0x2b => PhysicalKey::Backslash,
        0x2c => PhysicalKey::Z,
        0x2d => PhysicalKey::X,
        0x2e => PhysicalKey::C,
        0x2f => PhysicalKey::V,
        0x30 => PhysicalKey::B,
        0x31 => PhysicalKey::N,
        0x32 => PhysicalKey::M,
        0x33 => PhysicalKey::Comma,
        0x34 => PhysicalKey::Period,
        0x35 => PhysicalKey::Slash,
        0x36 => PhysicalKey::ShiftRight,
        0x38 => PhysicalKey::AltLeft,
        0x39 => PhysicalKey::Space,
        0x3a => PhysicalKey::CapsLock,
        _ => PhysicalKey::Unknown {
            scan_code: make_code,
            flags,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_flag_is_the_only_pressed_edge_boundary() {
        assert_eq!(
            decode_keyboard_packet(RawKeyboardPacket {
                make_code: 0x1e,
                flags: 0
            }),
            KeyboardEdge {
                key: PhysicalKey::A,
                pressed: true
            }
        );
        assert_eq!(
            decode_keyboard_packet(RawKeyboardPacket {
                make_code: 0x1e,
                flags: RI_KEY_BREAK
            }),
            KeyboardEdge {
                key: PhysicalKey::A,
                pressed: false
            }
        );
    }

    #[test]
    fn extended_flags_distinguish_right_modifiers_and_navigation() {
        assert_eq!(map_scan_code(0x1d, RI_KEY_E0), PhysicalKey::ControlRight);
        assert_eq!(map_scan_code(0x1d, 0), PhysicalKey::ControlLeft);
        assert_eq!(map_scan_code(0x38, RI_KEY_E0), PhysicalKey::AltRight);
        assert_eq!(map_scan_code(0x38, 0), PhysicalKey::AltLeft);
        assert_eq!(map_scan_code(0x4b, RI_KEY_E0), PhysicalKey::ArrowLeft);
    }

    #[test]
    fn e1_pause_sequence_is_not_mistaken_for_left_control() {
        assert_eq!(map_scan_code(0x1d, RI_KEY_E1), PhysicalKey::Pause);
        assert_eq!(
            map_scan_code(0x1d, RI_KEY_E1 | RI_KEY_BREAK),
            PhysicalKey::Pause
        );
    }

    #[test]
    fn print_screen_and_unknown_codes_are_retained() {
        assert_eq!(map_scan_code(0x37, RI_KEY_E0), PhysicalKey::PrintScreen);
        assert_eq!(
            map_scan_code(0x7f, 0),
            PhysicalKey::Unknown {
                scan_code: 0x7f,
                flags: 0
            }
        );
    }
}
