//! What an import is doing, and what the user has typed about it.
//!
//! An import is long enough that the window outlives several of its steps, so its
//! state is a value the view owns rather than a stack of futures: a cancelled
//! import still has to report what it managed to do, which a dropped task could
//! not.

use super::*;

/// What the single import card is doing.
///
/// Every failure is reported through a notification and then returns the card to
/// [`ModelImportState::Idle`], so no state here holds an error: a state that kept
/// one would be a second copy of a message the user has already read.
pub(crate) enum ModelImportState {
    /// Nothing is running; the card is the clickable upload prompt.
    Idle,
    /// A native source dialog is open.
    Picking,
    /// A dropped path is being revalidated and canonicalized away from the UI
    /// executor before it can become an import source.
    ValidatingDrop,
    /// The chosen folder is being classified as a package or a Mver source.
    Inspecting,
    Starting {
        cancel_requested: bool,
    },
    Running(SettingsModelImportMonitor),
    /// The model is installed and the product is rendering its own cover. The
    /// card shows the capture step — replacing the import line rather than adding
    /// to it — and the cards the run installed stay out of the grid until the
    /// capture finishes.
    Capturing,
}

pub(crate) struct ModelImportDraft {
    /// The display name an import will use, derived from the chosen source.
    pub(crate) title: String,
    pub(crate) source_root: Option<PathBuf>,
    pub(crate) state: ModelImportState,
    /// The conversion choices for a Mver source. `Some` only while the dialog
    /// for this source is still open — the draft keeps no selection once the run
    /// starts, because the request has already carried it.
    pub(crate) mver_mode_dialog: Option<MverModeDialog>,
    /// The models that existed when the run started, so the cards the run
    /// installed can be told apart from the ones that were already there and
    /// held back until their cover capture finishes.
    pub(crate) baseline_models: BTreeSet<ModelRowKey>,
}

impl Default for ModelImportDraft {
    fn default() -> Self {
        Self {
            title: String::new(),
            source_root: None,
            state: ModelImportState::Idle,
            mver_mode_dialog: None,
            baseline_models: BTreeSet::new(),
        }
    }
}

impl ModelImportDraft {
    /// Whether the card is showing progress rather than the upload prompt.
    ///
    /// The capture is part of the run: the model is installed, but the card has
    /// not handed over to the catalog yet, so the page keeps treating the run as
    /// in flight for every command gate.
    pub(crate) fn is_running(&self) -> bool {
        matches!(
            self.state,
            ModelImportState::Starting { .. }
                | ModelImportState::Running(_)
                | ModelImportState::Capturing
        )
    }

    /// Whether the card offers a cancel control for the step it is showing.
    ///
    /// The capture that follows a successful import cannot be cancelled: the
    /// model is already installed, and aborting the render would only leave it
    /// without its cover.
    pub(crate) fn shows_cancel(&self) -> bool {
        matches!(
            self.state,
            ModelImportState::Starting { .. } | ModelImportState::Running(_)
        )
    }

    /// Whether a cancel request would still reach a live operation.
    pub(crate) fn is_cancellable(&self) -> bool {
        match &self.state {
            ModelImportState::Starting { cancel_requested } => !cancel_requested,
            ModelImportState::Running(monitor) => !monitor.is_cancelled(),
            _ => false,
        }
    }

    pub(crate) fn can_import(&self) -> bool {
        self.source_root.is_some()
            && !self.title.is_empty()
            && !self.is_running()
            && !self.is_source_surface_open()
    }

    pub(crate) fn is_picker_open(&self) -> bool {
        matches!(self.state, ModelImportState::Picking)
    }

    /// Whether a window-owned source or conversion surface is still up.
    ///
    /// The folder picker and the conversion-mode dialog both stop the card from
    /// starting a second run, so command gates use this rather than picking one.
    pub(crate) fn is_source_surface_open(&self) -> bool {
        self.is_picker_open()
            || self.is_validating_drop()
            || self.is_inspecting()
            || self.has_open_mver_mode_dialog()
    }

    /// Whether a dropped source is being checked before inspection begins.
    pub(crate) fn is_validating_drop(&self) -> bool {
        matches!(self.state, ModelImportState::ValidatingDrop)
    }

    /// Whether the conversion-mode dialog owns the source the user chose.
    pub(crate) fn has_open_mver_mode_dialog(&self) -> bool {
        self.mver_mode_dialog.is_some()
    }

    /// Whether the conversion-mode dialog for the chosen Mver source is open.
    pub(crate) fn is_inspecting(&self) -> bool {
        matches!(self.state, ModelImportState::Inspecting)
    }

    pub(crate) fn running_operation_id(&self) -> Option<SettingsOperationId> {
        match &self.state {
            ModelImportState::Running(monitor) => Some(monitor.operation_id()),
            _ => None,
        }
    }

    pub(crate) fn apply_starting_cancellation(&self, operation: &SettingsModelImportOperation) {
        if matches!(
            self.state,
            ModelImportState::Starting {
                cancel_requested: true
            }
        ) {
            operation.cancel();
        }
    }

    /// Return to the upload prompt, keeping nothing about the run that ended.
    pub(crate) fn reset(&mut self) {
        self.state = ModelImportState::Idle;
        self.source_root = None;
        self.mver_mode_dialog = None;
        self.baseline_models.clear();
    }
}
