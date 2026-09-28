//! A diagnostics export that succeeds, and one that fails.
//!
//! They are the same scenario with different outcomes, and the failure path is
//! the one that has to be readable: an export that fails must say why in a way
//! the support thread can act on, and the check reads the log rather than the
//! return value, because the return value is what a caller would already see.

#[cfg(feature = "storage-test-injection")]
use super::SmokeRoot;
#[cfg(feature = "storage-test-injection")]
use crate::preset_root;
#[cfg(feature = "storage-test-injection")]
use crate::write_smoke_status;
#[cfg(feature = "storage-test-injection")]
use bongocat_log::LogLevel;
#[cfg(feature = "storage-test-injection")]
use bongocat_log::LogStream;
#[cfg(feature = "storage-test-injection")]
use bongocat_log::is_log_file_name;
#[cfg(feature = "storage-test-injection")]
use bongocat_log::parse_log_line;
#[cfg(feature = "storage-test-injection")]
use std::env;
#[cfg(feature = "storage-test-injection")]
use std::io;
#[cfg(feature = "storage-test-injection")]
use std::path::Path;
#[cfg(feature = "storage-test-injection")]
use zip::ZipArchive;

#[cfg(feature = "storage-test-injection")]
pub(crate) fn read_application_logs(directory: &Path) -> io::Result<String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|name| is_log_file_name(LogStream::Application, name))
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    let mut logs = String::new();
    for path in paths {
        logs.push_str(&std::fs::read_to_string(path)?);
    }
    Ok(logs)
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn contains_application_event(
    logs: &str,
    code: bongocat_app::ApplicationLogCode,
    level: LogLevel,
) -> bool {
    logs.lines().any(|line| {
        parse_log_line(line).is_some_and(|parsed| {
            parsed.code == code.as_str()
                && parsed.module == code.component().as_str()
                && parsed.level == level
        })
    })
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_diagnostics_export_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-diagnostics-export-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let exported = client.export_diagnostics_blocking()?;
    let status = exported
        .diagnostics_export
        .ok_or("diagnostics export did not return a typed result")?;
    if status.format_version != 1
        || status.preview_bundle_format_version != 1
        || status.preview_bundle_entry_count != 3
        || status.preview_bundle_skipped_source_files != 0
        || status.bytes_written == 0
        || status.preview_bundle_bytes_written == 0
    {
        return Err("diagnostics export returned an invalid typed result".into());
    }

    let diagnostics = layout.logs.join("diagnostics.json");
    let preview = layout.logs.join("diagnostics-preview.zip");
    if !diagnostics.is_file() || !preview.is_file() {
        return Err("diagnostics export did not create both private files".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&diagnostics)?.permissions().mode() & 0o777 != 0o600
            || std::fs::metadata(&preview)?.permissions().mode() & 0o777 != 0o600
        {
            return Err("diagnostics export did not preserve private file permissions".into());
        }
    }
    let mut archive = ZipArchive::new(std::fs::File::open(&preview)?)?;
    let mut entries = (0..archive.len())
        .map(|index| archive.by_index(index).map(|entry| entry.name().to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    if entries
        != [
            "application-events.log",
            "diagnostics.json",
            "manifest.json",
        ]
    {
        return Err("diagnostics preview archive entries diverged from the v1 contract".into());
    }
    let mut application_events = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("application-events.log")?,
        &mut application_events,
    )?;
    if !application_events
        .lines()
        .any(|line| line == "INFO  [application] application/started")
    {
        return Err(
            "diagnostics preview did not contain the canonical application start event".into(),
        );
    }

    client.shutdown_blocking()?;
    service.join()?;
    root.cleanup()?;
    write_smoke_status("diagnostics export completed with a private preview bundle")?;
    Ok(())
}

#[cfg(feature = "storage-test-injection")]
pub(crate) fn run_diagnostics_export_failure_smoke() -> Result<(), Box<dyn std::error::Error>> {
    use bongocat_config::{BuildEnvironment, StorageLayout};

    let root = env::temp_dir().join(format!(
        "bongocat-diagnostics-export-failure-smoke-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let root = SmokeRoot(root);
    let layout = StorageLayout::under(&root.0, BuildEnvironment::Development);
    let application =
        bongocat_app::Application::start_with_layout_for_smoke(layout.clone(), preset_root())?;
    let service = bongocat_app::ApplicationSettingsService::start(application)?;
    let client = service.client();
    let first = client.export_diagnostics_blocking()?;
    let first_status = first
        .diagnostics_export
        .ok_or("initial diagnostics export did not return a typed result")?;
    let diagnostics = layout.logs.join("diagnostics.json");
    let preview = layout.logs.join("diagnostics-preview.zip");
    let previous_diagnostics = std::fs::read(&diagnostics)?;
    let previous_preview = std::fs::read(&preview)?;

    // A directory at the destination is an OS-level replace/open failure. The writer must
    // reject it before touching the existing diagnostics or preview bytes.
    std::fs::remove_file(&diagnostics)?;
    std::fs::create_dir(&diagnostics)?;
    let error = client
        .export_diagnostics_blocking()
        .expect_err("diagnostics export must reject a directory destination");
    if error.code() != bongocat_ui_protocol::SettingsErrorCode::DiagnosticsExportFailed {
        return Err("diagnostics export returned an unstable filesystem failure code".into());
    }
    if std::fs::read(&preview)? != previous_preview {
        return Err("failed diagnostics export changed the previous preview bundle".into());
    }
    std::fs::remove_dir(&diagnostics)?;
    std::fs::write(&diagnostics, &previous_diagnostics)?;

    #[cfg(target_os = "macos")]
    {
        // Marking the existing preview bundle immutable makes the atomic commit fail after the
        // staging file has been fully written. Unlike a directory or a read-only parent, this is
        // an OS-level failure the current process cannot bypass through `set_private_directory`,
        // so it deterministically exercises the commit-failure recovery path.
        let immutable = std::process::Command::new("/usr/bin/chflags")
            .arg("uchg")
            .arg(&preview)
            .status()?;
        if !immutable.success() {
            return Err("failed to mark the previous preview bundle immutable".into());
        }
        let result = client.export_diagnostics_blocking();
        let cleared = std::process::Command::new("/usr/bin/chflags")
            .arg("nouchg")
            .arg(&preview)
            .status()?;
        if !cleared.success() {
            return Err("failed to clear the immutable preview bundle flag".into());
        }
        let error =
            result.expect_err("diagnostics export must reject an immutable preview destination");
        if error.code() != bongocat_ui_protocol::SettingsErrorCode::DiagnosticsExportFailed {
            return Err("immutable diagnostics preview returned an unstable error code".into());
        }
        if std::fs::read(&preview)? != previous_preview {
            return Err("preview commit failure changed the previous preview bundle".into());
        }
    }

    let staging = std::fs::read_dir(&layout.logs)?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with(".diagnostics-preview.zip."))
        .collect::<Vec<_>>();
    if !staging.is_empty() {
        return Err(format!("failed export left staging files: {staging:?}").into());
    }
    if first_status.preview_bundle_entry_count != 3 {
        return Err("initial diagnostics export returned an invalid preview status".into());
    }

    client.shutdown_blocking()?;
    service.join()?;
    root.cleanup()?;
    write_smoke_status("diagnostics export filesystem failures preserved the previous bundle")?;
    Ok(())
}
