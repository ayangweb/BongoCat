//! Manual smoke harness for the native model source pickers.
//!
//! A model source is either a folder or a `.zip` archive, so the smoke covers
//! both entry points:
//!
//! ```text
//! cargo run -p bongocat-platform --example model_source_picker_smoke -- --expect-cancel
//! cargo run -p bongocat-platform --example model_source_picker_smoke -- --expect-selected-any --kind archive
//! cargo run -p bongocat-platform --example model_source_picker_smoke -- --expect-selected <path> --kind directory
//! ```
//!
//! `--kind` defaults to `directory`. On Windows `--auto` additionally drives the
//! dialog with the Win32 controller.

use bongocat_platform::{pick_model_archive, pick_model_directory};
use std::{error::Error, io};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use bongocat_platform::ModelSourcePickerError;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use bongocat_platform::ModelSourcePickerOutcome;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{
    env,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc},
};

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    Directory,
    Archive,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SourceKind {
    /// Whether an arbitrary selection is a plausible source of this kind. Which
    /// file is *usable* is the model store's decision, so the smoke only checks
    /// the shape the picker promises.
    fn accepts(self, path: &std::path::Path) -> bool {
        match self {
            Self::Directory => path.is_dir(),
            Self::Archive => path.is_file(),
        }
    }
}

#[cfg(target_os = "windows")]
use std::{sync::atomic::Ordering, thread, time::Duration};

#[cfg(target_os = "windows")]
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IDOK, IsWindowVisible, PostMessageW, WM_CLOSE,
            WM_COMMAND,
        },
    },
    core::BOOL,
};

#[cfg(target_os = "macos")]
struct NativeApplication {
    _window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
}

#[cfg(target_os = "macos")]
fn prepare_native_application() -> NativeApplication {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let mtm = MainThreadMarker::new().expect("picker smoke must run on the AppKit main thread");
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // SAFETY: the caller runs on the AppKit main thread, the allocated window is retained for the
    // smoke process lifetime, and `releasedWhenClosed(false)` satisfies objc2's ownership contract.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 200.0)),
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: this retained NSWindow instance is never closed by the smoke process.
    unsafe { window.setReleasedWhenClosed(false) };
    window.center();
    window.makeKeyAndOrderFront(None);
    application.activate();
    NativeApplication { _window: window }
}

#[cfg(target_os = "macos")]
fn start_native_picker(
    kind: SourceKind,
    sender: mpsc::SyncSender<
        Result<ModelSourcePickerOutcome, bongocat_platform::ModelSourcePickerError>,
    >,
) {
    use dispatch2::DispatchQueue;

    DispatchQueue::main().exec_async(move || {
        let callback_sender = sender;
        let callback_sender_for_picker = callback_sender.clone();
        let result = match kind {
            SourceKind::Directory => pick_model_directory(move |result| {
                let _ = callback_sender_for_picker.try_send(result);
                // Project validation runs on a worker thread, so dispatch shutdown unconditionally
                // rather than assuming the callback is already on AppKit's main thread.
                DispatchQueue::main().exec_async(|| {
                    let mtm = objc2::MainThreadMarker::new()
                        .expect("NSApplication stop must run on the AppKit main thread");
                    objc2_app_kit::NSApplication::sharedApplication(mtm).stop(None);
                });
            }),
            SourceKind::Archive => pick_model_archive(move |result| {
                let _ = callback_sender_for_picker.try_send(result);
                DispatchQueue::main().exec_async(|| {
                    let mtm = objc2::MainThreadMarker::new()
                        .expect("NSApplication stop must run on the AppKit main thread");
                    objc2_app_kit::NSApplication::sharedApplication(mtm).stop(None);
                });
            }),
        };
        if let Err(error) = result {
            let _ = callback_sender.try_send(Err(error));
            DispatchQueue::main().exec_async(|| {
                let mtm = objc2::MainThreadMarker::new()
                    .expect("NSApplication stop must run on the AppKit main thread");
                objc2_app_kit::NSApplication::sharedApplication(mtm).stop(None);
            });
        }
    });
}

#[cfg(target_os = "windows")]
fn run_native_application() {}

#[cfg(target_os = "macos")]
fn run_native_application(_application: &NativeApplication) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let mtm = MainThreadMarker::new().expect("picker smoke must run on the AppKit main thread");
    NSApplication::sharedApplication(mtm).run();
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
enum ExpectedOutcome {
    Cancelled,
    Selected(PathBuf),
    SelectedAny,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct SmokeOptions {
    expected: ExpectedOutcome,
    kind: SourceKind,
    automated: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn smoke_options() -> Result<SmokeOptions, io::Error> {
    let mut arguments = env::args().skip(1);
    let expected = match arguments.next().as_deref() {
        Some("--expect-cancel") => ExpectedOutcome::Cancelled,
        Some("--expect-selected") => {
            let path = arguments
                .next()
                .ok_or_else(|| io::Error::other("--expect-selected requires a path"))?;
            ExpectedOutcome::Selected(PathBuf::from(path).canonicalize()?)
        }
        Some("--expect-selected-any") => ExpectedOutcome::SelectedAny,
        _ => {
            return Err(io::Error::other(
                "expected --expect-cancel, --expect-selected <path>, or --expect-selected-any",
            ));
        }
    };
    let mut kind = SourceKind::Directory;
    let mut automated = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--auto" => automated = true,
            "--kind" => {
                kind = match arguments.next().as_deref() {
                    Some("directory") => SourceKind::Directory,
                    Some("archive") => SourceKind::Archive,
                    _ => {
                        return Err(io::Error::other("--kind requires 'directory' or 'archive'"));
                    }
                };
            }
            _ => return Err(io::Error::other("unexpected picker smoke argument")),
        }
    }
    Ok(SmokeOptions {
        expected,
        kind,
        automated,
    })
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
enum DialogAction {
    Accept,
    Cancel,
}

#[cfg(target_os = "windows")]
struct WindowSearch {
    process_id: u32,
    windows: Vec<HWND>,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn find_process_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: EnumWindows invokes this callback synchronously while the stack-owned WindowSearch
    // remains valid and exclusively borrowed by the enumeration call.
    let search = unsafe { &mut *(lparam.0 as *mut WindowSearch) };
    let mut process_id = 0;
    // SAFETY: hwnd comes from EnumWindows and process_id is valid writable storage for this call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    // SAFETY: hwnd comes from EnumWindows and remains valid for the duration of this callback.
    if process_id == search.process_id && unsafe { IsWindowVisible(hwnd) }.as_bool() {
        search.windows.push(hwnd);
    }
    BOOL(1)
}

#[cfg(target_os = "windows")]
fn current_process_windows() -> Result<Vec<HWND>, io::Error> {
    let mut search = WindowSearch {
        process_id: std::process::id(),
        windows: Vec::new(),
    };
    // SAFETY: the callback and LPARAM point to stack storage that remains alive for the synchronous
    // enumeration; the callback never retains the pointer.
    let enumeration = unsafe {
        EnumWindows(
            Some(find_process_window),
            LPARAM((&mut search as *mut WindowSearch) as isize),
        )
    };
    enumeration.map_err(|error| io::Error::other(error.to_string()))?;
    Ok(search.windows)
}

#[cfg(target_os = "windows")]
fn automate_dialog(action: DialogAction, completed: Arc<AtomicBool>) -> Result<(), io::Error> {
    let mut posted_messages = 0u32;
    for _ in 0..200 {
        if completed.load(Ordering::Acquire) {
            return Ok(());
        }
        for window in current_process_windows()? {
            let result = match action {
                // SAFETY: the discovered visible window belongs to this smoke process; WM_COMMAND
                // with IDOK exercises the dialog's standard confirmation path without retaining HWND.
                DialogAction::Accept => unsafe {
                    PostMessageW(Some(window), WM_COMMAND, WPARAM(IDOK.0 as usize), LPARAM(0))
                },
                // SAFETY: the discovered visible window belongs to this smoke process; WM_CLOSE
                // exercises the native dialog's standard cancellation path without retaining HWND.
                DialogAction::Cancel => unsafe {
                    PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                },
            };
            if result.is_ok() {
                posted_messages = posted_messages.saturating_add(1);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::other(format!(
        "timed out waiting for the Windows model source picker after {posted_messages} message posts"
    )))
}

#[cfg(target_os = "windows")]
fn start_automation(
    expected: &ExpectedOutcome,
    automated: bool,
    completed: Arc<AtomicBool>,
) -> Result<Option<thread::JoinHandle<Result<(), io::Error>>>, io::Error> {
    if !automated {
        return Ok(None);
    }
    let action = match expected {
        ExpectedOutcome::Cancelled => DialogAction::Cancel,
        ExpectedOutcome::Selected(_) | ExpectedOutcome::SelectedAny => DialogAction::Accept,
    };
    thread::Builder::new()
        .name("bongocat-picker-smoke-controller".to_owned())
        .spawn(move || automate_dialog(action, completed))
        .map(Some)
}

#[cfg(target_os = "macos")]
fn start_automation(
    _expected: &ExpectedOutcome,
    automated: bool,
    _completed: Arc<AtomicBool>,
) -> Result<Option<std::thread::JoinHandle<Result<(), io::Error>>>, io::Error> {
    if automated {
        return Err(io::Error::other(
            "--auto is supported only by the Windows picker smoke",
        ));
    }
    Ok(None)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn verify_unsupported_platform() -> Result<(), Box<dyn Error>> {
    // The platform layer exposes no native model source picker on this target. Both entry points
    // must reject the request synchronously and must never invoke the completion callback.
    match pick_model_directory(|_| {
        unreachable!("the folder picker must not invoke its callback without a native backend")
    }) {
        Err(ModelSourcePickerError::UnsupportedPlatform) => {}
        Ok(()) => {
            return Err(io::Error::other(
                "the folder picker reported success without a native backend",
            )
            .into());
        }
        Err(error) => {
            return Err(io::Error::other(format!(
                "the folder picker reported '{error}' instead of an unsupported platform"
            ))
            .into());
        }
    }
    match pick_model_archive(|_| {
        unreachable!("the archive picker must not invoke its callback without a native backend")
    }) {
        Err(ModelSourcePickerError::UnsupportedPlatform) => Ok(()),
        Ok(()) => Err(io::Error::other(
            "the archive picker reported success without a native backend",
        )
        .into()),
        Err(error) => Err(io::Error::other(format!(
            "the archive picker reported '{error}' instead of an unsupported platform"
        ))
        .into()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn run_native_picker_smoke() -> Result<(), Box<dyn Error>> {
    let options = smoke_options()?;
    #[cfg(target_os = "macos")]
    let native_application = prepare_native_application();
    let (sender, receiver) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicBool::new(false));
    #[cfg(target_os = "macos")]
    start_native_picker(options.kind, sender.clone());
    #[cfg(target_os = "windows")]
    {
        let callback_completed = Arc::clone(&completed);
        match options.kind {
            SourceKind::Directory => pick_model_directory(move |result| {
                callback_completed.store(true, Ordering::Release);
                let _ = sender.send(result);
            })?,
            SourceKind::Archive => pick_model_archive(move |result| {
                callback_completed.store(true, Ordering::Release);
                let _ = sender.send(result);
            })?,
        }
    }
    let automation = start_automation(&options.expected, options.automated, completed)?;
    #[cfg(target_os = "macos")]
    run_native_application(&native_application);
    #[cfg(target_os = "windows")]
    run_native_application();
    #[cfg(target_os = "windows")]
    let actual = receiver
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| {
            io::Error::other("timed out waiting for the model source picker callback")
        })??;
    #[cfg(target_os = "macos")]
    let actual = receiver
        .recv()
        .map_err(|_| io::Error::other("model source picker callback was dropped"))??;
    if let Some(automation) = automation {
        automation
            .join()
            .map_err(|_| io::Error::other("model source picker automation panicked"))??;
    }
    match (options.expected, actual) {
        (ExpectedOutcome::Cancelled, ModelSourcePickerOutcome::Cancelled) => Ok(()),
        (ExpectedOutcome::Selected(expected), ModelSourcePickerOutcome::Selected(actual))
            if actual == expected =>
        {
            Ok(())
        }
        (ExpectedOutcome::SelectedAny, ModelSourcePickerOutcome::Selected(actual))
            if actual.is_absolute() && options.kind.accepts(&actual) =>
        {
            Ok(())
        }
        _ => Err(io::Error::other("model source picker returned an unexpected outcome").into()),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        verify_unsupported_platform()
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        run_native_picker_smoke()
    }
}
