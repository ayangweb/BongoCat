//! Handing a link to the operating system.
//!
//! Every "open this address" control in the product goes through here: the
//! About page's project and feedback rows, the update window's release-page
//! button, and every link in the release notes. They share this module because
//! they share one hazard, which is invisible until it kills the process.
//!
//! # Why the hand-off cannot happen inside the callback
//!
//! A GPUI button or link callback runs while the framework holds a mutable
//! borrow of its `App`, which is a `RefCell`. On Windows, opening a URL means
//! `ShellExecuteW`, and `ShellExecuteW` **pumps this process's message queue**
//! before it returns. Pumping delivers the messages GPUI's own platform window
//! handles, and handling them runs its foreground tasks — on this thread, inside
//! the callback, with that borrow still held.
//!
//! Those tasks call `AsyncApp::update`, which asks the same `RefCell` for
//! another mutable borrow. `RefCell` grants one, so the second one panics with
//! `RefCell already borrowed` and the process dies on the way out. The product's
//! own long-lived pollers in `bongocat-app` are among those tasks, which is what
//! makes the trap so easy to fall into: any of them would do it.
//!
//! The freeze users saw before the crash is the same call, blocking on the
//! browser while the window cannot paint.
//!
//! macOS runs the `open` command instead of `ShellExecuteW` and pumps nothing,
//! which is why the identical button was fine there.
//!
//! # The rule
//!
//! The blocking platform call runs on a background executor, so no message pump
//! happens on the thread that owns the borrow. Only the outcome comes back.
//!
//! `ShellExecuteW` is the only message-pumping call the product makes from a
//! view callback. `opener::reveal` spawns its own worker thread, and the
//! clipboard writes do not pump; the directory launches happen on the settings
//! worker, off the UI thread entirely.

use bongocat_platform::ExternalUrlOpenError;
use gpui_kit::{BackgroundExecutor, Task};

use crate::{SettingsError, SettingsErrorCode};

/// Hand an HTTPS URL to the operating system, off the GUI thread.
///
/// The caller supplies the executor because a view callback has one and a render
/// helper does not, and because the returned [`Task`] is what lets the About page
/// report a failure back into its own view.
///
/// The platform adapter re-validates the scheme, so a URL arriving from a parsed
/// manifest is refused exactly like a product-owned constant; this decides
/// nothing about what may be opened.
pub(crate) fn open(url: String, executor: &BackgroundExecutor) -> Task<Result<(), SettingsError>> {
    open_with(url, executor, bongocat_platform::open_external_url)
}

/// [`Self::open`] with the launch injected, so the scheduling this module exists
/// to guarantee can be asserted without a test run opening a browser.
pub(crate) fn open_with(
    url: String,
    executor: &BackgroundExecutor,
    launch: impl FnOnce(&str) -> Result<(), ExternalUrlOpenError> + Send + 'static,
) -> Task<Result<(), SettingsError>> {
    executor.spawn(async move {
        launch(&url).map_err(|_| SettingsError::new(SettingsErrorCode::ExternalLinkOpenFailed))
    })
}
