#![forbid(unsafe_code)]

//! Shared, bounded, human-readable logging for BongoCat.
//!
//! The application and Cubism Core use separate streams so support can tell
//! vendor messages apart, but both streams share this writer, level filter,
//! daily rollover, per-file size guard, directory budget, and retention
//! implementation. No third-party logging types cross the project boundary.
//!
//! The six decisions a reader has to keep apart — what may be said, what a line
//! looks like, how a line is read back, where it goes, when it rolls over, and
//! when it is deleted — are the modules below.

//! Shared, bounded, human-readable logging for BongoCat.
//!
//! The application and Cubism Core use separate streams so support can tell
//! vendor messages apart, but both streams share this writer, level filter,
//! daily rollover, per-file size guard, directory budget, and retention
//! implementation. No third-party logging types cross the project boundary.

use bongocat_storage::{create_private_dir_all, set_private_file};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};
use time::{Date, OffsetDateTime, format_description::FormatItem, macros::format_description};

mod clock;
mod level;
mod parse;
mod record;
mod retention;
mod rotation;
mod sanitize;
mod settings;
#[cfg(test)]
mod tests;
mod writer;

pub(crate) use clock::*;
pub(crate) use record::*;
pub(crate) use retention::*;
pub(crate) use rotation::*;
pub(crate) use sanitize::*;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the crate root re-exports are named here rather than left to it.
pub use level::LogLevel;
pub use parse::{ParsedLogLine, parse_log_line};
pub use record::{LogRecord, LogStream};
pub use retention::{RetentionReport, TextLogStats, enforce_directory_retention, is_log_file_name};
pub use settings::{LogSettings, LogSettingsController};
pub use writer::TextLogWriter;

pub const DEFAULT_RETENTION_DAYS: u64 = 7;

pub const MAX_LOG_FILE_BYTES: u64 = 1024 * 1024;

pub const MAX_TOTAL_LOG_BYTES: u64 = 8 * 1024 * 1024;

pub const MAX_TOTAL_LOG_FILES: u64 = 32;

pub(crate) const SECONDS_PER_DAY: u64 = 86_400;

pub(crate) const UTC_DATE_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]");

pub(crate) const RETENTION_CHECK_INTERVAL: Duration = Duration::from_secs(60);
