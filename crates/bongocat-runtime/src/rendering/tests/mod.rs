//! The renderer's tests, split by what they cover.
//!
//! The evaluation-order test is the one that matters most: everything else here
//! asks whether a value is right, and that one asks whether the values are applied
//! in the right order, which is what a user sees as "my key press did nothing".

use super::*;

use bongocat_live2d::Live2dErrorCode;
use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
use std::path::Path;

fn preset_model(id: &str) -> CommittedModel {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    PresetModelCatalog::open(
        repository_root.join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse(id).expect("model id"))
    .expect("preset model")
}

mod automatic;
mod error;
mod evaluate;
mod model;
mod motion;
