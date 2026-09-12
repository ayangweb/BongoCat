use bongocat_platform::{DirectoryPickerOutcome, pick_model_directory};
use std::{
    env,
    error::Error,
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

#[cfg(target_os = "windows")]
use std::{thread, time::Duration};

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
fn prepare_native_application() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let mtm = MainThreadMarker::new().expect("picker smoke must run on the AppKit main thread");
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    application.activate();
}

#[cfg(target_os = "macos")]
fn run_native_application() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let mtm = MainThreadMarker::new().expect("picker smoke must run on the AppKit main thread");
    NSApplication::sharedApplication(mtm).run();
}

#[cfg(not(target_os = "macos"))]
fn prepare_native_application() {}

#[cfg(not(target_os = "macos"))]
fn run_native_application() {}

enum ExpectedOutcome {
    Cancelled,
    Selected(PathBuf),
    SelectedAny,
}

struct SmokeOptions {
    expected: ExpectedOutcome,
    automated: bool,
}

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
    let automated = arguments.next().as_deref() == Some("--auto");
    if arguments.next().is_some() {
        return Err(io::Error::other("unexpected picker smoke argument"));
    }
    Ok(SmokeOptions {
        expected,
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
        "timed out waiting for the Windows directory picker after {posted_messages} message posts"
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

#[cfg(not(target_os = "windows"))]
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

fn main() -> Result<(), Box<dyn Error>> {
    let options = smoke_options()?;
    prepare_native_application();
    let (sender, receiver) = mpsc::sync_channel(1);
    let completed = Arc::new(AtomicBool::new(false));
    let callback_completed = Arc::clone(&completed);
    pick_model_directory(move |result| {
        callback_completed.store(true, Ordering::Release);
        let _ = sender.send(result);
        #[cfg(target_os = "macos")]
        {
            use objc2::MainThreadMarker;
            use objc2_app_kit::NSApplication;

            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).stop(None);
            }
        }
    })?;
    let automation = start_automation(&options.expected, options.automated, completed)?;
    run_native_application();
    #[cfg(target_os = "windows")]
    let actual = receiver
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| io::Error::other("timed out waiting for the directory picker callback"))??;
    #[cfg(not(target_os = "windows"))]
    let actual = receiver
        .recv()
        .map_err(|_| io::Error::other("directory picker callback was dropped"))??;
    if let Some(automation) = automation {
        automation
            .join()
            .map_err(|_| io::Error::other("directory picker automation panicked"))??;
    }
    match (options.expected, actual) {
        (ExpectedOutcome::Cancelled, DirectoryPickerOutcome::Cancelled) => Ok(()),
        (ExpectedOutcome::Selected(expected), DirectoryPickerOutcome::Selected(actual))
            if actual == expected =>
        {
            Ok(())
        }
        (ExpectedOutcome::SelectedAny, DirectoryPickerOutcome::Selected(actual))
            if actual.is_absolute() && actual.is_dir() =>
        {
            Ok(())
        }
        _ => Err(io::Error::other("directory picker returned an unexpected outcome").into()),
    }
}
