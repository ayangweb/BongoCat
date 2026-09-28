use objc2::{class, msg_send, runtime::AnyObject};
use std::{
    ffi::{CStr, c_void},
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    time::Duration,
};

type CFAbsoluteTime = f64;
type CFTimeInterval = f64;
type CFIndex = isize;
type CFOptionFlags = u64;
type CFAllocatorRef = *const c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopTimerRef = *mut c_void;
type CFStringRef = *const c_void;

#[repr(C)]
struct CFRunLoopTimerContext {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    copy_description: Option<unsafe extern "C" fn(*const c_void) -> CFStringRef>,
}

struct ScheduledMenuAction {
    menu_name: String,
    item_name: String,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;

    fn CFAbsoluteTimeGetCurrent() -> CFAbsoluteTime;
    fn CFRunLoopGetMain() -> CFRunLoopRef;
    fn CFRunLoopTimerCreate(
        allocator: CFAllocatorRef,
        fire_date: CFAbsoluteTime,
        interval: CFTimeInterval,
        flags: CFOptionFlags,
        order: CFIndex,
        callout: unsafe extern "C" fn(CFRunLoopTimerRef, *mut c_void),
        context: *mut CFRunLoopTimerContext,
    ) -> CFRunLoopTimerRef;
    fn CFRunLoopAddTimer(run_loop: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
    fn CFRunLoopTimerInvalidate(timer: CFRunLoopTimerRef);
    fn CFRelease(object: *const c_void);
}

const EXPECTED_MENUS: [(&str, &[&str]); 3] = [
    (
        "BongoCat GPUI Spike",
        &[
            "Services",
            "Hide BongoCat GPUI Spike",
            "Hide Others",
            "Show All",
            "Quit BongoCat GPUI Spike",
        ],
    ),
    ("Edit", &["Cut", "Copy", "Paste", "Select All"]),
    ("Window", &["Minimize", "Zoom"]),
];

unsafe fn string_from_object(value: *mut AnyObject) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: value is an NSString owned by the live NSMenu hierarchy.
    let utf8: *const c_char = unsafe { msg_send![value, UTF8String] };
    (!utf8.is_null()).then(|| {
        // SAFETY: NSString returns a NUL-terminated UTF-8 view that remains
        // valid while the menu owns the string.
        unsafe { CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned()
    })
}

unsafe fn menu_title(item: *mut AnyObject) -> Option<String> {
    // SAFETY: item is an NSMenuItem from NSMenu::itemAtIndex:.
    let title: *mut AnyObject = unsafe { msg_send![item, title] };
    // SAFETY: NSMenuItem::title returns a live NSString.
    unsafe { string_from_object(title) }
}

unsafe fn main_menu() -> Result<*mut AnyObject, String> {
    // SAFETY: this probe runs on the AppKit main thread after GPUI set_menus.
    let application: *mut AnyObject =
        unsafe { msg_send![class!(NSApplication), sharedApplication] };
    if application.is_null() {
        return Err("NSApplication.sharedApplication returned nil".into());
    }
    // SAFETY: sharedApplication is a valid NSApplication instance.
    let menu: *mut AnyObject = unsafe { msg_send![application, mainMenu] };
    (!menu.is_null())
        .then_some(menu)
        .ok_or_else(|| "NSApplication.mainMenu returned nil".into())
}

unsafe fn submenu_named(main_menu: *mut AnyObject, name: &str) -> Option<*mut AnyObject> {
    // SAFETY: main_menu is the live NSApplication main menu.
    let count: isize = unsafe { msg_send![main_menu, numberOfItems] };
    for index in 0..count {
        // SAFETY: index is bounded by numberOfItems.
        let item: *mut AnyObject = unsafe { msg_send![main_menu, itemAtIndex: index] };
        if item.is_null() || unsafe { menu_title(item) }.as_deref() != Some(name) {
            continue;
        }
        // SAFETY: item is a live top-level NSMenuItem.
        let submenu: *mut AnyObject = unsafe { msg_send![item, submenu] };
        if !submenu.is_null() {
            return Some(submenu);
        }
    }
    None
}

unsafe fn item_index(submenu: *mut AnyObject, name: &str) -> Option<isize> {
    // SAFETY: submenu is retained by the live NSApplication main menu.
    let count: isize = unsafe { msg_send![submenu, numberOfItems] };
    (0..count).find(|index| {
        // SAFETY: index is bounded by numberOfItems.
        let item: *mut AnyObject = unsafe { msg_send![submenu, itemAtIndex: *index] };
        !item.is_null() && unsafe { menu_title(item) }.as_deref() == Some(name)
    })
}

pub fn verify_structure() -> Result<(), String> {
    // SAFETY: callers run this on the GPUI/AppKit main thread. The returned
    // menu objects remain owned by NSApplication throughout the inspection.
    unsafe {
        let main_menu = main_menu()?;
        for (menu_name, expected_items) in EXPECTED_MENUS {
            let submenu = submenu_named(main_menu, menu_name)
                .ok_or_else(|| format!("native menu '{menu_name}' was not installed"))?;
            for item_name in expected_items {
                if item_index(submenu, item_name).is_none() {
                    return Err(format!(
                        "native menu '{menu_name}' did not contain '{item_name}'"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn perform(menu_name: &str, item_name: &str) -> Result<(), String> {
    // SAFETY: callers run this on the GPUI/AppKit main thread. AppKit owns the
    // menu hierarchy, and performActionForItemAtIndex: validates and dispatches
    // the same target/action pair used by a user menu selection.
    unsafe {
        let main_menu = main_menu()?;
        let submenu = submenu_named(main_menu, menu_name)
            .ok_or_else(|| format!("native menu '{menu_name}' was not installed"))?;
        let index = item_index(submenu, item_name)
            .ok_or_else(|| format!("native menu '{menu_name}' did not contain '{item_name}'"))?;
        let _: () = msg_send![submenu, update];
        let item: *mut AnyObject = msg_send![submenu, itemAtIndex: index];
        if item.is_null() {
            return Err(format!(
                "native menu '{menu_name}' lost '{item_name}' before dispatch"
            ));
        }
        let enabled: bool = msg_send![item, isEnabled];
        if !enabled {
            return Err(format!(
                "native menu '{menu_name}' item '{item_name}' was disabled"
            ));
        }
        let _: () = msg_send![submenu, performActionForItemAtIndex: index];
    }
    Ok(())
}

unsafe extern "C" fn release_scheduled_action(info: *const c_void) {
    if !info.is_null() {
        // SAFETY: schedule_perform transfers exactly one Box allocation to
        // the timer context, and Core Foundation calls release once.
        drop(unsafe { Box::from_raw(info.cast_mut().cast::<ScheduledMenuAction>()) });
    }
}

unsafe extern "C" fn perform_scheduled_action(timer: CFRunLoopTimerRef, info: *mut c_void) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if info.is_null() {
            return Err("menu timer context was null".to_string());
        }
        // SAFETY: the timer context owns a live ScheduledMenuAction until the
        // timer is invalidated below.
        let action = unsafe { &*info.cast::<ScheduledMenuAction>() };
        perform(&action.menu_name, &action.item_name)
    }));
    match result {
        Ok(Ok(())) => {
            // SAFETY: info remains valid until the timer is invalidated below.
            let action = unsafe { &*info.cast::<ScheduledMenuAction>() };
            println!(
                "gpui-settings-spike: native menu action performed menu={} item={}",
                action.menu_name, action.item_name
            );
        }
        Ok(Err(error)) => eprintln!("gpui-settings-spike: menu probe failed: {error}"),
        Err(_) => eprintln!("gpui-settings-spike: menu probe callback panicked"),
    }
    // SAFETY: timer is the live one-shot timer invoking this callback. Its
    // context release callback owns cleanup of ScheduledMenuAction.
    unsafe { CFRunLoopTimerInvalidate(timer) };
}

pub fn schedule_perform(menu_name: &str, item_name: &str, delay: Duration) -> Result<(), String> {
    // Resolve the item before scheduling so missing menu structure is a
    // synchronous probe failure rather than a log-only callback failure.
    // SAFETY: this inspection runs on the GPUI/AppKit main thread.
    unsafe {
        let main_menu = main_menu()?;
        let submenu = submenu_named(main_menu, menu_name)
            .ok_or_else(|| format!("native menu '{menu_name}' was not installed"))?;
        if item_index(submenu, item_name).is_none() {
            return Err(format!(
                "native menu '{menu_name}' did not contain '{item_name}'"
            ));
        }

        let action = Box::new(ScheduledMenuAction {
            menu_name: menu_name.into(),
            item_name: item_name.into(),
        });
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: Box::into_raw(action).cast(),
            retain: None,
            release: Some(release_scheduled_action),
            copy_description: None,
        };
        let timer = CFRunLoopTimerCreate(
            ptr::null(),
            CFAbsoluteTimeGetCurrent() + delay.as_secs_f64(),
            0.0,
            0,
            0,
            perform_scheduled_action,
            &mut context,
        );
        if timer.is_null() {
            release_scheduled_action(context.info);
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
