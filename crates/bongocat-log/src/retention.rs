//! When yesterday's log is deleted.
//!
//! Retention is bounded on both axes — a number of days and a total size —
//! because either alone lets one axis grow without limit. Only files this logger
//! wrote are eligible: an unknown file in the directory, and a symlink, are left
//! alone rather than deleted, so pointing the log at the wrong directory costs
//! disk and not the directory.

use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextLogStats {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub active_bytes: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetentionReport {
    pub pruned: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct LogFile {
    pub(crate) path: PathBuf,
    pub(crate) stream: LogStream,
    pub(crate) date: UtcDate,
    pub(crate) generation: Option<u64>,
    pub(crate) bytes: u64,
    pub(crate) modified: SystemTime,
    pub(crate) current: bool,
}

pub(crate) struct RetentionSweep {
    pub(crate) report: RetentionReport,
    pub(crate) application_pruned: u64,
    pub(crate) core_pruned: u64,
}

/// Enforce the shared log-directory budget without reading log contents.
///
/// Only the two known product-owned `.log` naming schemes are considered.
/// Unknown files and symlinks are ignored. Current UTC-day active files are
/// retained; expired or budget-breaking rotated files are removed oldest first.
pub fn enforce_directory_retention(
    directory: &Path,
    now: SystemTime,
    retention_days: u64,
) -> RetentionReport {
    enforce_directory_retention_sweep(directory, now, retention_days).report
}

pub(crate) fn enforce_directory_retention_sweep(
    directory: &Path,
    now: SystemTime,
    retention_days: u64,
) -> RetentionSweep {
    let today = UtcDate::from_system_time(now);
    let expiration = now.checked_sub(Duration::from_secs(
        retention_days.saturating_mul(SECONDS_PER_DAY),
    ));
    let mut application_pruned = 0_u64;
    let mut core_pruned = 0_u64;
    let mut files = collect_log_files(directory, today);

    files.retain(|file| {
        let expired = expiration.is_some_and(|deadline| file.modified < deadline);
        if expired && !file.current && fs::remove_file(&file.path).is_ok() {
            match file.stream {
                LogStream::Application => application_pruned = application_pruned.saturating_add(1),
                LogStream::CubismCore => core_pruned = core_pruned.saturating_add(1),
            }
            false
        } else {
            true
        }
    });

    let mut total_bytes = files
        .iter()
        .fold(0_u64, |total, file| total.saturating_add(file.bytes));
    files.sort_by_key(|file| (file.modified, file.path.clone()));
    let mut kept = Vec::with_capacity(files.len());
    for file in files {
        let over_budget =
            total_bytes > MAX_TOTAL_LOG_BYTES || kept.len() as u64 + 1 > MAX_TOTAL_LOG_FILES;
        if over_budget && !file.current && fs::remove_file(&file.path).is_ok() {
            total_bytes = total_bytes.saturating_sub(file.bytes);
            match file.stream {
                LogStream::Application => application_pruned = application_pruned.saturating_add(1),
                LogStream::CubismCore => core_pruned = core_pruned.saturating_add(1),
            }
        } else {
            kept.push(file);
        }
    }

    let remaining = collect_log_files(directory, today);
    let report = RetentionReport {
        pruned: application_pruned.saturating_add(core_pruned),
        retained_files: remaining.len() as u64,
        retained_bytes: remaining
            .iter()
            .fold(0_u64, |total, file| total.saturating_add(file.bytes)),
    };
    RetentionSweep {
        report,
        application_pruned,
        core_pruned,
    }
}

pub(crate) fn collect_log_files(directory: &Path, today: UtcDate) -> Vec<LogFile> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.file_type().is_file() {
                return None;
            }
            let (stream, date, generation) = parse_log_file_name(&path)?;
            let current = generation.is_none() && date == today;
            Some(LogFile {
                path,
                stream,
                date,
                generation,
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                current,
            })
        })
        .collect()
}

/// Return whether `file_name` exactly belongs to the requested product-owned
/// text-log naming scheme. Unknown files and legacy formats return `false`.
pub fn is_log_file_name(stream: LogStream, file_name: &str) -> bool {
    parse_log_file_name(Path::new(file_name)).is_some_and(|(parsed, _, _)| parsed == stream)
}

pub(crate) fn parse_log_file_name(path: &Path) -> Option<(LogStream, UtcDate, Option<u64>)> {
    let name = path.file_name()?.to_str()?;
    for stream in [LogStream::Application, LogStream::CubismCore] {
        let Some(rest) = name.strip_prefix(&format!("{}-", stream.prefix())) else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(".log") else {
            continue;
        };
        if let Some((date, generation)) = rest.rsplit_once('.') {
            let generation = generation.parse::<u64>().ok()?;
            if generation == 0 {
                return None;
            }
            return UtcDate::parse(date).map(|date| (stream, date, Some(generation)));
        }
        return UtcDate::parse(rest).map(|date| (stream, date, None));
    }
    None
}

pub(crate) fn retire_other_day_bases(directory: &Path, stream: LogStream, current: UtcDate) -> u64 {
    let candidates = collect_log_files(directory, current)
        .into_iter()
        .filter(|file| file.stream == stream && file.generation.is_none() && file.date != current)
        .collect::<Vec<_>>();
    let mut rotated = 0_u64;
    for file in candidates {
        if rotate_path(directory, stream, &file.path).is_ok() {
            rotated = rotated.saturating_add(1);
        }
    }
    rotated
}
