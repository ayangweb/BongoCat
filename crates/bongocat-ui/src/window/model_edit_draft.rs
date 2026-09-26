//! A model being renamed.
//!
//! The title is free-form display text, so it is filtered and bounded before it
//! is stored. It never becomes a key: the store's own identity is the source
//! path, which is why a rename cannot collide with or orphan a model.

use super::*;

/// The one model card that is open for editing.
///
/// The draft owns the title field and a cover the user picked but has not saved
/// yet, so cancelling is dropping this value: nothing reaches the settings
/// service until save, and a half-finished edit never appears in the catalog.
pub(crate) struct ModelEditDraft {
    pub(crate) model: SettingsModelKey,
    pub(crate) title: String,
    /// A cover chosen in this edit, still to be written to the model's package.
    pub(crate) cover: Option<PathBuf>,
    pub(crate) input: Entity<InputState>,
    pub(crate) input_focus: FocusHandle,
    pub(crate) cover_focus: FocusHandle,
    pub(crate) save_focus: FocusHandle,
    pub(crate) cancel_focus: FocusHandle,
    /// A cover dialog is open for this draft.
    pub(crate) picking: bool,
}

/// The title is free-form display text: control characters are dropped and
/// the value is trimmed and bounded to the metadata title limit. The store
/// key never comes from this field.
pub(crate) fn sanitize_model_title_input(value: &str) -> String {
    let filtered: String = value.chars().filter(|c| !c.is_control()).collect();
    let filtered = filtered.trim();
    filtered.chars().take(128).collect()
}

/// The import suggestion shown to the user is the chosen folder's own name.
///
/// The picker has already resolved a directory before this view receives the
/// path, so the view supplies `is_directory = true` rather than probing the
/// filesystem on the GPUI executor. The rule lives in
/// `model_source_display_name` because the settings service's
/// fallback title has to agree with what the page pre-filled. The portable store
/// id is allocated by the settings service at import time, so the displayed name
/// never needs ASCII folding; hand-typed edits are still sanitized by
/// `sanitize_model_title_input`.
pub(crate) fn suggested_model_title(source_root: &Path) -> String {
    crate::model_source_display_name(source_root, true).unwrap_or_else(|| "custom-model".to_owned())
}
