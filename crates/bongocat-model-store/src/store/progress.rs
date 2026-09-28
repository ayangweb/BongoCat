//! What an import reports while it runs, and how it is cancelled.
//!
//! Progress has to be monotonic and it has to be honest about the total: a
//! stage the store cannot size up front reports bytes as it copies rather than
//! guessing a count. Cancellation is checked between files, because the only
//! safe moment to abandon a copy is one where nothing partial is left behind.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelImportStage {
    Preparing,
    Copying,
    Validating,
    Committing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelImportProgress {
    pub stage: ModelImportStage,
    pub files_copied: u64,
    pub bytes_copied: u64,
}

pub(crate) struct ImportObservation<'a, Observe, IsCancelled> {
    pub(crate) observe: &'a mut Observe,
    pub(crate) is_cancelled: &'a mut IsCancelled,
}

impl<Observe, IsCancelled> ImportObservation<'_, Observe, IsCancelled>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    pub(crate) fn new<'a>(
        observe: &'a mut Observe,
        is_cancelled: &'a mut IsCancelled,
    ) -> ImportObservation<'a, Observe, IsCancelled> {
        ImportObservation {
            observe,
            is_cancelled,
        }
    }

    pub(crate) fn check_cancelled(&mut self) -> Result<(), ModelStoreError> {
        if (self.is_cancelled)() {
            Err(ModelStoreError::new(
                ModelStoreDiagnostic::Cancelled,
                None,
                "model import was cancelled",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn report(&mut self, progress: ModelImportProgress) {
        (self.observe)(progress);
    }
}

pub(crate) fn file_count_for_progress(file_count: usize) -> u64 {
    u64::try_from(file_count).unwrap_or(u64::MAX)
}
