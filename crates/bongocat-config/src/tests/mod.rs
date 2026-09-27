//! Test fixtures shared by the configuration test modules.
//!
//! The tests live in this directory rather than beside the code they cover, so a
//! production module holds only production code. The crash probe names here are
//! the contract between a parent test and the process it spawns to prove a writer
//! lock is released by process exit rather than by a clean shutdown.

use super::*;
use crate::atomic::*;
use std::process::{Command, Stdio};
use tempfile::tempdir;

mod recovery;
mod shortcuts;
mod store;
mod validation;

fn test_model_identity(id: &str) -> ModelIdentity {
    ModelIdentity {
        id: id.to_owned(),
        source: ModelSource::BuiltIn,
    }
}

const CRASH_PROBE_BASE: &str = "BONGOCAT_CONFIG_CRASH_PROBE_BASE";

const CRASH_PROBE_READY: &str = "BONGOCAT_CONFIG_CRASH_PROBE_READY";

fn config_backup_paths(layout: &StorageLayout) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(&layout.backups)
        .expect("backup directory")
        .map(|entry| entry.expect("backup entry"))
        .filter(|entry| entry.file_name().to_str().is_some_and(is_owned_backup_name))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn config_quarantine_paths(layout: &StorageLayout) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(&layout.backups)
        .expect("backup directory")
        .map(|entry| entry.expect("backup entry"))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(is_owned_quarantine_name)
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn interrupted_archive_paths(layout: &StorageLayout) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(&layout.backups)
        .expect("backup directory")
        .map(|entry| entry.expect("backup entry"))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| parse_owned_interrupted_archive_name(name).is_some())
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn write_interrupted_temp(store: &ConfigStore, config: &NativeConfig) -> Vec<u8> {
    let bytes = serde_json::to_vec_pretty(config).expect("interrupted config bytes");
    let path = config_temp_path(&store.layout().config);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("interrupted config temp");
    file.write_all(&bytes).expect("write interrupted config");
    file.sync_all().expect("sync interrupted config");
    bytes
}
