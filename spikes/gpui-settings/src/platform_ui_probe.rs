use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(target_os = "macos")]
use objc2::{
    class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::AnyObject,
};
#[cfg(target_os = "macos")]
use objc2_foundation::NSPoint;
#[cfg(target_os = "macos")]
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

#[cfg(target_os = "macos")]
type CFAbsoluteTime = f64;
#[cfg(target_os = "macos")]
type CFIndex = isize;
#[cfg(target_os = "macos")]
type CFAllocatorRef = *const c_void;
#[cfg(target_os = "macos")]
type CFRunLoopRef = *mut c_void;
#[cfg(target_os = "macos")]
type CFRunLoopTimerRef = *mut c_void;
#[cfg(target_os = "macos")]
type CFStringRef = *const c_void;

#[cfg(target_os = "macos")]
#[repr(C)]
struct CFRunLoopTimerContext {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    copy_description: Option<unsafe extern "C" fn(*const c_void) -> CFStringRef>,
}

#[cfg(target_os = "macos")]
struct ScheduledMouseMove {
    view: Retained<AnyObject>,
    x: i16,
    y: i16,
    content_height: i16,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;

    fn CFAbsoluteTimeGetCurrent() -> CFAbsoluteTime;
    fn CFRunLoopGetMain() -> CFRunLoopRef;
    fn CFRunLoopTimerCreate(
        allocator: CFAllocatorRef,
        fire_date: CFAbsoluteTime,
        interval: f64,
        flags: u64,
        order: CFIndex,
        callout: unsafe extern "C" fn(CFRunLoopTimerRef, *mut c_void),
        context: *mut CFRunLoopTimerContext,
    ) -> CFRunLoopTimerRef;
    fn CFRunLoopAddTimer(run_loop: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
    fn CFRunLoopTimerInvalidate(timer: CFRunLoopTimerRef);
    fn CFRelease(object: *const c_void);
}

#[cfg(target_os = "macos")]
unsafe fn send_mouse_move(action: &ScheduledMouseMove) -> Result<(), String> {
    // SAFETY: GPUI owns the NSView and its NSWindow while the settings window
    // is live. This function executes on the AppKit main run loop.
    let native_window: *mut AnyObject = unsafe { msg_send![&*action.view, window] };
    if native_window.is_null() {
        return Err("GPUI NSView was not attached to an NSWindow".into());
    }
    // SAFETY: native_window is a live NSWindow.
    let window_number: isize = unsafe { msg_send![native_window, windowNumber] };
    // SAFETY: every argument uses the Objective-C ABI type declared by
    // NSEvent's mouse-event initializer.
    let event: *mut AnyObject = unsafe {
        msg_send![class!(NSEvent),
            mouseEventWithType: 5usize
            location: NSPoint::new(
                f64::from(action.x),
                f64::from(action.content_height - action.y),
            )
            modifierFlags: 0usize
            timestamp: 0.0f64
            windowNumber: window_number
            context: core::ptr::null_mut::<AnyObject>()
            eventNumber: 0isize
            clickCount: 0isize
            pressure: 0.0f32
        ]
    };
    if event.is_null() {
        return Err("NSEvent mouseEventWithType returned nil".into());
    }
    // SAFETY: GPUI installs mouseMoved: on this live content-view subclass.
    // Calling the standard AppKit selector exercises GPUI's native event
    // conversion without NSApplication re-routing by the physical cursor.
    let _: () = unsafe { msg_send![&*action.view, mouseMoved: event] };
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn release_scheduled_mouse_move(context: *const c_void) {
    if !context.is_null() {
        // SAFETY: post_mouse_move transfers exactly one Box allocation to the
        // timer context, and Core Foundation calls release once.
        drop(unsafe { Box::from_raw(context.cast_mut().cast::<ScheduledMouseMove>()) });
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn send_scheduled_mouse_move(timer: CFRunLoopTimerRef, context: *mut c_void) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if context.is_null() {
            return Err("mouse-move dispatch context was null".to_string());
        }
        // SAFETY: the timer context owns this action until invalidation below.
        let action = unsafe { &*context.cast::<ScheduledMouseMove>() };
        autoreleasepool(|_| {
            // SAFETY: the callback runs on the main run loop while the GPUI window
            // owner remains alive for the probe duration.
            unsafe { send_mouse_move(action) }
        })
    }));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("gpui-settings-spike: UI probe failed: {error}"),
        Err(_) => eprintln!("gpui-settings-spike: UI probe callback panicked"),
    }
    // SAFETY: timer is the live one-shot timer invoking this callback. Its
    // context release callback owns cleanup of ScheduledMouseMove.
    unsafe { CFRunLoopTimerInvalidate(timer) };
}

#[cfg(target_os = "macos")]
pub fn post_mouse_move(window: &Window, x: i16, y: i16, content_height: i16) -> Result<(), String> {
    let raw = HasWindowHandle::window_handle(window)
        .map_err(|error| format!("read GPUI raw window handle: {error}"))?
        .as_raw();
    let view = match raw {
        RawWindowHandle::AppKit(handle) => {
            let view = handle.ns_view.as_ptr().cast::<AnyObject>();
            // SAFETY: GPUI owns a valid NSView for the live Window. Retaining
            // it keeps the Objective-C object alive until the timer releases
            // its context, without extending the GPUI Window owner itself.
            unsafe { Retained::retain(view) }.ok_or("retain GPUI NSView returned nil")?
        }
        other => return Err(format!("expected AppKit window handle, found {other:?}")),
    };

    let action = Box::new(ScheduledMouseMove {
        view,
        x,
        y,
        content_height,
    });
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: Box::into_raw(action).cast(),
        retain: None,
        release: Some(release_scheduled_mouse_move),
        copy_description: None,
    };
    // SAFETY: the current callback runs on GPUI's AppKit main thread. The
    // one-shot timer copies the context and invokes its release callback.
    unsafe {
        let timer = CFRunLoopTimerCreate(
            ptr::null(),
            CFAbsoluteTimeGetCurrent() + 0.01,
            0.0,
            0,
            0,
            send_scheduled_mouse_move,
            &mut context,
        );
        if timer.is_null() {
            release_scheduled_mouse_move(context.info);
            return Err("CFRunLoopTimerCreate returned null".into());
        }
        let run_loop = CFRunLoopGetMain();
        if run_loop.is_null() {
            CFRelease(timer.cast());
            return Err("CFRunLoopGetMain returned null".into());
        }
        CFRunLoopAddTimer(run_loop, timer, kCFRunLoopCommonModes);
        CFRelease(timer.cast());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn PostMessageW(
        window: *mut core::ffi::c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> i32;
}

#[cfg(target_os = "windows")]
pub fn post_mouse_move(
    window: &Window,
    x: i16,
    y: i16,
    _content_height: i16,
) -> Result<(), String> {
    const WM_MOUSEMOVE: u32 = 0x0200;

    let raw = HasWindowHandle::window_handle(window)
        .map_err(|error| format!("read GPUI raw window handle: {error}"))?
        .as_raw();
    let hwnd = match raw {
        RawWindowHandle::Win32(handle) => handle.hwnd.get() as *mut core::ffi::c_void,
        other => return Err(format!("expected Win32 window handle, found {other:?}")),
    };
    let packed_coordinates = (i32::from(y as u16) << 16) | i32::from(x as u16);
    // Windows product targets are x64/ARM64, so widening the Win32 signed
    // coordinate payload to LPARAM preserves all 32 packed bits.
    let lparam = packed_coordinates as isize;
    // SAFETY: hwnd is owned by the live GPUI Window, WM_MOUSEMOVE takes no
    // pointer payload, and LPARAM contains bounded client coordinates.
    if unsafe { PostMessageW(hwnd, WM_MOUSEMOVE, 0, lparam) } == 0 {
        return Err("PostMessageW(WM_MOUSEMOVE) failed".into());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn post_mouse_move(
    _window: &Window,
    _x: i16,
    _y: i16,
    _content_height: i16,
) -> Result<(), String> {
    Err("tooltip platform probe supports only macOS and Windows".into())
}
