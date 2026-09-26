//! Startup permission check and native user guidance.
//!
//! The product needs a platform capability before global input works: macOS requires the
//! Input Monitoring TCC grant, and Windows needs an elevated token to keep receiving Raw Input
//! while a higher-integrity window is in the foreground. Both are read-only queries that never
//! prompt, so the check may run on every start.
//!
//! Nothing about the prompt is persisted. A user who dismisses it is asked again on the next start
//! while the platform still reports the capability as missing, and a user who already granted the
//! capability is never asked. The decision is always the current platform state, never product
//! state (ADR-0032).
//!
//! Both platforms present the prompt through a native dialog owned by the platform: macOS an
//! `NSAlert` on the main thread, which follows the application appearance (ADR-0048), Windows an
//! `rfd` message dialog, which maps to a Task Dialog. No product UI is built for this.

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
}

/// Outcome of one startup permission check.
///
/// Reported for the caller and the diagnostic run option only; the product records no prompt
/// state, so this value never changes a later start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupPermissionStatus {
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
    if platform::available() {
        return StartupPermissionStatus::Satisfied;
    }
    let result = platform::present_prompt(prompt);
    if !requested_permission_flow(&result, &prompt.primary) {
        return StartupPermissionStatus::Deferred;
    }
    StartupPermissionStatus::PermissionFlowRequested(platform::request_permission_flow())
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
    use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSModalResponse};
    use objc2_foundation::NSString;

    /// System Settings → Privacy & Security → Input Monitoring.
    const INPUT_MONITORING_SETTINGS_URL: &str =
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

    /// The response `-runModal` returns for the first added button, `NSAlertFirstButtonReturn`
    /// from AppKit's `NSAlert.h`. The constants named `NSModalResponseOK`/`NSModalResponseCancel`
    /// in `objc2-app-kit` carry the deprecated `NSOKButton`(1)/`NSCancelButton`(0) values from
    /// `NSPanel.h` instead, which `-runModal` does not return for custom buttons, so they must
    /// not be used here (verified against the macOS SDK header, 2026-09-18).
    const ALERT_FIRST_BUTTON_RESPONSE: NSModalResponse = 1000;

    pub const CAPABILITY: &str = "input_monitoring";

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
    /// Requesting access first is what registers the product in the Input Monitoring list; opening
    /// the pane afterwards takes the user to the exact switch. ADR-0024 only allows the TCC
    /// request API inside a user-initiated setting action, which is why this runs after the prompt
    /// and never at start, on a poll or during service recovery.
    pub fn request_permission_flow() -> bool {
        let _ = request_input_monitoring_permission();
        open_input_monitoring_settings()
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
    /// reveals the running executable for the user to open its properties dialog.
    pub fn request_permission_flow() -> bool {
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

    /// Pins the two macOS dialog constraints.
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
    }

    #[test]
    fn capability_name_is_stable_and_anonymous() {
        assert!(matches!(
            STARTUP_PERMISSION_CAPABILITY,
            "input_monitoring" | "administrator"
        ));
    }
}
