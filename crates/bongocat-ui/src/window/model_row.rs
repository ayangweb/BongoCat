//! One row of the model list: its key, its focus, and its actions.
//!
//! A row is identified by its key rather than by its index, so a list that
//! reorders under a selection does not move the selection to a different model.
//! The actions a row offers depend on what the catalog says the model is, and
//! the tab indices follow from how many there are — hence both live here rather
//! than in the page that draws them.

use super::*;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ModelRowKey {
    pub(crate) origin_rank: u8,
    pub(crate) id: String,
}

impl ModelRowKey {
    pub(crate) fn new(origin: SettingsModelOrigin, id: &str) -> Self {
        Self {
            origin_rank: match origin {
                SettingsModelOrigin::BuiltIn => 0,
                SettingsModelOrigin::Imported => 1,
            },
            id: id.to_owned(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ModelRowFocus {
    pub(crate) activate: FocusHandle,
    pub(crate) open_location: FocusHandle,
    pub(crate) edit: FocusHandle,
    pub(crate) delete: FocusHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModelRowActions {
    pub(crate) active: bool,
    pub(crate) can_activate: bool,
    /// An imported model is deletable even while it is the one on screen: the
    /// runtime switches to the standard preset before the package goes away, so
    /// no card the user imported ever loses its delete control.
    pub(crate) can_delete: bool,
    /// Every model in the catalog can be renamed and have its cover replaced,
    /// whichever origin it came from: a preset's name and cover are recorded on
    /// the user's side rather than written into the package the build owns, so
    /// the row offers the same edit affordance either way.
    pub(crate) can_edit: bool,
    pub(crate) can_open_location: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ModelRowAction {
    Activate,
    OpenLocation,
    Edit,
    /// Delete the model. Only reachable from the confirmation surface, so
    /// reaching this variant *is* the confirmation.
    Delete,
}

pub(crate) fn model_row_actions(
    entry: &SettingsModelEntry,
    active_model: Option<&SettingsModelKey>,
    commands_blocked: bool,
) -> ModelRowActions {
    let model = SettingsModelKey {
        id: entry.id.clone(),
        origin: entry.origin,
    };
    let active = active_model == Some(&model);
    let ready = matches!(&entry.availability, SettingsModelAvailability::Ready { .. });
    let installed = entry.origin == SettingsModelOrigin::Imported;
    ModelRowActions {
        active,
        can_activate: ready && !active && !commands_blocked,
        can_delete: installed && !commands_blocked,
        // Editing keeps no origin exception: a preset is renamed and re-covered
        // through the same user-side records an installed model uses, so only
        // deleting is still reserved for what the user installed.
        can_edit: !commands_blocked,
        can_open_location: entry.directory.is_some() && !commands_blocked,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ModelRowActionTabIndices {
    pub(crate) activate: isize,
    pub(crate) open_location: isize,
    pub(crate) edit: isize,
    pub(crate) delete: isize,
}

/// The tab position of each action on a card.
///
/// Every card action is rendered at all times — the delete confirmation is a
/// surface anchored to the delete control, not a replacement for the row — so
/// the positions do not move when a confirmation opens.
pub(crate) fn model_row_action_tab_indices(first_tab_index: isize) -> ModelRowActionTabIndices {
    ModelRowActionTabIndices {
        activate: first_tab_index,
        open_location: first_tab_index.saturating_add(1),
        edit: first_tab_index.saturating_add(2),
        delete: first_tab_index.saturating_add(3),
    }
}

/// Whether an open delete question should survive a re-projection of the catalog.
///
/// The question is drawn by the card's delete control, and the card only draws
/// that control while deleting is possible, so the question is only meaningful
/// under exactly the conditions the control needs: an installed model, still in
/// the catalog, and no structural command block (an import running or a picker
/// open). An in-flight command is deliberately not one of those conditions —
/// `pending` never feeds a visual gate (ADR-0053), so the control stays drawn
/// and the question stays valid while it waits, and the command methods refuse
/// a second command instead. Asking [`model_row_actions`] keeps the question
/// and the control from drifting.
pub(crate) fn model_delete_confirmation_is_valid(
    entries: &[SettingsModelEntry],
    active_model: Option<&SettingsModelKey>,
    commands_blocked: bool,
    model: &SettingsModelKey,
) -> bool {
    entries
        .iter()
        .find(|entry| entry.origin == model.origin && entry.id == model.id)
        .is_some_and(|entry| model_row_actions(entry, active_model, commands_blocked).can_delete)
}

pub(crate) fn model_availability_status(
    entry: &SettingsModelEntry,
    language: SettingsLanguage,
) -> Option<SharedString> {
    // A ready model has nothing to say that the card does not already show, so
    // only a diagnostic earns a status line.
    match &entry.availability {
        SettingsModelAvailability::Ready { .. } => None,
        SettingsModelAvailability::Invalid { diagnostic } => {
            let diagnostic = match diagnostic {
                SettingsModelDiagnostic::InvalidModelId
                | SettingsModelDiagnostic::ModelEntryAmbiguous
                | SettingsModelDiagnostic::ModelEntryMissing
                | SettingsModelDiagnostic::ModelReferenceEscapesRoot
                | SettingsModelDiagnostic::ModelReferenceInvalid
                | SettingsModelDiagnostic::ModelReferenceSymlinkEscape
                | SettingsModelDiagnostic::ModelSymlinkDirectoryUnsupported => {
                    "models.validation.package_layout_invalid"
                }
                SettingsModelDiagnostic::ModelFileCountExceeded
                | SettingsModelDiagnostic::ModelFileTooLarge
                | SettingsModelDiagnostic::ModelJsonTooLarge
                | SettingsModelDiagnostic::ModelPackageDepthExceeded
                | SettingsModelDiagnostic::ModelPackageSizeExceeded
                | SettingsModelDiagnostic::ModelTextureDimensionExceeded => {
                    "models.validation.package_safety_limits_exceeded"
                }
                SettingsModelDiagnostic::ModelJsonInvalid
                | SettingsModelDiagnostic::ModelUnsupportedVersion => {
                    "models.validation.model_format_invalid_or_unsupported"
                }
                SettingsModelDiagnostic::ModelTextureInvalidPng
                | SettingsModelDiagnostic::ModelTextureMissing => {
                    "models.validation.texture_invalid"
                }
                SettingsModelDiagnostic::ModelIoError => "models.validation.files_unavailable",
                SettingsModelDiagnostic::ModelMocMissing
                | SettingsModelDiagnostic::ModelResourceInvalid
                | SettingsModelDiagnostic::ModelResourceMissing
                | SettingsModelDiagnostic::ModelResourceNotFile => {
                    "models.validation.resource_invalid"
                }
            };
            Some(
                model_invalid_summary(
                    language,
                    entry.origin,
                    bongocat_i18n::text(language.catalog_locale(), diagnostic),
                )
                .into(),
            )
        }
    }
}
