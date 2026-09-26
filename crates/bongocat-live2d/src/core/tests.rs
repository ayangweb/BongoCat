//! The Cubism binding's tests, split by the module they cover.
//!
//! Every one of these loads a real model the repository ships, because the thing
//! under test is what Cubism reports about a model rather than what this crate
//! believes about one.

use super::*;

use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_owned()
}

fn preset_model(id: &str) -> CommittedModel {
    PresetModelCatalog::open(
        repository_root().join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse(id).expect("model id"))
    .expect("preset model")
}

fn clear_dynamic_flags(snapshot: &mut RenderSnapshot) {
    for drawable in &mut snapshot.drawables {
        drawable.dynamic_flags = DrawableDynamicFlags::default();
    }
}

mod drawable;
mod load;
mod parameter;
