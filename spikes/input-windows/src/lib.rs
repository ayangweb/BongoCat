#![forbid(unsafe_code)]

use std::collections::BTreeSet;

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
pub struct RawInputHeader {
    pub declared_size: usize,
    pub input_type: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInputError {
    TruncatedHeader,
    DeclaredSizeExceedsBuffer,
    UnsupportedInputType,
    TruncatedKeyboardPayload,
}

/// Decode the stable byte layout returned by `GetRawInputData` for a keyboard.
///
/// The platform wrapper must pass the header size reported by the target ABI:
/// 16 bytes for 32-bit and 24 bytes for 64-bit Windows. No pointer or handle
/// from the native header is exposed beyond this validation boundary.
pub fn decode_raw_keyboard_bytes(
    header: RawInputHeader,
    bytes: &[u8],
    header_size: usize,
) -> Result<RawKeyboardPacket, RawInputError> {
    if bytes.len() < header_size || bytes.len() < 8 {
        return Err(RawInputError::TruncatedHeader);
    }
    if header.declared_size > bytes.len() {
        return Err(RawInputError::DeclaredSizeExceedsBuffer);
    }
    if header.input_type != 1 {
        return Err(RawInputError::UnsupportedInputType);
    }
    let keyboard_offset = header_size;
    let keyboard_end = keyboard_offset
        .checked_add(4)
        .ok_or(RawInputError::TruncatedKeyboardPayload)?;
    if header.declared_size < keyboard_end || bytes.len() < keyboard_end {
        return Err(RawInputError::TruncatedKeyboardPayload);
    }
    let make_code = u16::from_le_bytes([bytes[keyboard_offset], bytes[keyboard_offset + 1]]);
    let flags = u16::from_le_bytes([bytes[keyboard_offset + 2], bytes[keyboard_offset + 3]]);
    Ok(RawKeyboardPacket { make_code, flags })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardEdge {
    pub key: PhysicalKey,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualKeyCode(u16);

impl VirtualKeyCode {
    pub const fn as_i32(self) -> i32 {
        self.0 as i32
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyStateSnapshot {
    pub still_pressed: BTreeSet<PhysicalKey>,
    pub queried: usize,
    pub unqueryable: usize,
    pub reset_required: bool,
}

/// Build an OS-state snapshot for keys that the local runtime believes are down.
///
/// Unknown keys are retained instead of being falsely released. The caller must
/// respond to `reset_required` with a lifecycle Reset because Windows cannot
/// reliably query those keys by virtual-key code.
pub fn collect_key_state_snapshot_with(
    candidates: &BTreeSet<PhysicalKey>,
    mut is_pressed: impl FnMut(VirtualKeyCode) -> bool,
) -> KeyStateSnapshot {
    let mut report = KeyStateSnapshot::default();
    for key in candidates {
        let Some(virtual_key) = virtual_key_for_reconciliation(*key) else {
            report.still_pressed.insert(*key);
            report.unqueryable += 1;
            report.reset_required = true;
            continue;
        };
        report.queried += 1;
        if is_pressed(virtual_key) {
            report.still_pressed.insert(*key);
        }
    }
    report
}

pub const fn virtual_key_for_reconciliation(key: PhysicalKey) -> Option<VirtualKeyCode> {
    let value = match key {
        PhysicalKey::Escape => 0x1b,
        PhysicalKey::Digit1 => 0x31,
        PhysicalKey::Digit2 => 0x32,
        PhysicalKey::Digit3 => 0x33,
        PhysicalKey::Digit4 => 0x34,
        PhysicalKey::Digit5 => 0x35,
        PhysicalKey::Digit6 => 0x36,
        PhysicalKey::Digit7 => 0x37,
        PhysicalKey::Digit8 => 0x38,
        PhysicalKey::Digit9 => 0x39,
        PhysicalKey::Digit0 => 0x30,
        PhysicalKey::Minus => 0xbd,
        PhysicalKey::Equal => 0xbb,
        PhysicalKey::Backspace => 0x08,
        PhysicalKey::Tab => 0x09,
        PhysicalKey::Q => 0x51,
        PhysicalKey::W => 0x57,
        PhysicalKey::E => 0x45,
        PhysicalKey::R => 0x52,
        PhysicalKey::T => 0x54,
        PhysicalKey::Y => 0x59,
        PhysicalKey::U => 0x55,
        PhysicalKey::I => 0x49,
        PhysicalKey::O => 0x4f,
        PhysicalKey::P => 0x50,
        PhysicalKey::BracketLeft => 0xdb,
        PhysicalKey::BracketRight => 0xdd,
        PhysicalKey::Enter => 0x0d,
        PhysicalKey::ControlLeft => 0xa2,
        PhysicalKey::ControlRight => 0xa3,
        PhysicalKey::A => 0x41,
        PhysicalKey::S => 0x53,
        PhysicalKey::D => 0x44,
        PhysicalKey::F => 0x46,
        PhysicalKey::G => 0x47,
        PhysicalKey::H => 0x48,
        PhysicalKey::J => 0x4a,
        PhysicalKey::K => 0x4b,
        PhysicalKey::L => 0x4c,
        PhysicalKey::Semicolon => 0xba,
        PhysicalKey::Apostrophe => 0xde,
        PhysicalKey::Grave => 0xc0,
        PhysicalKey::ShiftLeft => 0xa0,
        PhysicalKey::Backslash => 0xdc,
        PhysicalKey::Z => 0x5a,
        PhysicalKey::X => 0x58,
        PhysicalKey::C => 0x43,
        PhysicalKey::V => 0x56,
        PhysicalKey::B => 0x42,
        PhysicalKey::N => 0x4e,
        PhysicalKey::M => 0x4d,
        PhysicalKey::Comma => 0xbc,
        PhysicalKey::Period => 0xbe,
        PhysicalKey::Slash => 0xbf,
        PhysicalKey::ShiftRight => 0xa1,
        PhysicalKey::AltLeft => 0xa4,
        PhysicalKey::AltRight => 0xa5,
        PhysicalKey::Space => 0x20,
        PhysicalKey::CapsLock => 0x14,
        PhysicalKey::PrintScreen => 0x2c,
        PhysicalKey::Pause => 0x13,
        PhysicalKey::Insert => 0x2d,
        PhysicalKey::Delete => 0x2e,
        PhysicalKey::Home => 0x24,
        PhysicalKey::End => 0x23,
        PhysicalKey::PageUp => 0x21,
        PhysicalKey::PageDown => 0x22,
        PhysicalKey::ArrowLeft => 0x25,
        PhysicalKey::ArrowRight => 0x27,
        PhysicalKey::ArrowUp => 0x26,
        PhysicalKey::ArrowDown => 0x28,
        PhysicalKey::MetaLeft => 0x5b,
        PhysicalKey::MetaRight => 0x5c,
        PhysicalKey::Unknown { .. } => return None,
    };
    Some(VirtualKeyCode(value))
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

    #[test]
    fn reconciliation_uses_distinct_modifier_virtual_keys() {
        assert_eq!(
            virtual_key_for_reconciliation(PhysicalKey::ControlLeft),
            Some(VirtualKeyCode(0xa2))
        );
        assert_eq!(
            virtual_key_for_reconciliation(PhysicalKey::ControlRight),
            Some(VirtualKeyCode(0xa3))
        );
        assert_eq!(
            virtual_key_for_reconciliation(PhysicalKey::AltLeft),
            Some(VirtualKeyCode(0xa4))
        );
        assert_eq!(
            virtual_key_for_reconciliation(PhysicalKey::AltRight),
            Some(VirtualKeyCode(0xa5))
        );
    }

    #[test]
    fn reconciliation_only_queries_local_pressed_candidates() {
        let candidates = BTreeSet::from([
            PhysicalKey::ControlLeft,
            PhysicalKey::AltLeft,
            PhysicalKey::A,
        ]);
        let mut queried = Vec::new();
        let report = collect_key_state_snapshot_with(&candidates, |virtual_key| {
            queried.push(virtual_key);
            virtual_key == VirtualKeyCode(0xa2)
        });
        assert_eq!(queried.len(), 3);
        assert_eq!(report.queried, 3);
        assert_eq!(report.unqueryable, 0);
        assert!(!report.reset_required);
        assert_eq!(
            report.still_pressed,
            BTreeSet::from([PhysicalKey::ControlLeft])
        );
    }

    #[test]
    fn unknown_keys_require_reset_instead_of_false_release() {
        let unknown = PhysicalKey::Unknown {
            scan_code: 0x7f,
            flags: RI_KEY_E0,
        };
        let candidates = BTreeSet::from([unknown]);
        let mut query_count = 0;
        let report = collect_key_state_snapshot_with(&candidates, |_| {
            query_count += 1;
            false
        });
        assert_eq!(query_count, 0);
        assert_eq!(report.queried, 0);
        assert_eq!(report.unqueryable, 1);
        assert!(report.reset_required);
        assert_eq!(report.still_pressed, candidates);
    }

    #[test]
    fn raw_input_decoder_rejects_truncated_or_non_keyboard_packets() {
        let header = RawInputHeader {
            declared_size: 28,
            input_type: 1,
        };
        assert_eq!(
            decode_raw_keyboard_bytes(header, &[0; 12], 24),
            Err(RawInputError::TruncatedHeader)
        );
        assert_eq!(
            decode_raw_keyboard_bytes(
                RawInputHeader {
                    declared_size: 28,
                    input_type: 2,
                },
                &[0; 28],
                24,
            ),
            Err(RawInputError::UnsupportedInputType)
        );
        assert_eq!(
            decode_raw_keyboard_bytes(
                RawInputHeader {
                    declared_size: 40,
                    input_type: 1,
                },
                &[0; 28],
                24,
            ),
            Err(RawInputError::DeclaredSizeExceedsBuffer)
        );
    }

    #[test]
    fn raw_input_decoder_reads_keyboard_make_code_and_flags() {
        let mut bytes = vec![0u8; 28];
        bytes[24..26].copy_from_slice(&0x1du16.to_le_bytes());
        bytes[26..28].copy_from_slice(&RI_KEY_E0.to_le_bytes());
        let packet = decode_raw_keyboard_bytes(
            RawInputHeader {
                declared_size: bytes.len(),
                input_type: 1,
            },
            &bytes,
            24,
        )
        .unwrap();
        assert_eq!(
            decode_keyboard_packet(packet).key,
            PhysicalKey::ControlRight
        );
        assert!(decode_keyboard_packet(packet).pressed);
    }
}
