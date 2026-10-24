//! The native overlay window and the state hanging off it.
//!
//! Pet mode is transparent to hit-testing and never activated. Window mode
//! retains user32's caption, border and activation behavior. The message handler
//! reaches the state through `GWLP_USERDATA`, so the owner keeps the box alive
//! until after `DestroyWindow`.

use super::*;

pub(crate) const WINDOW_CLASS: windows::core::PCWSTR = w!("BongoCatProductOverlayWindow");

/// The taskbar half of the model window's extended style.
///
/// A tool window owns no taskbar button and `WS_EX_APPWINDOW` forces one, so
/// exactly one of the two is set at a time. Only the model window is toggled
/// this way: the settings window keeps the taskbar button GPUI gives it,
// because a framed window that loses `WS_EX_APPWINDOW` also loses its icon and
/// its minimize and maximize buttons to the tool window's short caption.
pub(crate) const fn taskbar_ex_style(visible: bool) -> WINDOW_EX_STYLE {
    if visible {
        WS_EX_APPWINDOW
    } else {
        WS_EX_TOOLWINDOW
    }
}

pub(crate) struct OverlayWindow {
    pub(crate) hwnd: HWND,
    pub(crate) owner_thread: ThreadId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) _state: Box<OverlayWindowState>,
    pub(crate) _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

pub(crate) struct OverlayWindowState {
    pub(crate) window_mode: bool,
    /// Whether the window is asked to sit in the topmost band. Kept beside the
    /// HWND rather than read from the session so `show` can re-assert it without
    /// the options being plumbed through every caller.
    pub(crate) always_on_top: bool,
    pub(crate) close_requested: bool,
    pub(crate) last_bounds: Option<OverlayWindowBounds>,
    /// Client geometry before the first native sizing step. A caption move
    /// never sets this, even when crossing a display changes the window DPI.
    pub(crate) resize_start_bounds: Option<OverlayWindowBounds>,
    pub(crate) context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    pub(crate) resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    /// The current plugin layer's hit-test data. A plugin that draws a control
    /// claims the click that lands on it; the window keeps every other click.
    pub(crate) plugin_targets: OverlayPluginTargets,
    /// Where a click on a plugin's control is reported.
    pub(crate) plugin_interaction_sender: Option<SyncSender<OverlayPluginInteraction>>,
    pub(crate) sizing: WindowSizing,
    pub(crate) drag: Option<ResizeDrag>,
}

impl OverlayWindow {
    pub(crate) fn create(
        options: OverlaySessionOptions,
        canvas: CanvasInfo,
        bounds: Option<OverlayWindowBounds>,
        context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    ) -> Result<Self, OverlayError> {
        // SAFETY: the class and HWND are created and subsequently used only on
        // the current UI thread. No borrowed Win32 pointers escape this owner.
        unsafe { Self::create_inner(options, canvas, bounds, context_menu_sender, resize_sender) }
            .map_err(windows_error("create Win32 overlay"))
    }

    pub(crate) unsafe fn create_inner(
        options: OverlaySessionOptions,
        canvas: CanvasInfo,
        bounds: Option<OverlayWindowBounds>,
        context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    ) -> WindowsResult<Self> {
        // A saved box that no longer touches any display is only restorable when
        // the placement constraint can pull it back onto one. With the
        // constraint enabled the correction below is what makes such a box
        // usable, so the box is kept as the candidate and clamped; without it
        // there is nothing to recover the window with, and restoring a box that
        // no longer intersects a display would leave an unreachable window, so
        // the window falls back to the cursor's display instead.
        let bounds =
            bounds.filter(|bounds| options.keep_inside_screen || overlay_bounds_visible(*bounds));
        let sizing = WindowSizing::new(canvas)
            .ok_or_else(|| invariant_error("model canvas has an invalid aspect ratio"))?;
        let bounds = bounds.map(|bounds| sizing.normalize(bounds));
        let module = unsafe { GetModuleHandleW(None)? };
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        // A window class is a process-wide resource, so this only ever
        // registers: a class that is already there is the state an earlier
        // creation left behind, and nothing here removes it. `UnregisterClassW`
        // only succeeds once no window of the class exists, and a destruction on
        // another thread can land between this call and `CreateWindowExW`, which
        // then fails with `ERROR_CLASS_DOES_NOT_EXIST`. The swap path leans on
        // the same rule: a replacement window is created before the window it
        // replaces is dropped, so the class has to outlive both.
        if unsafe { RegisterClassW(&class) } == 0 {
            let error = Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
        let mut extended = if options.window_mode {
            WS_EX_APPWINDOW
        } else {
            taskbar_ex_style(options.taskbar_icon_visible)
                | WS_EX_NOACTIVATE
                | WS_EX_NOREDIRECTIONBITMAP
        };
        if options.click_through && !options.window_mode {
            // `WS_EX_TRANSPARENT` alone leaves a DirectComposition-backed
            // top-level window on the desktop input path: `WM_NCHITTEST`
            // returns `HTTRANSPARENT`, but a real click still selects this
            // HWND. Layering is the Win32 pair that makes the whole window
            // pass through to the window underneath it.
            extended |= WS_EX_TRANSPARENT | WS_EX_LAYERED;
        }
        let scale = options.scale_percent;
        let (logical_width, logical_height) = sizing.dimensions_for_scale(96, scale);
        let cursor = current_cursor_position();
        let initial_x = bounds.map_or(cursor.x, |value| value.x);
        let initial_y = bounds.map_or(cursor.y, |value| value.y);
        let mut state = Box::new(OverlayWindowState {
            window_mode: options.window_mode,
            always_on_top: options.always_on_top,
            close_requested: false,
            last_bounds: None,
            resize_start_bounds: None,
            context_menu_sender,
            resize_sender,
            plugin_targets: OverlayPluginTargets::default(),
            plugin_interaction_sender: None,
            sizing,
            drag: None,
        });
        let hwnd = unsafe {
            CreateWindowExW(
                extended,
                WINDOW_CLASS,
                w!("BongoCat"),
                if options.window_mode {
                    WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX
                } else {
                    WS_POPUP
                },
                initial_x,
                initial_y,
                logical_width as i32,
                logical_height as i32,
                None,
                None,
                Some(instance),
                Some((&mut *state as *mut OverlayWindowState).cast()),
            )
        }?;
        let mut window = Self {
            hwnd,
            owner_thread: thread::current().id(),
            width: logical_width,
            height: logical_height,
            _state: state,
            _not_send_or_sync: std::marker::PhantomData,
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            return Err(invariant_error("GetDpiForWindow returned zero"));
        }
        let (width, height) = bounds.map_or_else(
            || sizing.dimensions_for_scale(dpi, scale),
            |value| (value.width, value.height),
        );
        let (x, y) = bounds.map_or_else(
            || centered_position(cursor, width, height),
            |value| (value.x, value.y),
        );
        let bounds = OverlayWindowBounds::new(x, y, width, height);
        // A brand new window has no drag to interrupt, so an unusable placement
        // is corrected immediately: this is the restore path for a saved box and
        // the centering path for a window that has never been placed.
        let bounds = if options.keep_inside_screen {
            let screens = screen_bounds_all();
            if bounds_inside_screens(&screens, bounds) {
                bounds
            } else {
                correction_for_screens(&screens, bounds).unwrap_or(bounds)
            }
        } else {
            bounds
        };
        let rect = unsafe { window_frame_rect(hwnd, width, height)? };
        unsafe {
            SetWindowPos(
                hwnd,
                if options.always_on_top {
                    Some(HWND_TOPMOST)
                } else {
                    Some(HWND_NOTOPMOST)
                },
                bounds.x + rect.left,
                bounds.y + rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE,
            )?;
        }
        window.width = width;
        window.height = height;
        Ok(window)
    }

    pub(crate) fn show(&self) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        if unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            return Ok(());
        }
        // SAFETY: the HWND is live, owned, and accessed only on its creation
        // thread; showing without activation does not transfer ownership.
        let _ = unsafe {
            ShowWindow(
                self.hwnd,
                if IsIconic(self.hwnd).as_bool() {
                    windows::Win32::UI::WindowsAndMessaging::SW_RESTORE
                } else {
                    SW_SHOWNOACTIVATE
                },
            )
        };
        if !unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            return Err(OverlayError::new("Win32 overlay did not become visible"));
        }
        // `ShowWindow` brings a window back at the top of the band it belongs to,
        // but it does not put a window back into the topmost band: a window that
        // lost the band while it was hidden would come back as an ordinary
        // window. Re-asserting here covers the show path; the frame tick covers
        // a demotion that happens while the window stays visible.
        if self._state.always_on_top {
            self.ensure_topmost()?;
        }
        Ok(())
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and accessed only from its owner thread.
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    pub(crate) fn assert_owner_thread(&self) {
        assert_eq!(self.owner_thread, thread::current().id());
    }

    /// Whether a right-button resize drag is in progress on this window.
    pub(crate) fn is_resize_dragging(&self) -> bool {
        self.assert_owner_thread();
        // SAFETY: userdata is either null before WM_NCCREATE or the live boxed
        // state this window owns, and it is read on the owner thread.
        let state =
            unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *const OverlayWindowState };
        !state.is_null() && unsafe { (*state).drag.is_some() }
    }

    pub(crate) fn bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        self.assert_owner_thread();
        // SAFETY: the HWND and boxed state belong to this thread. Minimized
        // rectangles are shell coordinates, never persisted model geometry.
        if unsafe { IsIconic(self.hwnd) }.as_bool()
            && let Some(bounds) = self._state.last_bounds
        {
            return Ok(bounds);
        }
        unsafe { client_window_bounds(self.hwnd) }
    }

    pub(crate) fn bounds_for_scale(
        &self,
        scale_percent: u16,
    ) -> Result<OverlayWindowBounds, OverlayError> {
        self.assert_owner_thread();
        // SAFETY: this HWND is live and read on its owner thread.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        if dpi == 0 {
            return Err(OverlayError::new("GetDpiForWindow returned zero"));
        }
        let (width, height) = self._state.sizing.dimensions_for_scale(dpi, scale_percent);
        Ok(OverlayWindowBounds {
            width,
            height,
            ..self.bounds()?
        })
    }

    /// Move the window to a corrected box without touching its size, z-order or
    /// activation. The caller has already compared the box against the live
    /// window rectangle, so an unchanged origin is a no-op.
    pub(crate) fn set_origin(&self, bounds: OverlayWindowBounds) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and confined to its owner thread. This only
        // corrects its origin while preserving size, z-order, and activation.
        unsafe {
            let rect = window_frame_rect(self.hwnd, bounds.width, bounds.height)
                .map_err(windows_error("measure window frame"))?;
            SetWindowPos(
                self.hwnd,
                None,
                bounds.x + rect.left,
                bounds.y + rect.top,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            )
            .map_err(windows_error("keep the overlay on a display"))?;
        }
        Ok(())
    }

    /// Resize and move the existing HWND without replacing its renderer.
    ///
    /// Scale changes are presentation geometry, not a new model generation.
    /// Keeping the HWND alive avoids exposing a freshly-created DirectComposition
    /// surface before its first compositor tick has settled.
    pub(crate) fn resize(&mut self, bounds: OverlayWindowBounds) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        let bounds = self._state.sizing.normalize(bounds.validate()?);
        if self.bounds()? == bounds {
            return Ok(());
        }
        // SAFETY: the HWND is live and confined to its owner thread. The caller
        // supplies a validated virtual-screen box, and the operation preserves
        // z-order and activation while changing only geometry.
        unsafe {
            let rect = window_frame_rect(self.hwnd, bounds.width, bounds.height)
                .map_err(windows_error("measure window frame"))?;
            if IsIconic(self.hwnd).as_bool() {
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowPlacement, SetWindowPlacement, WINDOWPLACEMENT,
                };
                let mut placement = WINDOWPLACEMENT {
                    length: size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                GetWindowPlacement(self.hwnd, &mut placement)
                    .map_err(windows_error("read minimized window placement"))?;
                // WINDOWPLACEMENT uses work-area coordinates. Preserve the
                // normal origin; change only its extent while minimized.
                placement.rcNormalPosition.right =
                    placement.rcNormalPosition.left + rect.right - rect.left;
                placement.rcNormalPosition.bottom =
                    placement.rcNormalPosition.top + rect.bottom - rect.top;
                SetWindowPlacement(self.hwnd, &placement)
                    .map_err(windows_error("resize minimized model window"))?;
                self._state.last_bounds = Some(bounds);
                self.width = bounds.width;
                self.height = bounds.height;
                return Ok(());
            }
            SetWindowPos(
                self.hwnd,
                None,
                bounds.x + rect.left,
                bounds.y + rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(windows_error("resize the existing overlay window"))?;
        }
        self.width = bounds.width;
        self.height = bounds.height;
        Ok(())
    }

    pub(crate) fn set_always_on_top(&mut self, always_on_top: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        self.apply_z_order(if always_on_top {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        })?;
        self._state.always_on_top = always_on_top;
        Ok(())
    }

    /// Whether the window is currently a member of the topmost band.
    pub(crate) fn is_topmost(&self) -> bool {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and confined to its owner thread, and
        // `GWL_EXSTYLE` only reads this window's own extended style bits.
        let style = WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32);
        style.contains(WS_EX_TOPMOST)
    }

    /// Whether the window is asked to sit in the topmost band.
    pub(crate) fn is_always_on_top(&self) -> bool {
        self._state.always_on_top
    }

    /// Restore both topmost membership and ordering above the foreground app.
    pub(crate) fn ensure_topmost(&self) -> Result<(), OverlayError> {
        // SAFETY: GetForegroundWindow only reads the desktop's current foreground HWND.
        self.ensure_topmost_above(unsafe { GetForegroundWindow() })
    }

    pub(super) fn ensure_topmost_above(&self, foreground: HWND) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        if !self.is_always_on_top() {
            return Ok(());
        }
        // A fullscreen browser can enter the topmost band without clearing our
        // WS_EX_TOPMOST bit. Band membership alone says nothing about their order.
        if !self.is_topmost() || window_is_above(foreground, self.hwnd) {
            self.apply_z_order(HWND_TOPMOST)?;
        }
        Ok(())
    }

    /// Move the window to the top or back out of the topmost band.
    ///
    /// Geometry and activation are suppressed: this is the only z-order
    /// transition the product makes, and it must never move, resize or focus the
    /// overlay as a side effect.
    fn apply_z_order(&self, insert_after: HWND) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and confined to its owner thread. The
        // position arguments are all suppressed, so the call only changes the
        // window's place in the Z order.
        unsafe {
            SetWindowPos(
                self.hwnd,
                Some(insert_after),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
            .map_err(windows_error("update overlay z-order"))?;
        }
        Ok(())
    }

    /// Whether the taskbar currently shows a button for this window.
    pub(crate) fn taskbar_icon_is_visible(&self) -> bool {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and confined to its owner thread, and
        // `GWL_EXSTYLE` only reads this window's own extended style bits.
        let style = WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32);
        style.contains(WS_EX_APPWINDOW) && !style.contains(WS_EX_TOOLWINDOW)
    }

    /// Show or hide this window's taskbar button without replacing it.
    ///
    /// The style pair is the same one `set_click_through` writes, and
    /// `SWP_FRAMECHANGED` is what makes the shell observe it. The read-back
    /// keeps a style the system refused from being reported as applied, the
    /// same way the click-through path is read back by its own caller.
    pub(crate) fn set_taskbar_icon_visible(&self, visible: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        let visible = visible || self._state.window_mode;
        if self.taskbar_icon_is_visible() == visible {
            return Ok(());
        }
        // SAFETY: the HWND is live and confined to its owner thread. The write
        // touches only the two taskbar bits of this window's extended style,
        // and the position call refreshes the cached non-client state without
        // moving, resizing or restacking the window.
        unsafe {
            let taskbar_style = taskbar_ex_style(visible).0 as isize;
            let current = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let next = if visible {
                (current | taskbar_style) & !WS_EX_TOOLWINDOW.0 as isize
            } else {
                (current | taskbar_style) & !WS_EX_APPWINDOW.0 as isize
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, next);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            )
            .map_err(windows_error("update overlay taskbar button"))?;
        }
        if self.taskbar_icon_is_visible() == visible {
            Ok(())
        } else {
            Err(OverlayError::new(
                "Win32 overlay taskbar button did not follow the extended style",
            ))
        }
    }

    pub(crate) fn set_click_through(&self, click_through: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        let click_through = click_through && !self._state.window_mode;
        // SAFETY: the HWND is live and confined to its owner thread. The
        // extended style controls whether the desktop input path can select this
        // window; `SWP_FRAMECHANGED` refreshes the cached non-client state
        // after the style update.
        unsafe {
            let click_through_style = WS_EX_TRANSPARENT.0 as isize | WS_EX_LAYERED.0 as isize;
            let mut style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            if click_through {
                style |= click_through_style;
            } else {
                style &= !click_through_style;
            }
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style);
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            )
            .map_err(windows_error("update overlay click-through"))?;
        }
        Ok(())
    }
}

pub(super) fn window_is_above(candidate: HWND, model: HWND) -> bool {
    if candidate.is_invalid() || candidate == model {
        return false;
    }
    // Ordinary foreground windows cannot cover a topmost model. This also
    // avoids walking the desktop order on the usual, unchanged frame.
    // SAFETY: the external HWND is only queried, never dereferenced or retained.
    let style = WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(candidate, GWL_EXSTYLE) } as u32);
    if !style.contains(WS_EX_TOPMOST) {
        return false;
    }
    let mut order = (candidate, model, false);
    // SAFETY: EnumWindows invokes the callback synchronously on this thread;
    // the stack context remains valid until it returns. Early stop is intentional.
    let _ = unsafe {
        EnumWindows(
            Some(compare_window_order),
            LPARAM(std::ptr::from_mut(&mut order) as isize),
        )
    };
    order.2
}

unsafe extern "system" fn compare_window_order(hwnd: HWND, context: LPARAM) -> BOOL {
    // SAFETY: only window_is_above passes this live, exclusive stack context.
    let order = unsafe { &mut *(context.0 as *mut (HWND, HWND, bool)) };
    if hwnd == order.0 {
        order.2 = true;
        BOOL(0)
    } else {
        BOOL(i32::from(hwnd != order.1))
    }
}

/// Compute the outer rectangle for client pixels using the HWND's current DPI.
pub(crate) unsafe fn window_frame_rect(hwnd: HWND, width: u32, height: u32) -> WindowsResult<RECT> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    // SAFETY: callers own the live HWND on this thread; rect is writable storage.
    unsafe {
        AdjustWindowRectExForDpi(
            &mut rect,
            WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32),
            false,
            WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32),
            GetDpiForWindow(hwnd),
        )?;
    }
    Ok(rect)
}

pub(crate) unsafe fn client_window_bounds(hwnd: HWND) -> Result<OverlayWindowBounds, OverlayError> {
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: callers own the HWND on its thread; both outputs are stack storage.
    unsafe {
        GetClientRect(hwnd, &mut rect).map_err(windows_error("read model client size"))?;
        ClientToScreen(hwnd, &mut origin)
            .ok()
            .map_err(windows_error("read model client origin"))?;
    }
    OverlayWindowBounds::new(origin.x, origin.y, rect.right as u32, rect.bottom as u32).validate()
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.assert_owner_thread();
        // SAFETY: Renderer has already dropped, so no composition target still
        // uses this HWND. The class is deliberately left registered: it belongs
        // to the process rather than to this window, and removing it here could
        // take it away from a window another thread is about to create.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
