use super::{InputSource, InputSourceError};
use windows::{
    Win32::{
        Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDDISPLAYNAME},
        System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        UI::{
            Input::{Ime::ImmGetDescriptionW, KeyboardAndMouse::GetKeyboardLayout},
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    },
    core::{PCWSTR, w},
};

pub(super) struct Monitor {
    cached: Option<InputSource>,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, InputSourceError> {
        Ok(Self { cached: None })
    }

    pub(super) fn snapshot(&mut self) -> Result<InputSource, InputSourceError> {
        // SAFETY: read-only USER32 queries, no retained HWND or pointer. A
        // foreground-less/secure desktop returns unavailable, not our layout.
        let layout = unsafe {
            let foreground = GetForegroundWindow();
            if foreground.is_invalid() {
                return Err(InputSourceError);
            }
            let thread = GetWindowThreadProcessId(foreground, None);
            if thread == 0 {
                return Err(InputSourceError);
            }
            GetKeyboardLayout(thread)
        };
        if layout.0.is_null() {
            return Err(InputSourceError);
        }
        let value = layout.0 as usize as u32;
        let id = format!("{value:08x}");
        if let Some(source) = self.cached.as_ref().filter(|source| source.id == id) {
            return Ok(source.clone());
        }
        let mut buffer = [0u16; 256];
        // SAFETY: windows-rs passes the exact slice capacity to IMM32. There is
        // no synchronous message to the foreground application or injected hook.
        let count = unsafe { ImmGetDescriptionW(layout, Some(&mut buffer)) } as usize;
        let name = if count > 0 && count < buffer.len() {
            String::from_utf16_lossy(&buffer[..count])
        } else if let Some(name) = layout_name(value) {
            name
        } else {
            // SAFETY: bounded locale lookup; an HKL's low word is its LANGID.
            let count = unsafe {
                GetLocaleInfoW(
                    value & 0xffff,
                    LOCALE_SLOCALIZEDDISPLAYNAME,
                    Some(&mut buffer),
                )
            };
            if count <= 1 {
                return Err(InputSourceError);
            }
            String::from_utf16_lossy(&buffer[..count as usize - 1])
        };
        let source = InputSource {
            id,
            name,
            icon_rgba: None,
        };
        self.cached = Some(source.clone());
        Ok(source)
    }
}

fn layout_name(layout: u32) -> Option<String> {
    // IME HKLs retain their full E... identifier; ordinary standard layouts
    // repeat LANGID in the high word, whereas their registry KLID is 0000LANG.
    let klid = if layout >> 28 == 0xe {
        layout
    } else if layout >> 16 == layout & 0xffff {
        layout & 0xffff
    } else {
        return None;
    };
    let key: Vec<u16> =
        format!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{klid:08X}\0")
            .encode_utf16()
            .collect();
    let mut buffer = [0u16; 256];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: terminated key/value strings, fixed writable buffer with its byte
    // capacity, and a REG_SZ filter. No registry handle is opened or retained.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            w!("Layout Text"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if result.is_err() {
        return None;
    }
    let count = buffer.iter().position(|c| *c == 0)?;
    (count != 0).then(|| String::from_utf16_lossy(&buffer[..count]))
}
