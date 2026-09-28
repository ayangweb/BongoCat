//! The standalone automated-verification modes of the product binary.
//!
//! Each function here is a whole process: the entry point recognizes its flag
//! and returns before the product assembles a single window, because what it
//! checks is a subsystem on its own: the login-startup registration, the settings
//! window's persisted bounds, the diagnostics export, or what a crash leaves
//! behind. Every one of them either verifies an assertion and exits or reports
//! what it observed and fails; none of them starts the product.
//!
//! The scenarios that need the running product (the settings window pages, the
//! system menu, the overlay frame loop) stay in `main.rs`, because they drive the
//! coordinator and the run loop this binary exists to host.

mod diagnostics;
mod panic;
mod settings_window;
mod startup_item;

// Each scenario is self-contained: it opens a real window, waits for a real
// frame and asserts on a real log, so what they share is a shape rather than a
// value. A scenario is re-exported under the gate it was written behind rather
// than under one gate for the module, because they are not the same gate: the
// login item is macOS-only and the rest need the injection feature.
#[cfg(feature = "storage-test-injection")]
pub(super) use diagnostics::{contains_application_event, read_application_logs};
#[cfg(feature = "storage-test-injection")]
pub(crate) use diagnostics::{run_diagnostics_export_failure_smoke, run_diagnostics_export_smoke};
#[cfg(feature = "storage-test-injection")]
pub(crate) use panic::{run_panic_diagnostics_smoke, run_panic_diagnostics_smoke_child};
#[cfg(feature = "storage-test-injection")]
pub(crate) use settings_window::run_settings_window_state_smoke;
#[cfg(feature = "storage-test-injection")]
pub(super) use startup_item::SmokeRoot;
#[cfg(target_os = "macos")]
pub(crate) use startup_item::run_startup_item_smoke;
pub(crate) use startup_item::run_startup_permission_smoke;
