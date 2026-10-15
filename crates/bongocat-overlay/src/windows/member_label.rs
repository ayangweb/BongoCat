//! UI-thread owned Win32 labels. No raw HWND or font escapes the platform adapter.
use super::*;
use ::windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DEFAULT_GUI_FONT, DeleteObject, GetMonitorInfoW, GetObjectW,
    GetStockObject, HFONT, HGDIOBJ, LOGFONTW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint,
};
use ::windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SendMessageW, SetWindowPos, ShowWindow, WINDOW_STYLE, WM_SETFONT, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

pub(crate) struct MemberLabel {
    hwnd: HWND,
    owner: HWND,
    font: HFONT,
    dpi: u32,
}
impl MemberLabel {
    pub(crate) fn create(owner: HWND, id: u64) -> Result<Self, OverlayError> {
        let text: Vec<u16> = format!("ID {id}").encode_utf16().chain(Some(0)).collect();
        // SAFETY: owner is a live UI-thread HWND. CreateWindowExW copies text;
        // the returned owned popup is destroyed before the owner's GPU/window.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                ::windows::core::PCWSTR(text.as_ptr()),
                WINDOW_STYLE(WS_POPUP.0 | 1),
                0,
                0,
                1,
                1,
                Some(owner),
                None,
                None,
                None,
            )
        }
        .map_err(windows_error("create room member label"))?;
        Ok(Self {
            hwnd,
            owner,
            font: HFONT::default(),
            dpi: 0,
        })
    }
    pub(crate) fn sync(
        &mut self,
        bounds: OverlayWindowBounds,
        visible: bool,
    ) -> Result<(), OverlayError> {
        // SAFETY: all handles are live on their owning thread. The stock font
        // is borrowed; LOGFONTW is initialized writable stack memory. The custom
        // font is replaced only after WM_SETFONT releases its previous use.
        unsafe {
            let dpi = GetDpiForWindow(self.owner).max(96);
            if dpi != self.dpi {
                let mut font = LOGFONTW::default();
                if GetObjectW(
                    GetStockObject(DEFAULT_GUI_FONT),
                    size_of::<LOGFONTW>() as i32,
                    Some((&mut font as *mut LOGFONTW).cast()),
                ) == 0
                {
                    return Err(OverlayError::new("read system label font failed"));
                }
                font.lfHeight = font.lfHeight * dpi as i32 / 96;
                let replacement = CreateFontIndirectW(&font);
                if replacement.is_invalid() {
                    return Err(OverlayError::new("create label font failed"));
                }
                SendMessageW(
                    self.hwnd,
                    WM_SETFONT,
                    Some(WPARAM(replacement.0 as usize)),
                    Some(LPARAM(1)),
                );
                if !self.font.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(self.font.0));
                }
                self.font = replacement;
                self.dpi = dpi;
            }
            let height = (24 * dpi / 96) as i32;
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                bounds.x,
                bounds.y + bounds.height as i32,
                bounds.width as i32,
                height,
                SWP_NOACTIVATE,
            )
            .map_err(windows_error("place room member label"))?;
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        Ok(())
    }
}
impl Drop for MemberLabel {
    fn drop(&mut self) {
        // SAFETY: the session keeps this owner on the UI thread and drops it
        // before the cat window. No window can reference the font afterwards.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            if !self.font.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.font.0));
            }
        }
    }
}

pub(crate) fn initial_bounds(index: usize) -> OverlayWindowBounds {
    // SAFETY: monitor queries use initialized stack memory; no handle is retained.
    let (work, dpi) = unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 800,
                bottom: 600,
            }
        };
        (work, GetDpiForSystem().max(96))
    };
    let width = (200 * dpi / 96).min((work.right - work.left).max(100) as u32);
    let gap = 16 * dpi / 96;
    let row_height = 180 * dpi / 96;
    let columns = ((work.right - work.left).max(1) as u32 / (width + gap)).max(1);
    let index = index.min(6) as u32;
    OverlayWindowBounds {
        x: work.left + (gap / 2 + (index % columns) * (width + gap)) as i32,
        y: (work.bottom - ((index / columns + 1) * row_height) as i32).max(work.top),
        width,
        height: width * 2 / 3,
    }
}
