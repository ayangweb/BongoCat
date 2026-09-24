#![forbid(unsafe_code)]

//! Persistent model-store services: installed catalog ownership, bounded
//! imports, legacy-source conversion, and user-side preset cover storage.
//!
//! Package parsing and immutable model metadata stay in `bongocat-model`;
//! this crate owns the filesystem-facing lifecycle that produces them.

mod key_names;
mod mver;
mod preset_covers;
mod store;

#[cfg(test)]
mod fixture_contract;

pub use mver::{ModelSourceContent, MverInputMode, legacy_keyboard_key_image_names};
pub use preset_covers::{PresetCoverStore, preset_cover_exists};
pub use store::{
    InstalledModelCatalog, ModelImportProgress, ModelImportStage, ModelStore, ModelStoreDiagnostic,
    ModelStoreError, ModelStoreRecovery,
};
