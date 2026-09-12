#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

pub const RETENTION_DAYS: u64 = 7;
pub const MAX_TOTAL_LOG_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetentionReport {
    pub pruned: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}

#[derive(Clone, Debug)]
struct LogFile {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

/// Enforce the shared log-directory budget without exposing log contents.
///
/// Only the two known JSONL naming schemes are considered. Active files are
/// retained so a writer can continue appending; each writer independently
/// bounds active-file size and age/rotation count before calling this helper.
pub fn enforce_directory_retention(
    directory: &Path,
    active_path: Option<&Path>,
    now: SystemTime,
) -> RetentionReport {
    let mut files = collect_log_files(directory);
    let expiration = now.checked_sub(Duration::from_secs(RETENTION_DAYS.saturating_mul(86_400)));
    let mut pruned = 0;

    files.retain(|file| {
        let expired = expiration.is_some_and(|deadline| file.modified < deadline);
        let is_active =
            active_path.is_some_and(|path| path == file.path) || is_active_path(&file.path);
        if expired && !is_active && fs::remove_file(&file.path).is_ok() {
            pruned += 1;
            false
        } else {
            true
        }
    });

    let mut total_bytes = files.iter().map(|file| file.bytes).sum::<u64>();

    files.sort_by_key(|file| file.modified);
    for file in &files {
        let is_active =
            active_path.is_some_and(|path| path == file.path) || is_active_path(&file.path);
        if total_bytes <= MAX_TOTAL_LOG_BYTES || is_active {
            continue;
        }
        if fs::remove_file(&file.path).is_ok() {
            total_bytes = total_bytes.saturating_sub(file.bytes);
            pruned += 1;
        }
    }

    let remaining = collect_log_files(directory);
    RetentionReport {
        pruned,
        retained_files: remaining.len() as u64,
        retained_bytes: remaining.iter().map(|file| file.bytes).sum::<u64>(),
    }
}

fn collect_log_files(directory: &Path) -> Vec<LogFile> {
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
            let name = path.file_name()?.to_str()?;
            let known = is_active_path(&path)
                || name
                    .strip_prefix("cubism-core.jsonl.")
                    .is_some_and(|generation| generation.parse::<u32>().is_ok())
                || name
                    .strip_prefix("application-")
                    .and_then(|rest| rest.split_once(".jsonl."))
                    .is_some_and(|(day, generation)| {
                        day.parse::<u64>().is_ok() && generation.parse::<usize>().is_ok()
                    });
            if !known {
                return None;
            }
            Some(LogFile {
                path,
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            })
        })
        .collect()
}

fn is_active_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name == "cubism-core.jsonl"
        || name
            .strip_prefix("application-")
            .and_then(|rest| rest.strip_suffix(".jsonl"))
            .is_some_and(|day| day.parse::<u64>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn aggregate_budget_prunes_oldest_rotated_file_across_writers() {
        let directory = tempdir().expect("log directory");
        let core = directory.path().join("cubism-core.jsonl");
        let application = directory.path().join("application-20.jsonl.1");
        let core_rotated = directory.path().join("cubism-core.jsonl.1");
        fs::write(&core, vec![b'c'; 1]).expect("core active");
        fs::write(&application, vec![b'a'; (MAX_TOTAL_LOG_BYTES / 2) as usize])
            .expect("application active");
        fs::write(
            &core_rotated,
            vec![b'r'; (MAX_TOTAL_LOG_BYTES / 2 + 1) as usize],
        )
        .expect("core rotated");
        let report = enforce_directory_retention(directory.path(), Some(&core), SystemTime::now());
        assert_eq!(report.pruned, 1);
        assert!(core_rotated.exists() ^ application.exists());
        assert!(report.retained_bytes <= MAX_TOTAL_LOG_BYTES);
    }

    #[test]
    fn expired_rotated_files_are_removed_but_active_files_are_preserved() {
        let directory = tempdir().expect("log directory");
        let active = directory.path().join("cubism-core.jsonl");
        let rotated = directory.path().join("cubism-core.jsonl.1");
        fs::write(&active, b"active").expect("active log");
        fs::write(&rotated, b"rotated").expect("rotated log");
        let future = SystemTime::now()
            .checked_add(Duration::from_secs((RETENTION_DAYS + 1) * 86_400))
            .expect("future timestamp");
        let report = enforce_directory_retention(directory.path(), Some(&active), future);
        assert_eq!(report.pruned, 1);
        assert!(active.exists());
        assert!(!rotated.exists());
    }

    #[test]
    fn unknown_files_are_outside_the_budget_and_symlinks_are_ignored() {
        let directory = tempdir().expect("log directory");
        let active = directory.path().join("cubism-core.jsonl");
        let unknown = directory.path().join("user-data.bin");
        fs::write(&active, b"active").expect("active log");
        fs::write(&unknown, vec![b'u'; (MAX_TOTAL_LOG_BYTES * 2) as usize]).expect("unknown file");
        let report =
            enforce_directory_retention(directory.path(), Some(&active), SystemTime::now());
        assert!(unknown.exists());
        assert_eq!(
            report.retained_bytes,
            active.metadata().expect("active metadata").len()
        );

        #[cfg(unix)]
        {
            let symlink = directory.path().join("cubism-core.jsonl.1");
            std::os::unix::fs::symlink(&unknown, &symlink).expect("symlink");
            let report =
                enforce_directory_retention(directory.path(), Some(&active), SystemTime::now());
            assert!(
                symlink
                    .symlink_metadata()
                    .expect("symlink metadata")
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(
                report.retained_bytes,
                active.metadata().expect("active metadata").len()
            );
        }
    }
}
