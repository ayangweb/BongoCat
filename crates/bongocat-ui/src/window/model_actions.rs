//! The model page's actions, split by what the user is doing.
//!
//! What is left here is the vocabulary the two native pickers share: their
//! failures are properties of opening a dialog rather than of what was being
//! chosen, so the page reports them the same way.

use bongocat_platform::ModelSourcePickerError;

use super::SettingsError;
use super::SettingsErrorCode;

/// Map any dialog failure to the one code the page reports.
///
/// The model folder picker and the cover picker share their result vocabulary
/// because their failures are properties of opening a native dialog, not of what
/// was being chosen; the page therefore reports them the same way too.
pub(crate) fn model_source_picker_error(_error: ModelSourcePickerError) -> SettingsError {
    SettingsError::new(SettingsErrorCode::ModelSourcePickerUnavailable)
}
