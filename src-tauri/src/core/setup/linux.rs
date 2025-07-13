use tauri::{AppHandle, WebviewWindow};

pub fn platform(
    _app_handle: &AppHandle,
    _main_window: WebviewWindow,
    _preference_window: WebviewWindow,
) {
}

pub fn key_from_code(code: u32) -> Option<&'static str> {
    match code {
        // Function key
        1 => Some("Escape"),
        28 => Some("Return"),
        14 => Some("Backspace"),
        15 => Some("Tab"),
        57 => Some("Space"),
        58 => Some("CapsLock"),
        99 => Some("PrintScreen"),
        70 => Some("ScrollLock"),
        119 => Some("Pause"),
        69 => Some("NumLock"),
        110 => Some("Insert"),
        102 => Some("Home"),
        107 => Some("End"),
        104 => Some("PageUp"),
        109 => Some("PageDown"),
        111 => Some("Delete"),
        
        // Arrow key
        103 => Some("UpArrow"),
        108 => Some("DownArrow"),
        105 => Some("LeftArrow"),
        106 => Some("RightArrow"),
        
        // F key
        59 => Some("F1"),
        60 => Some("F2"),
        61 => Some("F3"),
        62 => Some("F4"),
        63 => Some("F5"),
        64 => Some("F6"),
        65 => Some("F7"),
        66 => Some("F8"),
        67 => Some("F9"),
        68 => Some("F10"),
        87 => Some("F11"),
        88 => Some("F12"),
        
        // Numeric
        2 => Some("Num1"),
        3 => Some("Num2"),
        4 => Some("Num3"),
        5 => Some("Num4"),
        6 => Some("Num5"),
        7 => Some("Num6"),
        8 => Some("Num7"),
        9 => Some("Num8"),
        10 => Some("Num9"),
        11 => Some("Num0"),
        
        // Alphabetic
        16 => Some("KeyQ"),
        17 => Some("KeyW"),
        18 => Some("KeyE"),
        19 => Some("KeyR"),
        20 => Some("KeyT"),
        21 => Some("KeyY"),
        22 => Some("KeyU"),
        23 => Some("KeyI"),
        24 => Some("KeyO"),
        25 => Some("KeyP"),
        30 => Some("KeyA"),
        31 => Some("KeyS"),
        32 => Some("KeyD"),
        33 => Some("KeyF"),
        34 => Some("KeyG"),
        35 => Some("KeyH"),
        36 => Some("KeyJ"),
        37 => Some("KeyK"),
        38 => Some("KeyL"),
        44 => Some("KeyZ"),
        45 => Some("KeyX"),
        46 => Some("KeyC"),
        47 => Some("KeyV"),
        48 => Some("KeyB"),
        49 => Some("KeyN"),
        50 => Some("KeyM"),
        
        // Symbolic
        41 => Some("BackQuote"),
        12 => Some("Minus"),
        13 => Some("Equal"),
        26 => Some("LeftBracket"),
        27 => Some("RightBracket"),
        39 => Some("SemiColon"),
        40 => Some("Quote"),
        43 => Some("BackSlash"),
        86 => Some("IntlBackslash"),
        89 => Some("IntlRo"),
        124 => Some("IntlYen"),
        101 => Some("KanaMode"),
        51 => Some("Comma"),
        52 => Some("Dot"),
        53 => Some("Slash"),
        
        // Control key
        29 => Some("ControlLeft"),
        97 => Some("ControlRight"),
        42 => Some("ShiftLeft"),
        54 => Some("ShiftRight"),
        56 => Some("Alt"),
        100 => Some("AltGr"),
        125 => Some("MetaLeft"),
        126 => Some("MetaRight"),
        127 => Some("Apps"),
        
        // NumPad
        55 => Some("KpMultiply"),
        78 => Some("KpMinus"),
        74 => Some("KpPlus"),
        98 => Some("KpDivide"),
        117 => Some("KpEqual"),
        121 => Some("KpComma"),
        96 => Some("KpReturn"),
        83 => Some("KpDecimal"),
        79 => Some("Kp1"),
        80 => Some("Kp2"),
        81 => Some("Kp3"),
        75 => Some("Kp4"),
        76 => Some("Kp5"),
        77 => Some("Kp6"),
        71 => Some("Kp7"),
        72 => Some("Kp8"),
        73 => Some("Kp9"),
        82 => Some("Kp0"),
        
        // Media key
        115 => Some("VolumeUp"),
        114 => Some("VolumeDown"),
        113 => Some("VolumeMute"),
        
        // Language key
        90 => Some("Lang1"),
        91 => Some("Lang2"),
        92 => Some("Lang3"),
        93 => Some("Lang4"),
        94 => Some("Lang5"),
        
        // Other
        _ => None,
    }
}

