//! Where a model sits on the Model library page, and which input mode it belongs
//! to.
//!
//! The build ships one preset per input mode and the user imports their own, so
//! both facts are decided here from the same mode list the import path uses: a
//! mode added upstream reaches the page order, the badges and the import dialog
//! together.

use bongocat_config::{ImportedModelMetadata, ModelInputMode};
use bongocat_model_store::{ModelStoreInputMode, MverInputMode};

/// The preset model that is always available as the final startup fallback.
pub(crate) const STANDARD_PRESET_MODEL_ID: &str = "standard";

pub(crate) const fn model_input_mode_from_mver(mode: MverInputMode) -> ModelInputMode {
    match mode {
        MverInputMode::Standard => ModelInputMode::Standard,
        MverInputMode::Keyboard => ModelInputMode::Keyboard,
        MverInputMode::Gamepad => ModelInputMode::Gamepad,
    }
}

pub(crate) const fn model_input_mode_from_store(mode: ModelStoreInputMode) -> ModelInputMode {
    match mode {
        ModelStoreInputMode::Standard => ModelInputMode::Standard,
        ModelStoreInputMode::Keyboard => ModelInputMode::Keyboard,
        ModelStoreInputMode::Gamepad => ModelInputMode::Gamepad,
    }
}

pub(crate) fn preset_model_input_mode(id: &str) -> Option<ModelInputMode> {
    match id {
        "standard" => Some(ModelInputMode::Standard),
        "keyboard" => Some(ModelInputMode::Keyboard),
        "gamepad" => Some(ModelInputMode::Gamepad),
        _ => None,
    }
}

/// Where a preset model sits on the Model library page: the position of the input mode
/// it belongs to.
///
/// The build ships one preset per mode, and the modes already declare their own
/// order — [`MverInputMode::ALL`] is Standard, Keyboard, Gamepad, and that order
/// is part of the settings contract because it is the order a conversion
/// reports and titles its models in. Reading it here rather than repeating the
/// three ids keeps one list of modes instead of two that can drift.
///
/// The page has to read it at all because the ids are `standard`, `keyboard`
/// and `gamepad`: ordering by id puts Gamepad first and Standard last, which is
/// the reverse of the mode order a user expects.
///
/// A preset whose id is not one of the modes — a package a developer dropped
/// into the catalog root, or one a later build adds before this list learns
/// about it — is not part of that order, so it sorts after every mode.
pub(crate) fn preset_model_order(id: &str) -> usize {
    MverInputMode::ALL
        .iter()
        .position(|mode| mode.as_str() == id)
        .unwrap_or(MverInputMode::ALL.len())
}

/// Where an installed model sits on the Model library page: the order the user
/// imported it in.
///
/// The configuration's record list is that order. An import appends its record
/// and a deletion only removes one, so a record's position is the model's place
/// on the page and a newly imported model always lands at the end.
///
/// An entry with no record — a package directory copied into the store root by
/// hand, which no import ever ran for — has no place in that order, so it sorts
/// after every model the user actually imported. Ordering those by id keeps the
/// page stable rather than dependent on the order the scan happened to walk the
/// directory in.
pub(crate) fn installed_model_order(records: &[ImportedModelMetadata], id: &str) -> usize {
    records
        .iter()
        .position(|record| record.id == id)
        .unwrap_or(usize::MAX)
}
