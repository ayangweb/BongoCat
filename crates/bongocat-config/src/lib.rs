//! The persisted configuration: the document shape, the shortcuts it carries, and
//! the store that writes it.
//!
//! This crate is platform-free and holds no state beyond a store handle. The root
//! keeps the schema version, the shared bounds and the private file limits, and
//! re-exports the four public modules below so the rest of the workspace never
//! names where an item lives. `atomic` is crate-internal — only the store and the
//! tests reach it — and `schema` is the JSON Schema generator, where
//! `config_schema` is the document itself.

#![forbid(unsafe_code)]

use bongocat_storage::{create_private_dir_all, set_private_file};
#[cfg(any(test, feature = "schema-generation"))]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::{File, OpenOptions, TryLockError},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod atomic;
mod config_schema;
mod shortcuts;
mod storage;
mod store;
#[cfg(test)]
mod tests;

pub use config_schema::*;
pub use shortcuts::*;
pub use storage::*;
pub use store::*;

#[cfg(any(test, feature = "schema-generation"))]
mod schema;
#[cfg(any(test, feature = "schema-generation"))]
pub use schema::write_json_schemas;
mod window_state;
pub use window_state::{
    OverlayWindowPlacement, WINDOW_STATE_SCHEMA_VERSION, WINDOW_STATE_WRITER_LOCK_FILE_NAME,
    WindowPlacement, WindowState, WindowStateError, WindowStateLoadOutcome, WindowStateLoadStatus,
    WindowStateStore,
};

pub const BUNDLE_ID: &str = "com.ayangweb.bongo-cat";
pub const WINDOW_STATE_FILE_NAME: &str = "window-state.json";
pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS: u16 = 24;
pub const MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS: u16 = 24 * 365;
pub const DEFAULT_LOG_RETENTION_DAYS: u8 = 7;
pub const MAXIMUM_LOG_RETENTION_DAYS: u8 = 30;
/// Default delay between automatic model behavior selections.
pub const DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS: u32 = 30;
/// Automatic behavior selection is disabled by default; a positive interval is
/// required when it is enabled so the renderer can never spin on every frame.
pub const MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS: u32 = 1;
/// Keep the persisted value bounded to a practical user-selectable range.
pub const MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS: u32 = 3_600;
const BACKUP_FORMAT_VERSION: u32 = 1;
const MAX_CONFIG_BACKUPS: usize = 8;
const MAX_CONFIG_BACKUP_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CONFIG_QUARANTINES: usize = 4;
const MAX_CONFIG_QUARANTINE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INTERRUPTED_ARCHIVES: usize = 4;
const MAX_INTERRUPTED_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024;
const RECOVERY_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const RECOVERY_LOCK_TIMEOUT: Duration = Duration::from_secs(1);
