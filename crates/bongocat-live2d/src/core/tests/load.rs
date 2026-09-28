//! A shipped model loads, and what came back is usable.

use super::*;

#[test]
fn all_preset_models_produce_stable_drawable_snapshots() {
    for id in ["standard", "keyboard", "gamepad"] {
        let committed = preset_model(id);
        let mut model = crate::Live2dModel::load(&committed).expect("load Cubism model");
        let mut first = model.update_and_snapshot().expect("first snapshot");
        assert!(!first.drawables.is_empty());
        assert_eq!(first.bounds, ModelBounds::from_canvas(first.canvas));
        assert!(first.drawables.iter().all(|drawable| {
            drawable.texture_id.index() < model.texture_assets().len()
                && !drawable.vertices.is_empty()
                && !drawable.indices.is_empty()
        }));
        clear_dynamic_flags(&mut first);
        for _ in 0..10 {
            let mut repeated = model.update_and_snapshot().expect("repeat snapshot");
            // Core may report transient change bits while the exported
            // drawable values remain unchanged. They are frame metadata,
            // not part of the stable visual snapshot contract.
            clear_dynamic_flags(&mut repeated);
            assert_eq!(repeated, first);
        }
    }
}

#[test]
fn preset_core_models_survive_repeated_load_update_and_drop_cycles() {
    for id in ["standard", "keyboard", "gamepad"] {
        let committed = preset_model(id);
        let moc_path = committed.root().join(&committed.index().moc);
        for _ in 0..100 {
            let mut model = CoreModel::load(&moc_path).expect("load Core model");
            let snapshot = model.update_and_snapshot().expect("update Core model");
            assert!(
                !snapshot.drawables.is_empty(),
                "{id} must produce a drawable snapshot before drop"
            );
            drop(model);
        }
    }
}
