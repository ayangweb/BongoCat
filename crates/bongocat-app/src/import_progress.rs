//! Folding the per-model import progress of one import action into one sequence.

use bongocat_model_store::{ModelImportProgress, ModelImportStage};

/// Fold the per-model import progress of one import action into one sequence.
///
/// The store reports one model at a time and starts every one of them at
/// `Preparing` with zero totals, because each is imported on its own. The
/// settings monitor drops an update that moves backwards, so without this
/// folding a legacy source's second model would report nothing and the display
/// would sit on the first model's final numbers. Carrying the finished models'
/// totals forward and never letting the stage regress keeps a single honest,
/// monotone sequence for the whole action, which is what the UI is showing.
pub(crate) struct ImportProgressAccumulator<Observe> {
    observe: Observe,
    completed_files: u64,
    completed_bytes: u64,
    current_files: u64,
    current_bytes: u64,
    stage: ModelImportStage,
}

impl<Observe> ImportProgressAccumulator<Observe>
where
    Observe: FnMut(ModelImportProgress),
{
    pub(crate) fn new(observe: Observe) -> Self {
        Self {
            observe,
            completed_files: 0,
            completed_bytes: 0,
            current_files: 0,
            current_bytes: 0,
            stage: ModelImportStage::Preparing,
        }
    }

    pub(crate) fn report(&mut self, update: ModelImportProgress) {
        if update.stage == ModelImportStage::Preparing {
            self.completed_files = self.completed_files.saturating_add(self.current_files);
            self.completed_bytes = self.completed_bytes.saturating_add(self.current_bytes);
            self.current_files = 0;
            self.current_bytes = 0;
        }
        self.current_files = update.files_copied;
        self.current_bytes = update.bytes_copied;
        self.stage = self.stage.max(update.stage);
        (self.observe)(ModelImportProgress {
            stage: self.stage,
            files_copied: self.completed_files.saturating_add(self.current_files),
            bytes_copied: self.completed_bytes.saturating_add(self.current_bytes),
        });
    }
}
