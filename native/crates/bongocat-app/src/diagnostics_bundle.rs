#![forbid(unsafe_code)]

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const APPLICATION_LOG_PREFIX: &str = "application-";
const APPLICATION_LOG_SUFFIX: &str = ".jsonl";
const APPLICATION_EVENTS_ENTRY: &str = "application-events.jsonl";
const DIAGNOSTICS_ENTRY: &str = "diagnostics.json";
const MANIFEST_ENTRY: &str = "manifest.json";
const PREVIEW_BUNDLE_NAME: &str = "diagnostics-preview.zip";
pub(crate) const PREVIEW_BUNDLE_FORMAT_VERSION: u32 = 1;
pub(crate) const PREVIEW_BUNDLE_ENTRY_COUNT: u32 = 3;
const MAX_APPLICATION_LOG_FILES: usize = 8;
const MAX_APPLICATION_LOG_BYTES: u64 = 1024 * 1024;
const MAX_EVENT_LINE_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreviewBundleStatus {
    pub format_version: u32,
    pub bytes_written: u64,
    pub entry_count: u32,
    pub application_event_count: u64,
    pub skipped_source_files: u64,
}

#[derive(Debug)]
pub(crate) struct PreviewBundleError;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceEvent {
    component: String,
    level: String,
    code: String,
}

#[derive(Serialize)]
struct PreviewManifest {
    schema_version: u32,
    diagnostics_entry: &'static str,
    application_events_entry: &'static str,
    application_event_count: u64,
    skipped_source_files: u64,
}

pub(crate) fn write_preview_bundle(
    directory: &Path,
    diagnostics_json: &[u8],
) -> Result<PreviewBundleStatus, PreviewBundleError> {
    let (events, skipped_source_files) = collect_application_events(directory);
    let manifest = serde_json::to_vec(&PreviewManifest {
        schema_version: PREVIEW_BUNDLE_FORMAT_VERSION,
        diagnostics_entry: DIAGNOSTICS_ENTRY,
        application_events_entry: APPLICATION_EVENTS_ENTRY,
        application_event_count: events.len() as u64,
        skipped_source_files,
    })
    .map_err(|_| PreviewBundleError)?;
    let event_bytes = serialize_events(&events)?;
    let archive_bytes = write_archive(&manifest, diagnostics_json, &event_bytes)?;
    verify_archive(&archive_bytes)?;

    let path = directory.join(PREVIEW_BUNDLE_NAME);
    write_private_atomic(&path, &archive_bytes)?;
    Ok(PreviewBundleStatus {
        format_version: PREVIEW_BUNDLE_FORMAT_VERSION,
        bytes_written: archive_bytes.len() as u64,
        entry_count: PREVIEW_BUNDLE_ENTRY_COUNT,
        application_event_count: events.len() as u64,
        skipped_source_files,
    })
}

fn collect_application_events(directory: &Path) -> (Vec<SourceEvent>, u64) {
    let mut paths = fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_application_log_path(path))
        .collect::<Vec<_>>();
    paths.sort_unstable();

    let mut events = Vec::new();
    let mut skipped = paths.len().saturating_sub(MAX_APPLICATION_LOG_FILES) as u64;
    for path in paths.drain(..).take(MAX_APPLICATION_LOG_FILES) {
        match parse_application_log(&path) {
            Ok(mut parsed) => events.append(&mut parsed),
            Err(()) => skipped = skipped.saturating_add(1),
        }
    }
    (events, skipped)
}

fn is_application_log_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let valid_name = name
        .strip_prefix(APPLICATION_LOG_PREFIX)
        .is_some_and(|suffix| {
            suffix
                .strip_suffix(APPLICATION_LOG_SUFFIX)
                .is_some_and(|day| !day.is_empty() && day.bytes().all(|byte| byte.is_ascii_digit()))
                || suffix
                    .split_once(".jsonl.")
                    .is_some_and(|(day, generation)| {
                        !day.is_empty()
                            && day.bytes().all(|byte| byte.is_ascii_digit())
                            && !generation.is_empty()
                            && generation.bytes().all(|byte| byte.is_ascii_digit())
                    })
        });
    valid_name
        && fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn parse_application_log(path: &Path) -> Result<Vec<SourceEvent>, ()> {
    let bytes = fs::read(path).map_err(|_| ())?;
    if bytes.len() as u64 > MAX_APPLICATION_LOG_BYTES {
        return Err(());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| ())?;
    let mut events = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.len() > MAX_EVENT_LINE_BYTES {
            return Err(());
        }
        let event = serde_json::from_str::<SourceEvent>(line).map_err(|_| ())?;
        if !is_valid_event(&event) {
            return Err(());
        }
        events.push(event);
    }
    Ok(events)
}

fn is_valid_event(event: &SourceEvent) -> bool {
    matches!(
        event.component.as_str(),
        "application" | "configuration" | "input" | "model" | "renderer" | "runtime" | "settings"
    ) && matches!(event.level.as_str(), "info" | "warn" | "error")
        && matches!(
            event.code.as_str(),
            "started"
                | "previous_run_unclean"
                | "shutdown_started"
                | "shutdown_completed"
                | "shutdown_failed"
                | "panicked"
                | "runtime_unavailable"
                | "diagnostics_export_failed"
        )
}

fn serialize_events(events: &[SourceEvent]) -> Result<Vec<u8>, PreviewBundleError> {
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, event).map_err(|_| PreviewBundleError)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn write_archive(
    manifest: &[u8],
    diagnostics_json: &[u8],
    application_events: &[u8],
) -> Result<Vec<u8>, PreviewBundleError> {
    let cursor = Cursor::new(Vec::new());
    let mut archive = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in [
        (MANIFEST_ENTRY, manifest),
        (DIAGNOSTICS_ENTRY, diagnostics_json),
        (APPLICATION_EVENTS_ENTRY, application_events),
    ] {
        archive
            .start_file(name, options)
            .map_err(|_| PreviewBundleError)?;
        archive.write_all(bytes).map_err(|_| PreviewBundleError)?;
    }
    archive
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|_| PreviewBundleError)
}

fn verify_archive(bytes: &[u8]) -> Result<(), PreviewBundleError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| PreviewBundleError)?;
    if archive.len() != PREVIEW_BUNDLE_ENTRY_COUNT as usize {
        return Err(PreviewBundleError);
    }
    for name in [MANIFEST_ENTRY, DIAGNOSTICS_ENTRY, APPLICATION_EVENTS_ENTRY] {
        let file = archive.by_name(name).map_err(|_| PreviewBundleError)?;
        if file.is_dir() || file.compression() != CompressionMethod::Stored {
            return Err(PreviewBundleError);
        }
    }
    Ok(())
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), PreviewBundleError> {
    #[cfg(unix)]
    let options = {
        use atomic_write_file::unix::OpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut options = AtomicWriteFile::options();
        options.preserve_mode(false).mode(0o600);
        options
    };
    #[cfg(not(unix))]
    let options = AtomicWriteFile::options();
    let mut file = options.open(path).map_err(|_| PreviewBundleError)?;
    file.write_all(bytes).map_err(|_| PreviewBundleError)?;
    file.commit().map_err(|_| PreviewBundleError)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| PreviewBundleError)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn bundle_reserializes_only_fixed_application_records() {
        let directory = tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("application-1.jsonl"),
            b"{\"component\":\"application\",\"level\":\"info\",\"code\":\"started\"}\n",
        )
        .expect("application log");
        fs::write(directory.path().join("core.jsonl"), b"private core message").expect("core log");

        let status = write_preview_bundle(directory.path(), b"{\"format_version\":1}")
            .expect("preview bundle");
        assert_eq!(status.application_event_count, 1);
        assert_eq!(status.skipped_source_files, 0);
        let bytes = fs::read(directory.path().join(PREVIEW_BUNDLE_NAME)).expect("bundle bytes");
        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("read bundle");
        let mut events = String::new();
        std::io::Read::read_to_string(
            &mut archive
                .by_name(APPLICATION_EVENTS_ENTRY)
                .expect("event entry"),
            &mut events,
        )
        .expect("event text");
        assert!(events.contains("started"));
        assert!(!events.contains("private core message"));
    }

    #[test]
    fn bundle_skips_invalid_application_logs_without_copying_their_contents() {
        let directory = tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("application-1.jsonl"),
            b"{\"component\":\"application\",\"level\":\"info\",\"code\":\"started\",\"private\":\"model-name\"}\n",
        )
        .expect("invalid application log");

        let status = write_preview_bundle(directory.path(), b"{\"format_version\":1}")
            .expect("preview bundle");
        assert_eq!(status.application_event_count, 0);
        assert_eq!(status.skipped_source_files, 1);
        let bytes = fs::read(directory.path().join(PREVIEW_BUNDLE_NAME)).expect("bundle bytes");
        assert!(!String::from_utf8_lossy(&bytes).contains("model-name"));
    }
}
