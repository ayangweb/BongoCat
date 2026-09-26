//! The directories an operation owns while it runs.
//!
//! Nothing is written into the store's model directory directly: an import
//! stages into a dot-prefixed directory, a delete stages the same way, and the
//! name says which operation owns it. That is what makes an interrupted run
//! recoverable — the next start can tell an abandoned operation from a model the
//! user installed, and clear the first without guessing about the second.

use super::*;

pub(crate) const IMPORTING_PREFIX: &str = ".importing-";

pub(crate) const DELETING_PREFIX: &str = ".deleting-";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelStoreRecovery {
    pub abandoned_imports_removed: usize,
    pub abandoned_deletions_removed: usize,
}

pub(crate) fn is_owned_operation_name(name: &str, prefix: &str) -> bool {
    let Some(remainder) = name.strip_prefix(prefix) else {
        return false;
    };
    let Some((id_and_process, sequence)) = remainder.rsplit_once('-') else {
        return false;
    };
    let Some((id, process)) = id_and_process.rsplit_once('-') else {
        return false;
    };
    ModelId::parse(id).is_ok() && process.parse::<u32>().is_ok() && sequence.parse::<u64>().is_ok()
}

pub(crate) struct StagingCleanup {
    pub(crate) path: Option<PathBuf>,
}

impl StagingCleanup {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub(crate) fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}
