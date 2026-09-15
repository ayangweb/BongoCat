#![forbid(unsafe_code)]

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cell::Cell;
use std::{
    fs,
    io::{Cursor, ErrorKind, Read, Write},
    path::Path,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const APPLICATION_LOG_PREFIX: &str = "application-";
const APPLICATION_LOG_SUFFIX: &str = ".jsonl";
const APPLICATION_EVENTS_ENTRY: &str = "application-events.jsonl";
const DIAGNOSTICS_ENTRY: &str = "diagnostics.json";
const MANIFEST_ENTRY: &str = "manifest.json";
const PREVIEW_BUNDLE_ENTRIES: [&str; 3] =
    [MANIFEST_ENTRY, DIAGNOSTICS_ENTRY, APPLICATION_EVENTS_ENTRY];
const PREVIEW_BUNDLE_NAME: &str = "diagnostics-preview.zip";
pub(crate) const PREVIEW_BUNDLE_FORMAT_VERSION: u32 = 1;
pub(crate) const PREVIEW_BUNDLE_ENTRY_COUNT: u32 = PREVIEW_BUNDLE_ENTRIES.len() as u32;
const MAX_APPLICATION_LOG_FILES: usize = 8;
const MAX_APPLICATION_LOG_BYTES: u64 = 1024 * 1024;
const MAX_APPLICATION_EVENTS_BYTES: u64 =
    MAX_APPLICATION_LOG_FILES as u64 * MAX_APPLICATION_LOG_BYTES;
const MAX_DIAGNOSTICS_JSON_BYTES: u64 = 1024 * 1024;
const MAX_PREVIEW_BUNDLE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_EVENT_LINE_BYTES: usize = 1024;

#[cfg(test)]
#[derive(Clone, Copy, Eq, PartialEq)]
enum WriteFailurePoint {
    AfterOpen,
    BeforeCommit,
    ReplaceTargetWithDirectory,
}

#[cfg(test)]
thread_local! {
    static WRITE_FAILURE_POINT: Cell<Option<WriteFailurePoint>> = const { Cell::new(None) };
}

#[cfg(test)]
struct WriteFailureGuard;

#[cfg(test)]
impl Drop for WriteFailureGuard {
    fn drop(&mut self) {
        WRITE_FAILURE_POINT.with(|point| point.set(None));
    }
}

#[cfg(test)]
fn inject_write_failure(point: WriteFailurePoint, path: &Path) -> Result<(), PreviewBundleError> {
    WRITE_FAILURE_POINT.with(|configured| {
        if configured.get() == Some(point) {
            match point {
                WriteFailurePoint::ReplaceTargetWithDirectory => {
                    fs::remove_file(path).map_err(|_| PreviewBundleError)?;
                    fs::create_dir(path).map_err(|_| PreviewBundleError)
                }
                WriteFailurePoint::AfterOpen | WriteFailurePoint::BeforeCommit => {
                    Err(PreviewBundleError)
                }
            }
        } else {
            Ok(())
        }
    })
}

#[cfg(test)]
fn fail_atomic_write_at(point: WriteFailurePoint) -> WriteFailureGuard {
    WRITE_FAILURE_POINT.with(|configured| configured.set(Some(point)));
    WriteFailureGuard
}

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedPreviewManifest {
    schema_version: u32,
    diagnostics_entry: String,
    application_events_entry: String,
    application_event_count: u64,
    #[serde(rename = "skipped_source_files")]
    _skipped_source_files: u64,
}

pub(crate) fn write_preview_bundle(
    directory: &Path,
    diagnostics_json: &[u8],
) -> Result<PreviewBundleStatus, PreviewBundleError> {
    if diagnostics_json.len() as u64 > MAX_DIAGNOSTICS_JSON_BYTES {
        return Err(PreviewBundleError);
    }
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
    if archive_bytes.len() as u64 > MAX_PREVIEW_BUNDLE_BYTES {
        return Err(PreviewBundleError);
    }
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
        if bytes.len() as u64 > MAX_APPLICATION_EVENTS_BYTES {
            return Err(PreviewBundleError);
        }
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
    if bytes.len() as u64 > MAX_PREVIEW_BUNDLE_BYTES {
        return Err(PreviewBundleError);
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| PreviewBundleError)?;
    if archive.len() != PREVIEW_BUNDLE_ENTRY_COUNT as usize {
        return Err(PreviewBundleError);
    }

    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| PreviewBundleError)?;
        if file.is_dir() || file.compression() != CompressionMethod::Stored {
            return Err(PreviewBundleError);
        }
        names.push(file.name().to_owned());
    }
    names.sort_unstable();
    let mut expected = PREVIEW_BUNDLE_ENTRIES.map(str::to_owned);
    expected.sort_unstable();
    if names != expected {
        return Err(PreviewBundleError);
    }

    let mut manifest_bytes = Vec::new();
    archive
        .by_name(MANIFEST_ENTRY)
        .map_err(|_| PreviewBundleError)?
        .read_to_end(&mut manifest_bytes)
        .map_err(|_| PreviewBundleError)?;
    let manifest = serde_json::from_slice::<VerifiedPreviewManifest>(&manifest_bytes)
        .map_err(|_| PreviewBundleError)?;
    if manifest.schema_version != PREVIEW_BUNDLE_FORMAT_VERSION
        || manifest.diagnostics_entry != DIAGNOSTICS_ENTRY
        || manifest.application_events_entry != APPLICATION_EVENTS_ENTRY
    {
        return Err(PreviewBundleError);
    }

    let mut diagnostics = Vec::new();
    archive
        .by_name(DIAGNOSTICS_ENTRY)
        .map_err(|_| PreviewBundleError)?
        .read_to_end(&mut diagnostics)
        .map_err(|_| PreviewBundleError)?;
    if diagnostics.len() as u64 > MAX_DIAGNOSTICS_JSON_BYTES
        || !serde_json::from_slice::<serde_json::Value>(&diagnostics)
            .map_err(|_| PreviewBundleError)?
            .is_object()
    {
        return Err(PreviewBundleError);
    }

    let mut event_bytes = Vec::new();
    archive
        .by_name(APPLICATION_EVENTS_ENTRY)
        .map_err(|_| PreviewBundleError)?
        .read_to_end(&mut event_bytes)
        .map_err(|_| PreviewBundleError)?;
    if event_bytes.len() as u64 > MAX_APPLICATION_EVENTS_BYTES
        || verified_event_count(&event_bytes)? != manifest.application_event_count
    {
        return Err(PreviewBundleError);
    }
    Ok(())
}

fn verified_event_count(bytes: &[u8]) -> Result<u64, PreviewBundleError> {
    let text = std::str::from_utf8(bytes).map_err(|_| PreviewBundleError)?;
    let mut count = 0_u64;
    for line in text.lines() {
        if line.is_empty() || line.len() > MAX_EVENT_LINE_BYTES {
            return Err(PreviewBundleError);
        }
        let event = serde_json::from_str::<SourceEvent>(line).map_err(|_| PreviewBundleError)?;
        if !is_valid_event(&event) {
            return Err(PreviewBundleError);
        }
        count = count.saturating_add(1);
    }
    Ok(count)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), PreviewBundleError> {
    ensure_regular_or_missing_target(path)?;
    let result = (|| {
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
        #[cfg(test)]
        inject_write_failure(WriteFailurePoint::AfterOpen, path)?;
        file.write_all(bytes).map_err(|_| PreviewBundleError)?;
        #[cfg(test)]
        inject_write_failure(WriteFailurePoint::BeforeCommit, path)?;
        #[cfg(test)]
        inject_write_failure(WriteFailurePoint::ReplaceTargetWithDirectory, path)?;
        file.commit().map_err(|_| PreviewBundleError)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| PreviewBundleError)?;
        }
        Ok(())
    })();
    if result.is_err() {
        cleanup_preview_staging(path.parent().ok_or(PreviewBundleError)?)?;
    }
    result
}

fn cleanup_preview_staging(directory: &Path) -> Result<(), PreviewBundleError> {
    for entry in fs::read_dir(directory).map_err(|_| PreviewBundleError)? {
        let entry = entry.map_err(|_| PreviewBundleError)?;
        let name = entry.file_name();
        if !is_preview_staging_name(&name) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| PreviewBundleError)?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            fs::remove_file(path).map_err(|_| PreviewBundleError)?;
        }
    }
    Ok(())
}

fn is_preview_staging_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let prefix = format!(".{PREVIEW_BUNDLE_NAME}.");
    let Some(suffix) = name.strip_prefix(&prefix) else {
        return false;
    };
    suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn ensure_regular_or_missing_target(path: &Path) -> Result<(), PreviewBundleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(PreviewBundleError),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(PreviewBundleError),
    }
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

    #[test]
    fn failed_atomic_write_preserves_the_previous_bundle_and_cleans_staging() {
        let directory = tempdir().expect("temporary directory");
        write_preview_bundle(directory.path(), b"{\"format_version\":1}")
            .expect("initial preview bundle");
        let path = directory.path().join(PREVIEW_BUNDLE_NAME);
        let previous = fs::read(&path).expect("previous preview bundle");

        for point in [
            WriteFailurePoint::AfterOpen,
            WriteFailurePoint::BeforeCommit,
        ] {
            let guard = fail_atomic_write_at(point);
            assert!(write_preview_bundle(directory.path(), b"{\"format_version\":2}").is_err());
            drop(guard);

            assert_eq!(fs::read(&path).expect("preserved preview bundle"), previous);
            let staging = fs::read_dir(directory.path())
                .expect("bundle directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .filter_map(|name| name.into_string().ok())
                .filter(|name| name.starts_with(".diagnostics-preview.zip."))
                .collect::<Vec<_>>();
            assert!(
                staging.is_empty(),
                "temporary preview files remain: {staging:?}"
            );
        }
    }

    #[test]
    fn replace_failure_cleans_atomic_write_staging() {
        let directory = tempdir().expect("temporary directory");
        write_preview_bundle(directory.path(), b"{\"format_version\":1}")
            .expect("initial preview bundle");
        let path = directory.path().join(PREVIEW_BUNDLE_NAME);
        let guard = fail_atomic_write_at(WriteFailurePoint::ReplaceTargetWithDirectory);

        assert!(write_preview_bundle(directory.path(), b"{\"format_version\":2}").is_err());
        drop(guard);

        assert!(
            path.is_dir(),
            "test hook must force the atomic replace to fail"
        );
        let staging = fs::read_dir(directory.path())
            .expect("bundle directory")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter_map(|name| name.into_string().ok())
            .filter(|name| name.starts_with(".diagnostics-preview.zip."))
            .collect::<Vec<_>>();
        assert!(
            staging.is_empty(),
            "temporary preview files remain after a failed replace: {staging:?}"
        );
    }

    #[test]
    fn staging_cleanup_preserves_non_staging_files_with_the_bundle_prefix() {
        let directory = tempdir().expect("temporary directory");
        let unrelated = directory.path().join(".diagnostics-preview.zip.user-notes");
        fs::write(&unrelated, b"must remain").expect("unrelated file");
        let staging = directory.path().join(".diagnostics-preview.zip.A1b2C3");
        fs::write(&staging, b"staging").expect("staging file");

        cleanup_preview_staging(directory.path()).expect("staging cleanup");

        assert_eq!(
            fs::read(&unrelated).expect("unrelated file remains"),
            b"must remain"
        );
        assert!(
            !staging.exists(),
            "atomic-write staging file must be removed"
        );
    }

    #[test]
    fn archive_verification_rejects_an_invalid_manifest() {
        let archive = write_archive(b"{}", b"{}", b"").expect("archive bytes");
        assert!(verify_archive(&archive).is_err());
    }

    #[test]
    fn bundle_rejects_an_oversized_diagnostics_document_before_writing() {
        let directory = tempdir().expect("temporary directory");
        let diagnostics = vec![b'0'; MAX_DIAGNOSTICS_JSON_BYTES as usize + 1];

        assert!(write_preview_bundle(directory.path(), &diagnostics).is_err());
        assert!(!directory.path().join(PREVIEW_BUNDLE_NAME).exists());
    }

    #[cfg(unix)]
    #[test]
    fn bundle_rejects_a_symlink_target_without_touching_its_destination() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temporary directory");
        let destination = directory.path().join("outside-preview.zip");
        fs::write(&destination, b"outside bytes").expect("outside preview");
        let preview = directory.path().join(PREVIEW_BUNDLE_NAME);
        symlink(&destination, &preview).expect("preview symlink");

        assert!(write_preview_bundle(directory.path(), b"{\"format_version\":1}").is_err());
        assert_eq!(
            fs::read(&destination).expect("outside bytes"),
            b"outside bytes"
        );
        assert!(
            fs::symlink_metadata(&preview)
                .expect("preview metadata")
                .file_type()
                .is_symlink()
        );
    }
}
