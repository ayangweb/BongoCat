//! Startup permission check and native user guidance.
//!
//! The product needs a platform capability before global input works: macOS requires the
//! Input Monitoring TCC grant, Windows needs an elevated token to keep receiving Raw Input while a
//! higher-integrity window is in the foreground, and Linux needs read access to an evdev input
//! device. These are read-only queries that never prompt, so the check may run on every start.
//!
//! Nothing about the prompt is persisted. A user who dismisses it is asked again on the next start
//! while the platform still reports the capability as missing, and a user who already granted the
//! capability is never asked. The decision is always the current platform state, never product
//! state (ADR-0032).
//!
//! macOS presents the prompt itself through an `NSAlert` on the main thread, which follows the
//! application appearance (ADR-0048), and then hands the user to the guided flow from
//! `permission-flow` (ADR-0078): a floating panel that opens the Input Monitoring pane and shows
//! the drag guidance. Windows presents an `rfd` message dialog, which maps to a Task Dialog, and
//! reveals the running executable. No product UI is built for either.

/// Whether the operating system currently grants the input capability the
/// product needs before global input works.
///
/// The answer comes from the platform's own query: the macOS Input Monitoring
/// TCC grant, which the product also re-reads while a settings window is open
/// so the diagnostics page can show the current state. It is a read-only fact
/// about the machine, never a prompt result and never persisted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPermission {
    Denied,
    Granted,
}

/// Localized text for the startup prompt.
///
/// The application layer builds this from the translation catalog and hands it to the platform
/// adapter, so product copy never enters this crate and the adapter never reads configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupPermissionPrompt {
    pub title: String,
    pub description: String,
    /// Label of the button that starts the platform permission flow.
    pub primary: String,
    /// Label of the button that dismisses the prompt and keeps the product starting.
    pub secondary: String,
    /// The locale the copy above was written in.
    ///
    /// The macOS guided flow is a Swift panel with its own catalog, so the adapter needs the
    /// product language to make the panel and the prompt agree. It is the same value the caller
    /// resolved the copy from, which keeps the two from drifting apart.
    pub locale: String,
}

/// Outcome of one startup permission check.
///
/// Reported for the caller and the diagnostic run option only; the product records no prompt
/// state, so this value never changes a later start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupPermissionStatus {
    /// This platform has no startup permission flow implemented by the product.
    Unsupported,
    /// The platform already reports the capability; no prompt was shown.
    Satisfied,
    /// The prompt was shown and dismissed. The product keeps starting with the current
    /// capability and checks again on the next start.
    Deferred,
    /// The prompt was shown and the user asked for the platform permission flow. The payload
    /// reports whether the platform accepted the request.
    PermissionFlowRequested(bool),
}

/// Stable capability name for diagnostics. It never contains user data.
pub const STARTUP_PERMISSION_CAPABILITY: &str = platform::CAPABILITY;

/// Reads the startup capability without prompting or storing anything.
///
/// This is the only check the diagnostic run option and the acceptance record need, and it is
/// exactly the state the prompt acts on.
pub fn startup_permission_available() -> bool {
    platform::available()
}

/// Runs one startup permission check for the current platform.
///
/// The platform is queried first. When it already provides the capability nothing is shown; when
/// it does not, the prompt is presented once and the user decides whether the product keeps
/// starting with reduced input coverage or opens the platform permission flow.
pub fn check_startup_permission(prompt: &StartupPermissionPrompt) -> StartupPermissionStatus {
    if !platform::SUPPORTED {
        return StartupPermissionStatus::Unsupported;
    }
    if platform::available() {
        return StartupPermissionStatus::Satisfied;
    }
    let result = platform::present_prompt(prompt);
    if !requested_permission_flow(&result, &prompt.primary) {
        return StartupPermissionStatus::Deferred;
    }
    StartupPermissionStatus::PermissionFlowRequested(platform::request_permission_flow(
        &prompt.locale,
    ))
}

/// Presents the startup prompt and blocks the calling thread until the user answers it.
///
/// The caller is the dedicated startup-permission worker, never the main thread, so a pending
/// dialog cannot delay any product window. macOS hands the presentation to the main thread via
/// `dispatch2::run_on_main` and waits for the answer there: AppKit windows must be built on the
/// main thread, and the GPUI platform already exists because the worker is spawned inside the
/// GPUI run loop (ADR-0032, non-blocking amendment).
///
/// The dialog is an `NSAlert` rather than the parentless `rfd` async dialog the module used
/// before 2026-09-18. That rfd path is a `CFUserNotification`, which never touches AppKit and
/// therefore never inherits `NSApplication.appearance` — it stayed on the system appearance
/// while the product ran dark (ADR-0048), which is exactly the defect this switch repairs.
/// `NSAlert` windows inherit the application appearance, so the prompt follows the theme the
/// product applies at startup. `rfd`'s *synchronous* macOS dialog remains banned for a
/// different, older reason: it would still create the shared `NSApplication` on this worker
/// before GPUI's subclass owns it (the contract test below keeps pinning that).
#[cfg(target_os = "macos")]
mod platform {
    use crate::{
        InputPermission, StartupPermissionPrompt, input_monitoring_permission,
        request_input_monitoring_permission,
    };
    use objc2::{rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSModalResponse};
    use objc2_foundation::{NSArray, NSString, NSUserDefaults};
    use permission_flow::{AppPath, Permission, PermissionFlowController, StartFlowOptions};
    use std::{
        cell::RefCell,
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    /// System Settings → Privacy & Security → Input Monitoring.
    const INPUT_MONITORING_SETTINGS_URL: &str =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

    /// The response `-runModal` returns for the first added button, `NSAlertFirstButtonReturn`
    /// from AppKit's `NSAlert.h`. The constants named `NSModalResponseOK`/`NSModalResponseCancel`
    /// in `objc2-app-kit` carry the deprecated `NSOKButton`(1)/`NSCancelButton`(0) values from
    /// `NSPanel.h` instead, which `-runModal` does not return for custom buttons, so they must
    /// not be used here (verified against the macOS SDK header, 2026-09-18).
    const ALERT_FIRST_BUTTON_RESPONSE: NSModalResponse = 1000;

    /// `tccutil`, the only supported way to clear a TCC grant from inside the product.
    const TCCUTIL: &str = "/usr/bin/tccutil";

    /// The TCC service behind the Input Monitoring pane.
    const LISTEN_EVENT_SERVICE: &str = "ListenEvent";

    /// The bundle identifier every build environment shares (ADR-0008), which is therefore also
    /// the TCC subject the guided flow resets.
    const PRODUCT_BUNDLE_IDENTIFIER: &str = "com.ayangweb.bongo-cat";

    /// The resource bundle the vendored Swift package resolves its strings from.
    const RESOURCE_BUNDLE_NAME: &str = "PermissionFlow_PermissionFlow.bundle";

    /// The preference macOS resolves a bundle's `.lproj` catalogue from.
    const APPLE_LANGUAGES_KEY: &str = "AppleLanguages";

    thread_local! {
        /// The guided flow's controller.
        ///
        /// `PermissionFlowController` is neither `Send` nor `Sync` and may only be created, used
        /// and dropped on the macOS main thread, so it lives in a main-thread-local slot instead
        /// of travelling back to the worker that asked for the flow. Keeping it alive is what
        /// keeps the floating panel on screen; starting another flow replaces it, and its `Drop`
        /// closes the panel.
        static FLOW_CONTROLLER: RefCell<Option<PermissionFlowController>> =
            const { RefCell::new(None) };
    }

    pub const CAPABILITY: &str = "input_monitoring";
    pub const SUPPORTED: bool = true;

    pub fn available() -> bool {
        input_monitoring_permission() == InputPermission::Granted
    }

    pub fn present_prompt(prompt: &StartupPermissionPrompt) -> rfd::MessageDialogResult {
        let title = prompt.title.clone();
        let description = prompt.description.clone();
        let primary = prompt.primary.clone();
        let secondary = prompt.secondary.clone();
        // `runModal` runs from a main-queue block, which AppKit dispatches between GPUI
        // events — never nested inside a GPUI event handler, which is the reentrancy that
        // forced the model pickers to require a sheet parent (ADR-0032).
        dispatch2::run_on_main(move |mtm| {
            // The prompt asks for a user decision at first start, when another application is
            // usually the active one; without this the modal alert can sit behind it. The
            // selector exists on every supported release while the replacement `-activate` is
            // macOS 14+ and the product supports 12+.
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            let alert = NSAlert::new(mtm);
            alert.setAlertStyle(NSAlertStyle::Warning);
            alert.setMessageText(&NSString::from_str(&title));
            alert.setInformativeText(&NSString::from_str(&description));
            // Added most-to-least prominent: the flow button first (Return key), the
            // dismiss button second.
            alert.addButtonWithTitle(&NSString::from_str(&primary));
            alert.addButtonWithTitle(&NSString::from_str(&secondary));
            if alert.runModal() == ALERT_FIRST_BUTTON_RESPONSE {
                // The adapter-internal contract with `check_startup_permission` stays the rfd
                // result type so both platforms map through the same code below.
                rfd::MessageDialogResult::Custom(primary)
            } else {
                rfd::MessageDialogResult::Cancel
            }
        })
    }

    /// Runs the user-initiated permission flow.
    ///
    /// The order is the one the flow was asked for (ADR-0078): clear the grant, make sure the
    /// panel's strings can be resolved, put the panel in the product's language, and only then
    /// open the pane and show the drag guidance.
    ///
    /// The fallback path is the pre-ADR-0078 behaviour and exists for one case: the Swift panel
    /// aborts the process on its first localized string when its resource bundle is missing, so a
    /// build whose bundle cannot be resolved opens the pane without the guidance rather than
    /// crashing. Packaging verifies the bundle is present, which is where a mistake has to fail.
    pub fn request_permission_flow(locale: &str) -> bool {
        reset_input_monitoring_grant();
        if !ensure_resource_bundle_available() {
            let _ = request_input_monitoring_permission();
            return open_input_monitoring_settings();
        }
        apply_flow_language(locale);
        start_guided_flow()
    }

    /// Clears the Input Monitoring grant before the guided flow starts.
    ///
    /// The product asks for this on every entry into the flow, so the flow always begins from a
    /// not-granted state. Consequences that ADR-0078 records rather than hides: every build
    /// environment shares one bundle identifier, so this also clears the grant of an installed
    /// build, and a running event tap loses its grant and enters the `PermissionDenied` recovery
    /// path. The result is deliberately ignored — `tccutil` reports failure for a bundle the
    /// system has no entry for, which is the same state this is trying to reach.
    fn reset_input_monitoring_grant() {
        let _ = Command::new(TCCUTIL)
            .args(["reset", LISTEN_EVENT_SERVICE, PRODUCT_BUNDLE_IDENTIFIER])
            .status();
    }

    /// Opens the Input Monitoring pane.
    ///
    /// `NSWorkspace` is not part of the shared-application machinery: it neither creates nor reads
    /// `NSApplication`, and neither `sharedWorkspace` nor `openURL` is main-thread-only in the
    /// `objc2-app-kit` bindings, so this stays safe on the startup-permission worker.
    fn open_input_monitoring_settings() -> bool {
        use objc2_app_kit::NSWorkspace;
        use objc2_foundation::{NSString, NSURL};

        let value = NSString::from_str(INPUT_MONITORING_SETTINGS_URL);
        let Some(url) = NSURL::URLWithString(&value) else {
            return false;
        };
        NSWorkspace::sharedWorkspace().openURL(&url)
    }

    /// Starts the guided flow and keeps its controller alive.
    ///
    /// `PermissionFlowController::new` asserts the macOS main thread, and the controller is
    /// neither `Send` nor `Sync`, so the whole sequence runs inside one main-queue block and the
    /// controller is parked in the main thread's slot before the block returns.
    fn start_guided_flow() -> bool {
        let target = guidance_target();
        dispatch2::run_on_main(move |_mtm| {
            let Ok(path) = AppPath::try_from(target.as_path()) else {
                return false;
            };
            let Ok(controller) = PermissionFlowController::new() else {
                return false;
            };
            if controller
                .start_flow(StartFlowOptions::new(Permission::INPUT_MONITORING, path))
                .is_err()
            {
                return false;
            }
            FLOW_CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
            true
        })
    }

    /// The bundle the panel asks the user to drag into the authorization list.
    ///
    /// The executable's own bundle when there is one, and the executable otherwise. Upstream's
    /// helper that guesses the host application from the launch context is deliberately not used:
    /// it falls back to the parent process chain, so a development build would ask the user to
    /// grant whichever application launched it (the contract test below keeps that out). The Swift
    /// panel filters the list down to `.app` bundles, so a development binary yields an empty drag
    /// target and the panel keeps showing the pane guidance.
    fn guidance_target() -> PathBuf {
        let Ok(executable) = std::env::current_exe() else {
            return PathBuf::new();
        };
        enclosing_bundle(&executable).unwrap_or(executable)
    }

    /// The application bundle an executable lives inside, if any.
    fn enclosing_bundle(executable: &Path) -> Option<PathBuf> {
        executable
            .ancestors()
            .find(|ancestor| {
                ancestor
                    .extension()
                    .is_some_and(|extension| extension == "app")
            })
            .map(Path::to_path_buf)
    }

    /// Makes the Swift panel resolve its strings in the product's language.
    ///
    /// The vendored Swift package ships eleven `.lproj` catalogues and chooses one through
    /// `Bundle.preferredLocalizations`, which for a bundle nested in an application follows the
    /// application's own preferred localizations — that is, `AppleLanguages` in this application's
    /// preference domain, which the application bundle has to declare through
    /// `CFBundleLocalizations`. Upstream's Rust API has no locale parameter at all (the Swift side
    /// has one the shim never passes), so this preference is the only lever before the fork adds
    /// it. It is also the behaviour a language setting is expected to have: the app's other AppKit
    /// surfaces follow the same value.
    ///
    /// `vi-VN` has no upstream catalogue and resolves to English, which ADR-0078 records.
    fn apply_flow_language(locale: &str) {
        let languages = NSArray::from_retained_slice(&[NSString::from_str(locale)]);
        // SAFETY: `NSArray` is an Objective-C object and `AnyObject` is the top type every object
        // can be viewed as, which is the type `setObject:forKey:` takes. The value is an array of
        // strings, which is what `AppleLanguages` holds.
        let languages = unsafe { Retained::cast_unchecked::<AnyObject>(languages) };
        let key = NSString::from_str(APPLE_LANGUAGES_KEY);
        // SAFETY: the key is a plain string and the value has the type the preference expects.
        unsafe {
            NSUserDefaults::standardUserDefaults().setObject_forKey(Some(&languages), &key);
        }
    }

    /// Makes sure the Swift package's resource bundle is where `Bundle.module` looks for it.
    ///
    /// `swift-rs` builds the bundle into its own `OUT_DIR` and never publishes it, and the accessor
    /// SwiftPM generates calls `fatalError("unable to find bundle named …")` when none of its
    /// candidates contains it — `Bundle.main.resourceURL`, the framework resource URL and
    /// `Bundle.main.bundleURL`. Packaging places the bundle in `Contents/Resources` for a release
    /// artifact, so only a development binary has work to do here, and for it the executable's own
    /// directory is the candidate that matches. Returns whether the bundle can now be resolved;
    /// nothing is written into an application bundle, which would invalidate its signature.
    fn ensure_resource_bundle_available() -> bool {
        let Ok(executable) = std::env::current_exe() else {
            return false;
        };
        let Some(executable_dir) = executable.parent() else {
            return false;
        };
        if let Some(bundle) = enclosing_bundle(&executable)
            && bundle
                .join("Contents/Resources")
                .join(RESOURCE_BUNDLE_NAME)
                .is_dir()
        {
            return true;
        }
        let destination = executable_dir.join(RESOURCE_BUNDLE_NAME);
        if destination.is_dir() {
            return true;
        }
        let Some(source) = built_resource_bundle(executable_dir) else {
            return false;
        };
        copy_dir(&source, &destination);
        destination.is_dir()
    }

    /// Finds the resource bundle `swift-rs` built, under the Cargo profile directory.
    ///
    /// `permission-flow` owns two `build` entries: the build script's own directory, which has no
    /// Swift output, and the one holding its `OUT_DIR`. A missing directory skips to the next entry
    /// instead of ending the search.
    pub(super) fn built_resource_bundle(executable_dir: &Path) -> Option<PathBuf> {
        for entry in fs::read_dir(executable_dir.join("build")).ok()?.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("permission-flow-") {
                continue;
            }
            let build_path = entry.path().join("out/swift-rs/PermissionFlowShimFFI");
            if let Some(bundle) = find_resource_bundle(&build_path) {
                return Some(bundle);
            }
        }
        None
    }

    /// Finds the resource bundle below the build path `swift-rs` gave to SwiftPM.
    ///
    /// That layout has already changed once: before Xcode 27 SwiftPM wrote products to
    /// `<arch>-apple-macosx/<Configuration>` under the build path, and since then to
    /// `[out/]Products/<Configuration>`, with Xcode 27 keeping the older directory around as well.
    /// A search pinned to one shape silently finds nothing on the other toolchain, which here means
    /// the panel never opens and the user gets the old prompt with no explanation. The bundle is
    /// therefore looked up by walking the package's own build path, bounded because the deepest
    /// layout in use puts it three levels down.
    fn find_resource_bundle(build_path: &Path) -> Option<PathBuf> {
        const MAX_DEPTH: usize = 3;

        let mut pending = vec![(build_path.to_path_buf(), 0usize)];
        while let Some((directory, depth)) = pending.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .file_name()
                    .is_some_and(|name| name == RESOURCE_BUNDLE_NAME)
                    && path.is_dir()
                {
                    return Some(path);
                }
                if depth < MAX_DEPTH && path.is_dir() {
                    pending.push((path, depth + 1));
                }
            }
        }
        None
    }

    fn copy_dir(from: &Path, to: &Path) {
        let Ok(entries) = fs::read_dir(from) else {
            return;
        };
        let _ = fs::create_dir_all(to);
        for entry in entries.flatten() {
            let source = entry.path();
            let destination = to.join(entry.file_name());
            if source.is_dir() {
                copy_dir(&source, &destination);
            } else {
                let _ = fs::copy(&source, &destination);
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use crate::StartupPermissionPrompt;
    use std::{mem::size_of, path::Path};
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    pub const CAPABILITY: &str = "administrator";
    pub const SUPPORTED: bool = true;

    pub fn available() -> bool {
        process_is_elevated()
    }

    pub fn present_prompt(prompt: &StartupPermissionPrompt) -> rfd::MessageDialogResult {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(prompt.title.clone())
            .set_description(prompt.description.clone())
            .set_buttons(rfd::MessageButtons::OkCancelCustom(
                prompt.primary.clone(),
                prompt.secondary.clone(),
            ))
            .show()
    }

    /// Runs the user-initiated permission flow.
    ///
    /// There is no in-place elevation: ADR-0023 keeps the per-user installer and the product
    /// unprivileged by default. The standard path is the compatibility flag, so the flow only
    /// reveals the running executable for the user to open its properties dialog. The locale is a
    /// macOS concern: this path shows no panel of its own.
    pub fn request_permission_flow(_locale: &str) -> bool {
        let Ok(executable) = std::env::current_exe() else {
            return false;
        };
        reveal_executable(&executable)
    }

    fn reveal_executable(executable: &Path) -> bool {
        opener::reveal(executable).is_ok()
    }

    /// Reads `TokenElevation` from the current process token.
    ///
    /// The compatibility flag the prompt asks for starts the process elevated, so this reports
    /// exactly the state the guidance promises, and it also reports it for a user who launched the
    /// product from an already elevated shell.
    fn process_is_elevated() -> bool {
        // SAFETY: the process handle is a pseudo handle that needs no release, the token handle is
        // closed on every path below, and `TOKEN_ELEVATION` is the documented output type for
        // `TokenElevation`, so the buffer has the exact size the call expects.
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned_length = 0_u32;
            let result = GetTokenInformation(
                token,
                TokenElevation,
                Some(std::ptr::from_mut(&mut elevation).cast()),
                u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or_default(),
                &mut returned_length,
            );
            let _ = CloseHandle(token);
            result.is_ok() && elevation.TokenIsElevated != 0
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use crate::{InputPermission, StartupPermissionPrompt, linux::evdev_input_permission};

    pub const CAPABILITY: &str = "evdev_access";
    pub const SUPPORTED: bool = false;

    pub fn available() -> bool {
        evdev_input_permission() == InputPermission::Granted
    }

    pub fn present_prompt(_prompt: &StartupPermissionPrompt) -> rfd::MessageDialogResult {
        rfd::MessageDialogResult::Cancel
    }

    pub fn request_permission_flow(_locale: &str) -> bool {
        false
    }
}

/// Maps an `rfd` result back to "the user asked for the platform permission flow".
///
/// macOS returns the custom label, because the parentless dialog is a `CFUserNotification` whose
/// buttons carry the requested titles. Windows returns the custom labels too since the workspace
/// enables `rfd`'s `common-controls-v6` feature and the executable already carries an application
/// manifest declaring the ComCtl32 v6 dependency (embedded by `gpui-pre`'s static library), so the
/// prompt is a `TaskDialogIndirect` whose buttons carry the requested titles; closing the dialog
/// reports `Cancel`. The standard-button mapping below stays as a safety net for a `MessageBoxW`
/// fallback: if the task dialog cannot bind (activation context missing), `TaskDialogIndirect`
/// fails and `rfd` reports `Cancel`, which must never be mistaken for consent.
fn requested_permission_flow(result: &rfd::MessageDialogResult, primary: &str) -> bool {
    match result {
        rfd::MessageDialogResult::Custom(label) => label == primary,
        rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => true,
        rfd::MessageDialogResult::No | rfd::MessageDialogResult::Cancel => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt() -> StartupPermissionPrompt {
        StartupPermissionPrompt {
            title: "title".to_owned(),
            description: "description".to_owned(),
            primary: "primary".to_owned(),
            secondary: "secondary".to_owned(),
            locale: "en-US".to_owned(),
        }
    }

    #[test]
    fn only_the_primary_choice_starts_the_permission_flow() {
        let prompt = prompt();
        assert!(requested_permission_flow(
            &rfd::MessageDialogResult::Custom(prompt.primary.clone()),
            &prompt.primary
        ));
        assert!(!requested_permission_flow(
            &rfd::MessageDialogResult::Custom(prompt.secondary.clone()),
            &prompt.primary
        ));
    }

    #[test]
    fn standard_button_results_map_without_a_custom_label() {
        let prompt = prompt();
        // Windows without `common-controls-v6` reports the standard pair instead of the custom
        // labels, so dismissal must stay distinguishable from the permission flow.
        assert!(requested_permission_flow(
            &rfd::MessageDialogResult::Ok,
            &prompt.primary
        ));
        assert!(!requested_permission_flow(
            &rfd::MessageDialogResult::Cancel,
            &prompt.primary
        ));
        // A closed window or an unknown label is never treated as consent for the flow.
        assert!(!requested_permission_flow(
            &rfd::MessageDialogResult::No,
            &prompt.primary
        ));
    }

    /// Pins the three macOS flow constraints.
    ///
    /// 1. `rfd`'s *synchronous* message dialog is still banned: it builds its `NSAlert` plumbing
    ///    (`PolicyManager`/`FocusManager`) on the calling thread, and a historical pre-GPUI caller
    ///    even crashed the product by creating the plain shared `NSApplication` there
    ///    (`objc-0.2.7`: "Ivar platform not found on class NSApplication"). The product then
    ///    aborts with `Ivar platform not found on class NSApplication` if anything constructs the
    ///    shared application before GPUI's `GPUIApplication` subclass.
    /// 2. The themed `NSAlert` must be handed to the main thread (`run_on_main`): AppKit windows
    ///    may only be built there, and `runModal` must run between GPUI events instead of inside
    ///    one of its handlers. The behaviour itself needs a human to answer a real system alert,
    ///    which no automated test can do, so this contract pins the implementation.
    /// 3. The guided flow's drag target must come from this executable, never from
    ///    `AppPath::suggested_host_app`: that helper walks the parent process chain, so a
    ///    development build would ask the user to grant whichever application launched it.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_prompt_keeps_the_appkit_free_dialog_path() {
        let source = include_str!("startup_permission.rs");
        let macos_module = source
            .split("#[cfg(target_os = \"macos\")]\nmod platform {")
            .nth(1)
            .expect("macOS platform module")
            .split("#[cfg(target_os = \"windows\")]")
            .next()
            .expect("macOS platform module end");
        assert!(
            !macos_module.contains("rfd::MessageDialog::new"),
            "the macOS startup prompt must not use rfd's synchronous message dialog: it creates \
             the plain NSApplication before GPUI's platform runs, and the product then aborts \
             with `Ivar platform not found on class NSApplication`"
        );
        assert!(
            macos_module.contains("run_on_main"),
            "the macOS startup prompt must present its NSAlert on the main thread: AppKit windows \
             are main-thread-only and `runModal` must not run inside a GPUI event handler"
        );
        assert!(
            !macos_module.contains("AsyncMessageDialog"),
            "the macOS startup prompt must not go back to rfd's parentless CFUserNotification: it \
             does not follow the application appearance (ADR-0048)"
        );
        assert!(
            !macos_module.contains("suggested_host_app"),
            "the macOS guided flow must resolve its drag target from this executable: \
             `AppPath::suggested_host_app` falls back to the parent process chain and would ask \
             the user to grant the application that launched the build"
        );
    }

    /// Pins the two macOS prerequisites the guided flow cannot start without.
    ///
    /// Both are supplied outside this module — the rpath by the build scripts and the resource
    /// bundle by packaging — so nothing else would fail until a user pressed the button.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_guided_flow_declares_its_build_prerequisites() {
        let source = include_str!("startup_permission.rs");
        assert!(
            source.contains("ListenEvent"),
            "the guided flow must reset the Input Monitoring grant through tccutil"
        );
        // Every package whose binaries link this one carries the flag, because a library
        // dependency's `rustc-link-arg` does not reach the binary that finally links.
        for (package, build_script) in [
            ("bongocat-platform", include_str!("../build.rs")),
            (
                "bongocat-overlay",
                include_str!("../../bongocat-overlay/build.rs"),
            ),
            ("bongocat-ui", include_str!("../../bongocat-ui/build.rs")),
            ("bongocat-app", include_str!("../../bongocat-app/build.rs")),
        ] {
            assert!(
                build_script.contains("-Wl,-rpath,/usr/lib/swift"),
                "{package} must put the Swift runtime on the loader path of its binaries: \
                 permission-flow's own build script cannot pass the flag on, and without it the \
                 process aborts before `main`"
            );
        }
    }

    /// A development binary has to find the resource bundle in every layout SwiftPM has used for a
    /// `--build-path` build.
    ///
    /// The lookup that used to be pinned to Xcode 27's `[out/]Products` shape is what made
    /// packaging fail in CI, and here the same mistake is silent: the bundle is not found, the
    /// panel never starts, and the user is left with the old prompt instead of the guided one.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_development_copy_finds_the_bundle_in_every_products_layout() {
        use super::platform::built_resource_bundle;

        for products in [
            "out/Products/Debug",
            "Products/Debug",
            "debug",
            "aarch64-apple-macosx/debug",
        ] {
            let root = std::env::temp_dir().join("bongocat-startup-permission-bundle");
            let _ = std::fs::remove_dir_all(&root);
            let expected = root
                .join("build/permission-flow-0123456789abcdef/out/swift-rs/PermissionFlowShimFFI")
                .join(products)
                .join("PermissionFlow_PermissionFlow.bundle");
            std::fs::create_dir_all(&expected).expect("products directory");

            assert_eq!(
                built_resource_bundle(&root),
                Some(expected),
                "the {products} layout must be searched"
            );

            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// A profile directory with no Swift output at all is the state of a build that never ran the
    /// guide, and it has to read as "not found" so the caller falls back instead of guessing.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_profile_directory_without_swift_output_reports_no_bundle() {
        use super::platform::built_resource_bundle;

        let root = std::env::temp_dir().join("bongocat-startup-permission-no-bundle");
        let _ = std::fs::remove_dir_all(&root);
        // The build script's own `build` entry exists and carries no Swift output.
        std::fs::create_dir_all(root.join("build/permission-flow-0123456789abcdef"))
            .expect("entry");

        assert_eq!(built_resource_bundle(&root), None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn capability_name_is_stable_and_anonymous() {
        assert!(matches!(
            STARTUP_PERMISSION_CAPABILITY,
            "input_monitoring" | "administrator" | "evdev_access"
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reports_the_interactive_permission_flow_as_unsupported() {
        assert_eq!(
            check_startup_permission(&prompt()),
            StartupPermissionStatus::Unsupported
        );
    }
}
