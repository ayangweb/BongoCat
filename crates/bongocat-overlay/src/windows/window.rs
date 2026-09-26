//! The native overlay window and the state hanging off it.
//!
//! The window is layered, transparent to hit-testing and never activated, so it
//! can sit over whatever the user is doing without taking a click or a focus
//! ring. Its message handler reaches the state through `GWLP_USERDATA`, which is
//! why the owner keeps the box alive until after `DestroyWindow`.

use super::*;

pub(crate) const WINDOW_CLASS: windows::core::PCWSTR = w!("BongoCatProductOverlayWindow");

pub(crate) struct OverlayWindow {
    pub(crate) hwnd: HWND,
    pub(crate) instance: HINSTANCE,
    pub(crate) owner_thread: ThreadId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) _state: Box<OverlayWindowState>,
    pub(crate) _not_send_or_sync: std::marker::PhantomData<Rc<()>>,
}

pub(crate) struct OverlayWindowState {
    pub(crate) context_menu_sender: Option<SyncSender<OverlayContextMenuRequest>>,
    pub(crate) resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    /// The logical size `100%` maps to, which is the size the window would be
    /// created with for the current model. It is converted to the window's
    /// physical pixels when a drag begins, so a window that moved to a display
    /// with a different DPI still scales from the right base.
    pub(crate) resize_base_logical: (u32, u32),
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
        let module = unsafe { GetModuleHandleW(None)? };
        let instance = HINSTANCE(module.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            let error = Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
        let mut extended = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP;
        if options.click_through {
            // `WS_EX_TRANSPARENT` alone leaves a DirectComposition-backed
            // top-level window on the desktop input path: `WM_NCHITTEST`
            // returns `HTTRANSPARENT`, but a real click still selects this
            // HWND. Layering is the Win32 pair that makes the whole window
            // pass through to the window underneath it.
            extended |= WS_EX_TRANSPARENT | WS_EX_LAYERED;
        }
        let scale = options.scale_percent;
        let (base_width, base_height) = default_overlay_window_dimensions(canvas);
        let (logical_width, logical_height) = model_window_dimensions(canvas, scale);
        let cursor = current_cursor_position();
        let initial_x = bounds.map_or(cursor.x, |value| value.x);
        let initial_y = bounds.map_or(cursor.y, |value| value.y);
        let mut state = Box::new(OverlayWindowState {
            context_menu_sender,
            resize_sender,
            resize_base_logical: (base_width, base_height),
            drag: None,
        });
        let hwnd = match unsafe {
            CreateWindowExW(
                extended,
                WINDOW_CLASS,
                w!("BongoCat"),
                WS_POPUP,
                initial_x,
                initial_y,
                logical_width as i32,
                logical_height as i32,
                None,
                None,
                Some(instance),
                Some((&mut *state as *mut OverlayWindowState).cast()),
            )
        } {
            Ok(hwnd) => hwnd,
            Err(error) => {
                let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
                return Err(error);
            }
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            let _ = unsafe { DestroyWindow(hwnd) };
            let _ = unsafe { UnregisterClassW(WINDOW_CLASS, Some(instance)) };
            return Err(invariant_error("GetDpiForWindow returned zero"));
        }
        let width = bounds.map_or(logical_to_physical(logical_width, dpi)?, |value| {
            value.width
        });
        let height = bounds.map_or(logical_to_physical(logical_height, dpi)?, |value| {
            value.height
        });
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
        unsafe {
            SetWindowPos(
                hwnd,
                if options.always_on_top {
                    Some(HWND_TOPMOST)
                } else {
                    Some(HWND_NOTOPMOST)
                },
                bounds.x,
                bounds.y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE,
            )?;
        }
        Ok(Self {
            hwnd,
            instance,
            owner_thread: thread::current().id(),
            width,
            height,
            _state: state,
            _not_send_or_sync: std::marker::PhantomData,
        })
    }

    pub(crate) fn show(&self) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        if unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            return Ok(());
        }
        // SAFETY: the HWND is live, owned, and accessed only on its creation
        // thread; showing without activation does not transfer ownership.
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        if !unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            return Err(OverlayError::new("Win32 overlay did not become visible"));
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
        let mut rect = RECT::default();
        // SAFETY: the HWND is live and accessed only from its owner thread.
        unsafe { GetWindowRect(self.hwnd, &mut rect) }
            .map_err(windows_error("read overlay window position"))?;
        OverlayWindowBounds::new(
            rect.left,
            rect.top,
            (rect.right - rect.left) as u32,
            (rect.bottom - rect.top) as u32,
        )
        .validate()
    }

    /// Move the window to a corrected box without touching its size, z-order or
    /// activation. The caller has already compared the box against the live
    /// window rectangle, so an unchanged origin is a no-op.
    pub(crate) fn set_origin(&self, bounds: OverlayWindowBounds) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and confined to its owner thread. This only
        // corrects its origin while preserving size, z-order, and activation.
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                bounds.x,
                bounds.y,
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
        let bounds = bounds.validate()?;
        if self.bounds()? == bounds {
            return Ok(());
        }
        // SAFETY: the HWND is live and confined to its owner thread. The caller
        // supplies a validated virtual-screen box, and the operation preserves
        // z-order and activation while changing only geometry.
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                bounds.x,
                bounds.y,
                bounds.width as i32,
                bounds.height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(windows_error("resize the existing overlay window"))?;
        }
        self.width = bounds.width;
        self.height = bounds.height;
        Ok(())
    }

    pub(crate) fn set_always_on_top(&self, always_on_top: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
        // SAFETY: the HWND is live and confined to its owner thread. This is
        // the only in-place z-order transition for the overlay.
        unsafe {
            SetWindowPos(
                self.hwnd,
                if always_on_top {
                    Some(HWND_TOPMOST)
                } else {
                    Some(HWND_NOTOPMOST)
                },
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

    pub(crate) fn set_click_through(&self, click_through: bool) -> Result<(), OverlayError> {
        self.assert_owner_thread();
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

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.assert_owner_thread();
        // SAFETY: Renderer has already dropped, so no composition target still
        // uses this HWND. Class unregistration follows window destruction.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(WINDOW_CLASS, Some(self.instance));
        }
    }
}
