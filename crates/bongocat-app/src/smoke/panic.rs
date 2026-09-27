//! A panic while the product is running, and what the next run finds.
//!
//! The environment variable names the directory the scenario owns, so a run on a
//! developer's machine does not collide with the development storage root the
//! application uses for everything else.

#[cfg(feature = "storage-test-injection")]
use super::{SmokeRoot, contains_application_event, read_application_logs};
#[cfg(feature = "storage-test-injection")]
use crate::preset_root;
#[cfg(feature = "storage-test-injection")]
use crate::write_smoke_status;
#[cfg(feature = "storage-test-injection")]
use bongocat_log::LogLevel;
#[cfg(feature = "storage-test-injection")]
use std::env;
#[cfg(feature = "storage-test-injection")]
use std::path::PathBuf;
#[cfg(feature = "storage-test-injection")]
use std::time::Duration;

#[cfg(feature = "storage-test-injection")]
pub(crate) const PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV: &str = "BONGOCAT_PANIC_DIAGNOSTICS_SMOKE_ROOT";

#[cfg(feature = "storage-test-injection")]
pub(crate) const PANIC_DIAGNOSTICS_SMOKE_PAYLOAD: &str = "panic-smoke-sensitive-payload";

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_panic_diagnostics_smoke_child() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::var_os(PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV)
        .ok_or("panic diagnostics child is missing its isolated storage root")?;
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err("panic diagnostics child storage root must be absolute".into());
    }
    let layout = StorageLayout::under(&root, BuildEnvironment::Development);
    let mut application =
        bongocat_app::Application::start_with_layout_for_smoke(layout, preset_root())?;
    application.install_process_panic_hook();
    panic!("{PANIC_DIAGNOSTICS_SMOKE_PAYLOAD}: {}", root.display());
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_panic_diagnostics_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-panic-diagnostics-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let mut child = std::process::Command::new(env::current_exe()?)
        .arg("--panic-diagnostics-smoke-child")
        .env(PANIC_DIAGNOSTICS_SMOKE_ROOT_ENV, &root.0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let timed_out = loop {
        if child.try_wait()?.is_some() {
            break false;
        }
        if std::time::Instant::now() >= deadline {
            child.kill()?;
            break true;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let child = child.wait_with_output()?;
    if timed_out {
        return Err("panic diagnostics child exceeded 10 seconds".into());
    }
    if child.status.success() {
        return Err("panic diagnostics child exited successfully".into());
    }
    let child_output = format!(
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    if child_output.contains(PANIC_DIAGNOSTICS_SMOKE_PAYLOAD)
        || child_output.contains(root.0.to_string_lossy().as_ref())
    {
        return Err("panic diagnostics child exposed its payload or storage path".into());
    }

    let run_marker = layout.logs.join("application-running.marker");
    if !run_marker.is_file() {
        return Err("panic diagnostics child did not preserve the unclean run marker".into());
    }
    let crashed_logs = read_application_logs(&layout.logs)?;
    if !contains_application_event(
        &crashed_logs,
        bongocat_app::ApplicationLogCode::Panicked,
        LogLevel::Error,
    ) {
        return Err("panic diagnostics child did not persist the stable panic record".into());
    }
    if crashed_logs.contains(PANIC_DIAGNOSTICS_SMOKE_PAYLOAD)
        || crashed_logs.contains(root.0.to_string_lossy().as_ref())
    {
        return Err("persistent panic diagnostics exposed their payload or storage path".into());
    }
    let config_after_crash = std::fs::read(&layout.config)?;

    let restarted =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let diagnostics = restarted.application_log_diagnostics();
    if diagnostics.events.previous_run_unclean != 1 || diagnostics.events.started != 1 {
        return Err("application restart did not classify the aborted run as unclean".into());
    }
    restarted.shutdown()?;
    if run_marker.exists() {
        return Err("clean restart shutdown did not remove the run marker".into());
    }
    if std::fs::read(&layout.config)? != config_after_crash {
        return Err("panic diagnostics or restart changed the current configuration".into());
    }
    let completed_logs = read_application_logs(&layout.logs)?;
    if !contains_application_event(
        &completed_logs,
        bongocat_app::ApplicationLogCode::ShutdownCompleted,
        LogLevel::Info,
    ) {
        return Err("clean restart did not persist its completed shutdown record".into());
    }

    write_smoke_status("panic diagnostics recovered after crash")?;
    root.cleanup()?;
    Ok(())
}
