//! Windows input source observation.
//!
//! The system's answer to "which input source is the foreground using" has two
//! parts: the keyboard layout of the thread that owns keyboard focus, and the
//! input method the session has activated inside that layout. The second part is
//! what the taskbar's input indicator shows, and what a freshly started process
//! picks up.
//!
//! Reading it from a running process needs one non-obvious rule. Text Services
//! freezes a process's profile state at the moment its first profile manager is
//! created: while any `ITfInputProcessorProfileMgr` is alive, `GetActiveProfile`
//! keeps answering with that snapshot and a switch made anywhere else is
//! invisible — which is why an indicator built on a retained manager only corrects
//! itself after a restart. Releasing the last manager and creating a new one
//! re-reads the session. This adapter therefore holds none: it creates a manager,
//! reads the active profile, and drops it again on every snapshot.
//!
//! Everything else follows from that identity. The localized name and the icon
//! come from the profile's own registration, and a plain keyboard layout is named
//! from the layout registry. Nothing is guessed, and nothing is cached except the
//! decoded artwork, which cannot change while the registration exists.
//!
//! The conversion mode inside an input method (the 中/英 toggle) is not part of
//! what this adapter reports: the first release shows the input method, and a
//! toggle inside one method is not a change of source.
//!
//! Observation is a read, not a subscription. Windows has no notification that
//! reaches a background process when the input method changes inside the window
//! it is already using: the Text Services activation sink only notifies a thread
//! that owns the focused window, a session-wide layout change reaches a
//! background thread's next read without raising any sink callback, and no
//! `WM_SETTINGCHANGE` or `WM_INPUTLANGCHANGE` broadcast is sent to other windows.
//! A foreground hook would cover window switches only, which the host's read
//! already covers, so the host's clock stays the trigger and every read is a
//! cheap, self-contained snapshot.

use super::{InputSource, InputSourceError};
use std::{collections::HashMap, mem::size_of};
use windows::{
    Win32::{
        Foundation::HWND,
        Globalization::{GetLocaleInfoW, LOCALE_SISO639LANGNAME2, LOCALE_SLOCALIZEDDISPLAYNAME},
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
            GetDIBits, GetObjectW, HBITMAP, HGDIOBJ, RGBQUAD, ReleaseDC,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Environment::ExpandEnvironmentStringsW,
            Registry::{
                HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD,
                RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
            },
        },
        UI::{
            Input::{
                Ime::{ImmGetDescriptionW, ImmGetIMEFileNameW},
                KeyboardAndMouse::{GetKeyboardLayout, HKL},
            },
            Shell::{SHDefExtractIconW, SHLoadIndirectString},
            TextServices::{
                CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD,
                ITfInputProcessorProfileMgr, TF_INPUTPROCESSORPROFILE,
                TF_PROFILETYPE_INPUTPROCESSOR,
            },
            WindowsAndMessaging::{
                DestroyIcon, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetIconInfo,
                GetWindowThreadProcessId, HICON, ICONINFO,
            },
        },
    },
    core::{GUID, PCWSTR},
};

/// The icon slot the HUD reserves; `StatusIcon::Rgba` is exactly this.
const ICON_SIDE: u32 = 32;

/// Input methods that register a substitute layout handle use the `0xE` prefix;
/// that is how a handle distinguishes an IME from a plain layout.
const IME_LAYOUT_PREFIX: u32 = 0xe;

/// The input method the session has activated, as the input indicator reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Active {
    clsid: GUID,
    guid: GUID,
    langid: u16,
    /// A text service rather than a keyboard layout. Both are profiles, and the
    /// keyboard category reports whichever is running.
    processor: bool,
}

impl Active {
    /// The stable identity of the running input method. Two input methods of one
    /// language share a layout handle, so the handle alone cannot identify them;
    /// the profile can.
    fn id(&self) -> String {
        format!("{}{}", guid_key(&self.clsid), guid_key(&self.guid))
    }

    fn key(&self) -> String {
        profile_key(&self.clsid, &self.guid, self.langid)
    }
}

/// What an input source is called and what it looks like right now.
struct Entry {
    name: String,
    icon: Option<Vec<u8>>,
    /// The label the system shows in place of an icon.
    label: Option<String>,
}

pub(super) struct Monitor {
    /// Decoded artwork, keyed by the profile that registered it. An icon cannot
    /// change while its registration exists, so decoding it once is enough.
    icons: HashMap<String, Option<Vec<u8>>>,
    /// The last published source, reused only while no window has focus.
    cached: Option<InputSource>,
    /// Declared last so it outlives every COM object created in it.
    _apartment: Option<Apartment>,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, InputSourceError> {
        Ok(Self {
            icons: HashMap::new(),
            cached: None,
            _apartment: Apartment::open(),
        })
    }

    pub(super) fn snapshot(&mut self) -> Result<InputSource, InputSourceError> {
        // A desktop without a foreground window — the lock screen, a UAC prompt,
        // a session switch — hides the previous source instead of replacing it
        // with a guess. Nothing about the user's input state changed, so the last
        // observation stays the truth until a window is focusable again.
        let Some(focus) = focused_window() else {
            return self.cached.clone().ok_or(InputSourceError);
        };
        let Some(layout) = thread_layout(focus) else {
            return Err(InputSourceError);
        };
        let key = layout.0 as usize as u32;
        let langid = (key & 0xffff) as u16;
        // The session's input method describes this window only while both are on
        // the same language: a window pinned to another language keeps that
        // language's layout whatever the session activated.
        let mut active =
            active_profile().filter(|active| active.langid == 0 || active.langid == langid);
        if let Some(active) = active.as_mut().filter(|active| active.langid == 0) {
            active.langid = langid;
        }
        // The identity of the running input method, or of the layout when the
        // session is on a plain keyboard layout. It is read again on every
        // snapshot: an input method switch changes nothing else about the focused
        // window. The conversion mode inside an input method is deliberately not
        // part of it — this release reports the input method, and a 中/英 toggle
        // inside one method is not a change of source.
        let id = match &active {
            Some(active) if active.processor => active.id(),
            _ => format!("{key:08x}"),
        };
        let entry = self.describe(layout, langid, active.as_ref());
        let source = InputSource {
            id,
            name: entry.name,
            icon_rgba: entry.icon,
            label: entry.label,
        };
        self.cached = Some(source.clone());
        Ok(source)
    }

    /// The name and icon of the input source the session is on.
    fn describe(&mut self, layout: HKL, langid: u16, active: Option<&Active>) -> Entry {
        let key = layout.0 as usize as u32;
        if let Some(active) = active.filter(|active| active.processor) {
            let name = profile_name(active)
                .or_else(|| imm_description(layout))
                .or_else(|| language_name(active.langid))
                .unwrap_or_default();
            let icon = self.icon(active);
            return Entry {
                name,
                icon,
                label: None,
            };
        }
        // A plain keyboard layout, or an input method this session reports no
        // profile for. An input method that registered a substitute handle can
        // still name itself through IMM32.
        let name = if is_input_method(key) {
            imm_description(layout).or_else(|| layout_name(key))
        } else {
            layout_name(key)
        }
        .or_else(|| language_name(langid))
        .unwrap_or_default();
        let icon = is_input_method(key)
            .then(|| ime_file_icon(layout))
            .flatten();
        // A layout has no icon of its own: the system draws its three-letter
        // language code there, so that is what is reported.
        let label = icon.is_none().then(|| language_label(langid)).flatten();
        Entry { name, icon, label }
    }

    /// The artwork a profile registered, decoded once per registration.
    fn icon(&mut self, active: &Active) -> Option<Vec<u8>> {
        let key = active.id();
        if let Some(cached) = self.icons.get(&key) {
            return cached.clone();
        }
        let icon = profile_icon(active);
        self.icons.insert(key, icon.clone());
        icon
    }
}

/// The input method the session has activated for the keyboard category.
///
/// The manager is created for this read and dropped before returning. Holding one
/// pins the process's profile state at creation time, which is what makes a
/// long-running process keep reporting the input method it started with; see the
/// module documentation.
fn active_profile() -> Option<Active> {
    // SAFETY: an in-process TSF object, created and released inside this call. No
    // pointer outlives it and no message loop is involved.
    let manager: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }
            .ok()?;
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    // SAFETY: the category is an immortal TSF constant and the destination is a
    // local structure the call fills in.
    let result = unsafe { manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) };
    // Dropping the manager here is what lets the next read see a later switch.
    drop(manager);
    result.ok()?;
    Some(Active {
        clsid: profile.clsid,
        guid: profile.guidProfile,
        langid: profile.langid,
        processor: profile.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR
            && profile.clsid != GUID::zeroed(),
    })
}

/// The COM apartment the TSF objects live in.
///
/// TSF requires a single-threaded apartment, and a plugin thread starts without
/// one. A thread that already belongs to another apartment is left alone: the
/// caller then falls back to the registry instead of tearing down an
/// initialization it does not own.
struct Apartment {
    owned: bool,
}

impl Apartment {
    fn open() -> Option<Self> {
        // SAFETY: balancing `CoInitializeEx` with `CoUninitialize` on the same
        // thread is the documented contract, and `owned` records whether this
        // call is the one that has to balance it.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        result.is_ok().then_some(Self { owned: true })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: this thread still owns the initialization counted above,
            // and every interface created in the apartment is already released.
            unsafe { CoUninitialize() };
        }
    }
}

/// The window that owns keyboard focus, preferring the focused control over the
/// top-level window because an embedded control can run in another thread.
fn focused_window() -> Option<HWND> {
    // SAFETY: read-only USER32 queries. A window handle is only used for the
    // duration of this call; nothing is retained.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return None;
        }
        let thread = GetWindowThreadProcessId(foreground, None);
        if thread == 0 {
            return Some(foreground);
        }
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info).is_ok() {
            if !info.hwndFocus.is_invalid() {
                return Some(info.hwndFocus);
            }
            if !info.hwndActive.is_invalid() {
                return Some(info.hwndActive);
            }
        }
        Some(foreground)
    }
}

fn thread_layout(window: HWND) -> Option<HKL> {
    // SAFETY: read-only USER32 queries. A zero thread id would make
    // `GetKeyboardLayout` answer for the calling thread, which is not the
    // question, so it is rejected instead.
    let layout = unsafe {
        let thread = GetWindowThreadProcessId(window, None);
        if thread == 0 {
            return None;
        }
        GetKeyboardLayout(thread)
    };
    (!layout.0.is_null()).then_some(layout)
}

fn is_input_method(hkl: u32) -> bool {
    hkl >> 28 == IME_LAYOUT_PREFIX
}

/// The input method's own description, which IMM32 input methods register.
fn imm_description(layout: HKL) -> Option<String> {
    let mut buffer = [0u16; 256];
    // SAFETY: IMM32 writes at most the buffer's capacity and reports the count.
    let count = unsafe { ImmGetDescriptionW(layout, Some(&mut buffer)) } as usize;
    let text = wide_text(&buffer[..count.min(buffer.len())]);
    (!text.trim().is_empty()).then_some(text)
}

/// The layout registry entry for a handle: the localized display name the
/// language list shows, then the plain layout text.
fn layout_name(layout: u32) -> Option<String> {
    let key = format!(
        "SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{}",
        klid(layout)
    );
    registry_string(HKEY_LOCAL_MACHINE, &key, "Layout Display Name")
        .map(|name| indirect_string(&name))
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            registry_string(HKEY_LOCAL_MACHINE, &key, "Layout Text")
                .filter(|name| !name.trim().is_empty())
        })
}

/// The registry key name of a layout.
///
/// It is derived from the handle alone: `GetKeyboardLayoutName` reports the
/// calling thread's layout rather than a given handle, so it cannot answer this
/// question. An input method keeps its full substitute handle; an ordinary
/// layout repeats its language in the high word, while the registry key is the
/// bare language id.
fn klid(layout: u32) -> String {
    if is_input_method(layout) || layout >> 16 != layout & 0xffff {
        format!("{layout:08X}")
    } else {
        format!("{:08X}", layout & 0xffff)
    }
}

/// The language a layout belongs to, used when nothing names the input source
/// more precisely than the language it is typed in.
fn language_name(langid: u16) -> Option<String> {
    let mut buffer = [0u16; 128];
    // SAFETY: bounded locale lookup; the returned count includes the terminator.
    let count = unsafe {
        GetLocaleInfoW(
            langid as u32,
            LOCALE_SLOCALIZEDDISPLAYNAME,
            Some(&mut buffer),
        )
    } as usize;
    (count > 1 && count <= buffer.len())
        .then(|| wide_text(&buffer[..count - 1]))
        .filter(|name| !name.trim().is_empty())
}

/// The label the system shows in place of an icon for a keyboard layout: the
/// language's three-letter ISO 639-2 code, upper-cased (`ENG` for English, `DEU`
/// for German, `JPN` for Japanese).
///
/// The system derives its own label from the same value, so this reports the
/// system's label rather than one the plugin made up. A language without such a
/// code reports none and the HUD keeps its name-derived badge.
fn language_label(langid: u16) -> Option<String> {
    let mut buffer = [0u16; 16];
    // SAFETY: bounded locale lookup; the returned count includes the terminator.
    let count = unsafe { GetLocaleInfoW(langid as u32, LOCALE_SISO639LANGNAME2, Some(&mut buffer)) }
        as usize;
    if count <= 1 || count > buffer.len() {
        return None;
    }
    let code = wide_text(&buffer[..count - 1]).to_uppercase();
    (!code.is_empty()
        && code.chars().count() <= 3
        && code.chars().all(|letter| letter.is_ascii_alphabetic()))
    .then_some(code)
}

/// The registry key holding one language profile's own registration.
fn profile_key(clsid: &GUID, guid: &GUID, langid: u16) -> String {
    format!(
        "SOFTWARE\\Microsoft\\CTF\\TIP\\{}\\LanguageProfile\\0x{:08x}\\{}",
        guid_key(clsid),
        langid,
        guid_key(guid)
    )
}

/// The name the profile registered for itself.
fn profile_name(active: &Active) -> Option<String> {
    let key = active.key();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for value in ["Display Description", "Description"] {
            let name = registry_string(root, &key, value)
                .map(|text| indirect_string(&text))
                .filter(|text| !text.trim().is_empty());
            if name.is_some() {
                return name;
            }
        }
    }
    None
}

/// The icon an input method registered with its language profile.
fn profile_icon(active: &Active) -> Option<Vec<u8>> {
    let key = active.key();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Some(file) = registry_string(root, &key, "IconFile") else {
            continue;
        };
        let file = indirect_string(&file);
        if file.trim().is_empty() {
            continue;
        }
        // The index is a signed resource id: an input method that stores its
        // icon by id writes a negative value.
        let index = registry_u32(root, &key, "IconIndex")
            .or_else(|| {
                registry_string(root, &key, "IconIndex").and_then(|value| value.trim().parse().ok())
            })
            .unwrap_or(0) as i32;
        if let Some(icon) = extract_icon(&file, index) {
            return Some(icon);
        }
    }
    None
}

/// The icon of the input method module itself, for methods that ship one but
/// never registered a language profile.
fn ime_file_icon(layout: HKL) -> Option<Vec<u8>> {
    let mut buffer = [0u16; 512];
    // SAFETY: IMM32 writes at most the buffer's capacity and reports the count.
    let count = unsafe { ImmGetIMEFileNameW(layout, Some(&mut buffer)) } as usize;
    let path = wide_text(&buffer[..count.min(buffer.len())]);
    (!path.trim().is_empty())
        .then(|| extract_icon(&path, 0))
        .flatten()
}

/// Pull one icon out of a module and normalize it to the HUD's slot.
fn extract_icon(path: &str, index: i32) -> Option<Vec<u8>> {
    let path = wide(path);
    let mut large = HICON::default();
    // SAFETY: the requested size is a pair of 32s, and the shell writes one
    // owned handle that is destroyed below on every path.
    let result = unsafe {
        SHDefExtractIconW(
            PCWSTR(path.as_ptr()),
            index,
            0,
            Some(&mut large),
            None,
            ICON_SIDE | (ICON_SIDE << 16),
        )
    };
    if result.is_err() || large.is_invalid() {
        return None;
    }
    let pixels = rasterize(large);
    // SAFETY: the handle came from the shell and is not used after this point.
    unsafe {
        let _ = DestroyIcon(large);
    }
    pixels
}

/// Decode an icon into straight RGBA8 at its native size.
///
/// The color bitmap and the AND mask are read separately: an icon drawn before
/// 32-bit alpha existed only has the mask, and a modern icon carries alpha but
/// may still leave the mask populated, so the alpha channel decides whenever it
/// is present.
fn rasterize(icon: HICON) -> Option<Vec<u8>> {
    let mut info = ICONINFO::default();
    // SAFETY: `icon` is a live handle. `GetIconInfo` hands back two bitmaps the
    // caller owns, released by the guard before this function returns.
    unsafe { GetIconInfo(icon, &mut info) }.ok()?;
    let _bitmaps = IconBitmaps {
        color: info.hbmColor,
        mask: info.hbmMask,
    };
    if info.hbmColor.is_invalid() {
        return None;
    }
    let mut bitmap = BITMAP::default();
    // SAFETY: `hbmColor` is live and `BITMAP` is the structure its type expects.
    let read = unsafe {
        GetObjectW(
            HGDIOBJ(info.hbmColor.0),
            size_of::<BITMAP>() as i32,
            Some((&mut bitmap as *mut BITMAP).cast()),
        )
    };
    if read == 0 {
        return None;
    }
    let (width, height) = (bitmap.bmWidth, bitmap.bmHeight);
    // Icons are small; a bitmap outside this range is not one.
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
        return None;
    }
    let (width, height) = (width as u32, height as u32);
    let color = dib_bits(info.hbmColor, width, height, 32)?;
    let mask = dib_bits(info.hbmMask, width, height, 1)?;
    let has_alpha = color.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let offset = ((y * width + x) * 4) as usize;
            let source = &color[offset..offset + 4];
            let alpha = if has_alpha {
                source[3]
            } else if mask_bit(&mask, width, x, y) {
                0
            } else {
                255
            };
            // DIB pixels are blue-first; the HUD wants red-first straight alpha.
            pixels.extend_from_slice(&[source[2], source[1], source[0], alpha]);
        }
    }
    fit_icon(&pixels, width, height)
}

/// Release the bitmaps `GetIconInfo` created, on every path.
struct IconBitmaps {
    color: HBITMAP,
    mask: HBITMAP,
}

impl Drop for IconBitmaps {
    fn drop(&mut self) {
        // SAFETY: both handles were created for this call and are released once.
        unsafe {
            for bitmap in [self.color, self.mask] {
                if !bitmap.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(bitmap.0));
                }
            }
        }
    }
}

/// Read a bitmap into a top-down, tightly packed buffer.
fn dib_bits(bitmap: HBITMAP, width: u32, height: u32, bits: u16) -> Option<Vec<u8>> {
    let stride = ((width as usize * bits as usize).div_ceil(32)) * 4;
    let mut pixels = vec![0u8; stride * height as usize];
    let mut info = DibInfo {
        header: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // A negative height asks for rows in top-down order, which is the
            // order the HUD consumes.
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: bits,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    if bits == 1 {
        info.colors[1] = RGBQUAD {
            rgbRed: 255,
            rgbGreen: 255,
            rgbBlue: 255,
            rgbReserved: 0,
        };
    }
    // SAFETY: the device context is only a scratch handle for `GetDIBits`, and
    // the buffers it writes into match the dimensions described above.
    unsafe {
        let dc = GetDC(None);
        if dc.is_invalid() {
            return None;
        }
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            (&mut info as *mut DibInfo).cast::<BITMAPINFO>(),
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        (lines == height as i32).then_some(pixels)
    }
}

/// `BITMAPINFO` declares a single palette entry; a monochrome read needs two.
#[repr(C)]
struct DibInfo {
    header: BITMAPINFOHEADER,
    colors: [RGBQUAD; 256],
}

impl Default for DibInfo {
    fn default() -> Self {
        Self {
            header: BITMAPINFOHEADER::default(),
            colors: [RGBQUAD::default(); 256],
        }
    }
}

fn mask_bit(mask: &[u8], width: u32, x: u32, y: u32) -> bool {
    let stride = ((width as usize).div_ceil(32)) * 4;
    let offset = y as usize * stride + (x / 8) as usize;
    // An icon mask marks the pixels that keep the background, i.e. the
    // transparent ones.
    mask.get(offset)
        .is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0)
}

/// Fit native icon artwork into the fixed square slot, letterboxed rather than
/// stretched, then reduce or enlarge it with a box filter so every source pixel
/// contributes exactly once.
fn fit_icon(pixels: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || pixels.len() != (width * height * 4) as usize {
        return None;
    }
    if width == ICON_SIDE && height == ICON_SIDE {
        return visible(pixels).then(|| pixels.to_vec());
    }
    let scale = (ICON_SIDE as f64 / width as f64).min(ICON_SIDE as f64 / height as f64);
    let target_width = ((width as f64 * scale).round() as u32).clamp(1, ICON_SIDE);
    let target_height = ((height as f64 * scale).round() as u32).clamp(1, ICON_SIDE);
    let origin_x = (ICON_SIDE - target_width) / 2;
    let origin_y = (ICON_SIDE - target_height) / 2;
    let mut slot = vec![0u8; (ICON_SIDE * ICON_SIDE * 4) as usize];
    for y in 0..target_height {
        let y0 = y * height / target_height;
        let y1 = ((y + 1) * height / target_height).max(y0 + 1);
        for x in 0..target_width {
            let x0 = x * width / target_width;
            let x1 = ((x + 1) * width / target_width).max(x0 + 1);
            let mut alpha_sum = 0u32;
            let mut channels = [0u32; 3];
            let mut count = 0u32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let offset = ((sy * width + sx) * 4) as usize;
                    let alpha = u32::from(pixels[offset + 3]);
                    alpha_sum += alpha;
                    for (channel, value) in channels.iter_mut().enumerate() {
                        *value += u32::from(pixels[offset + channel]) * alpha;
                    }
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let offset = (((origin_y + y) * ICON_SIDE + origin_x + x) * 4) as usize;
            for (channel, value) in channels.iter().enumerate() {
                // Color is averaged weighted by coverage, so a partially
                // transparent edge keeps its hue instead of fading to black.
                slot[offset + channel] = value
                    .checked_div(alpha_sum)
                    .map_or(0, |average| average.min(255) as u8);
            }
            slot[offset + 3] = (alpha_sum / count) as u8;
        }
    }
    visible(&slot).then_some(slot)
}

/// Whether anything would be drawn. A fully transparent icon is not artwork and
/// must not take the slot from the language badge.
fn visible(pixels: &[u8]) -> bool {
    pixels.chunks_exact(4).any(|pixel| pixel[3] != 0)
}

/// The localized name behind an indirect resource reference such as
/// `@%SystemRoot%\system32\input.dll,-5072`, or the string itself.
fn indirect_string(value: &str) -> String {
    let value = expand_environment(value);
    if !value.starts_with('@') {
        return value;
    }
    let source = wide(&value);
    let mut buffer = [0u16; 512];
    // SAFETY: the source is a terminated indirect reference and the buffer is
    // exactly the capacity passed to the API.
    let loaded = unsafe { SHLoadIndirectString(PCWSTR(source.as_ptr()), &mut buffer, None) };
    if loaded.is_err() {
        return value;
    }
    let text = wide_text(&buffer);
    if text.trim().is_empty() { value } else { text }
}

fn expand_environment(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let source = wide(value);
    let mut buffer = [0u16; 1024];
    // SAFETY: the source is terminated and the returned count includes the
    // terminator, which is checked against the capacity before it is used.
    let count =
        unsafe { ExpandEnvironmentStringsW(PCWSTR(source.as_ptr()), Some(&mut buffer)) } as usize;
    if count == 0 || count > buffer.len() {
        return value.to_string();
    }
    wide_text(&buffer[..count - 1])
}

fn registry_string(root: HKEY, path: &str, value: &str) -> Option<String> {
    let path = wide(path);
    let value = wide(value);
    let mut buffer = [0u16; 512];
    let mut size = size_of::<[u16; 512]>() as u32;
    // SAFETY: terminated key and value names, a fixed writable buffer with its
    // byte capacity, and a string filter. No registry handle is opened.
    let result = unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if result.is_err() {
        return None;
    }
    let text = wide_text(&buffer);
    (!text.is_empty()).then_some(text)
}

fn registry_u32(root: HKEY, path: &str, value: &str) -> Option<u32> {
    let path = wide(path);
    let value = wide(value);
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: as above, with a four-byte destination and a DWORD filter.
    let result = unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    };
    result.is_ok().then_some(data)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode a NUL-padded UTF-16 buffer.
fn wide_text(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// The registry key spelling of a GUID: braces and upper-case hex, which is how
/// the language profile catalogue stores its class and profile identifiers.
fn guid_key(guid: &GUID) -> String {
    let value = guid.to_u128();
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        value >> 96,
        (value >> 80) & 0xffff,
        (value >> 64) & 0xffff,
        (value >> 48) & 0xffff,
        value & 0xffff_ffff_ffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_method_handles_are_told_apart_from_plain_layouts() {
        assert!(is_input_method(0xe008_0404));
        assert!(!is_input_method(0x0409_0409));
        assert!(!is_input_method(0x0000_0804));
    }

    #[test]
    fn guid_keys_match_the_catalogue_spelling() {
        let pinyin = GUID::from_u128(0x81d4_e9c9_1d3b_41bc_9e6c_4b40_bf79_e35e);
        assert_eq!(guid_key(&pinyin), "{81D4E9C9-1D3B-41BC-9E6C-4B40BF79E35E}");
        let profile = GUID::from_u128(0xfa55_0b04_5ad7_411f_a5ac_ca03_8ec5_15d7);
        assert_eq!(guid_key(&profile), "{FA550B04-5AD7-411F-A5AC-CA038EC515D7}");
    }

    #[test]
    fn a_profile_key_is_the_registry_path_of_that_registration() {
        let active = Active {
            clsid: GUID::from_u128(0x81d4_e9c9_1d3b_41bc_9e6c_4b40_bf79_e35e),
            guid: GUID::from_u128(0xfa55_0b04_5ad7_411f_a5ac_ca03_8ec5_15d7),
            langid: 0x0804,
            processor: true,
        };

        assert_eq!(
            active.key(),
            "SOFTWARE\\Microsoft\\CTF\\TIP\\{81D4E9C9-1D3B-41BC-9E6C-4B40BF79E35E}\
             \\LanguageProfile\\0x00000804\\{FA550B04-5AD7-411F-A5AC-CA038EC515D7}"
        );
    }

    #[test]
    fn two_input_methods_of_one_language_are_different_identities() {
        // Both run behind the same layout handle, so the handle cannot tell them
        // apart; the profile can, which is what the plugin's id is built from.
        let pinyin = Active {
            clsid: GUID::from_u128(0x81d4_e9c9_1d3b_41bc_9e6c_4b40_bf79_e35e),
            guid: GUID::from_u128(0xfa55_0b04_5ad7_411f_a5ac_ca03_8ec5_15d7),
            langid: 0x0804,
            processor: true,
        };
        let wetype = Active {
            clsid: GUID::from_u128(0x8659_8fb9_66a2_463e_b9c2_aeb9_06d4_77ad),
            guid: GUID::from_u128(0x607f_df85_fcc8_4dbd_a365_4129_6f98_0c9c),
            langid: 0x0804,
            processor: true,
        };

        assert_ne!(pinyin.id(), wetype.id());
    }

    #[test]
    fn a_keyboard_layout_is_not_an_input_method() {
        let layout = Active {
            clsid: GUID::zeroed(),
            guid: GUID::zeroed(),
            langid: 0x0804,
            processor: false,
        };

        assert!(!layout.processor);
    }

    #[test]
    fn a_keyboard_layout_reports_the_label_the_system_shows() {
        // `ENG` beside "English (United States)" is the label the system draws
        // where an input method would have an icon.
        assert_eq!(language_label(0x0409).as_deref(), Some("ENG"));
        assert_eq!(language_label(0x0407).as_deref(), Some("DEU"));
        assert_eq!(language_label(0x040c).as_deref(), Some("FRA"));
        assert_eq!(language_label(0x0411).as_deref(), Some("JPN"));
        assert_eq!(language_label(0x0412).as_deref(), Some("KOR"));
    }

    #[test]
    fn a_layout_has_a_label_instead_of_an_icon() {
        let mut monitor = Monitor::new().expect("monitor");

        // The US layout's handle and language, with no input method active.
        let entry = monitor.describe(
            HKL(0x0409_0409_usize as *mut core::ffi::c_void),
            0x0409,
            None,
        );

        assert!(entry.icon.is_none(), "a layout has no icon of its own");
        assert_eq!(entry.label.as_deref(), Some("ENG"));
        assert!(!entry.name.trim().is_empty());
    }

    #[test]
    fn mask_bits_are_read_from_the_top_left() {
        // One row of 10 pixels: the first and the last are masked out.
        let mut mask = vec![0u8; 4];
        mask[0] = 0b1000_0000;
        mask[1] = 0b0100_0000;

        assert!(mask_bit(&mask, 10, 0, 0));
        assert!(mask_bit(&mask, 10, 9, 0));
        assert!(!mask_bit(&mask, 10, 1, 0));
        assert!(!mask_bit(&mask, 10, 0, 1));
    }

    #[test]
    fn a_native_sized_icon_is_passed_through() {
        let pixels = vec![7u8; 32 * 32 * 4];

        assert_eq!(fit_icon(&pixels, 32, 32).unwrap(), pixels);
    }

    #[test]
    fn oversized_artwork_is_reduced_without_losing_coverage() {
        // A 64x64 icon that is fully opaque red becomes a 32x32 icon that is
        // still fully opaque red: the box filter averages coverage, not color.
        let mut pixels = vec![0u8; 64 * 64 * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[255, 0, 0, 255]);
        }

        let fitted = fit_icon(&pixels, 64, 64).unwrap();

        assert_eq!(fitted.len(), 32 * 32 * 4);
        for pixel in fitted.chunks_exact(4) {
            assert_eq!(pixel, &[255, 0, 0, 255]);
        }
    }

    #[test]
    fn half_covered_pixels_keep_their_hue() {
        // A boundary that splits a source box in half: the edge pixel keeps the
        // full hue at half alpha instead of fading towards black.
        let mut pixels = vec![0u8; 64 * 64 * 4];
        for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            let column = index % 64;
            pixel.copy_from_slice(&[0, 0, 255, if column < 31 { 255 } else { 0 }]);
        }

        let fitted = fit_icon(&pixels, 64, 64).unwrap();
        // Column 15 of the result covers columns 30 and 31 of the source.
        let edge = &fitted[15 * 4..15 * 4 + 4];

        assert_eq!(edge[0], 0);
        assert_eq!(edge[1], 0);
        assert_eq!(edge[2], 255);
        assert_eq!(edge[3], 127);
    }

    #[test]
    fn an_invisible_icon_is_not_an_icon() {
        let pixels = vec![0u8; 32 * 32 * 4];

        assert!(fit_icon(&pixels, 32, 32).is_none());
    }

    #[test]
    fn malformed_buffers_are_rejected() {
        assert!(fit_icon(&[], 0, 0).is_none());
        assert!(fit_icon(&[0; 8], 4, 4).is_none());
    }
}
