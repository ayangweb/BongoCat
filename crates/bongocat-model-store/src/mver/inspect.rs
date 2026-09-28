//! Deciding whether a folder is a legacy source at all.
//!
//! Detection is deliberately speculative. It claims a source only when the root
//! `config.json` parses *and* a mode that config names really holds a
//! `model3.json`; anything else returns nothing, so the ordinary package import
//! reports its own diagnostic rather than this module guessing.

use super::*;

/// Recognize a legacy source, or report that it is something else.
///
/// Two independent pieces of evidence are required: a root `config.json` that
/// parses into the legacy section shape, and at least one mode it names that
/// really holds a `model3.json` directly under its `cat_model/` directory. A
/// converted BongoCat package puts its entry at the package root instead, so
/// the two formats cannot be confused from the outside.
pub(crate) fn inspect(
    source: &MverSource,
    limits: ModelPackageLimits,
) -> Result<Option<MverPlan>, ModelStoreError> {
    let Some(bytes) = source.read_legacy_config() else {
        return Ok(None);
    };
    let Ok(config) = serde_json::from_slice::<LegacyConfig>(&bytes) else {
        return Ok(None);
    };
    let resources = if source.is_directory(LEGACY_RESOURCE_ROOT)? {
        LEGACY_RESOURCE_ROOT
    } else {
        ""
    };

    let mut modes = Vec::new();
    for mode in MverInputMode::ALL {
        let Some(section) = config.section(mode) else {
            continue;
        };
        let root = join_reference(resources, mode.as_str());
        let model = join_reference(&root, LEGACY_MODEL_DIRECTORY);
        let entries = source.files_below(&model, limits)?;
        let mut model_entries = entries.iter().filter(|reference| {
            is_direct_child(reference, &model) && reference.ends_with(MODEL_ENTRY_SUFFIX)
        });
        // Exactly one entry, exactly like package entry discovery: zero means
        // the mode was configured but never given a model, and more than one is
        // ambiguous and would be rejected after conversion anyway.
        let (Some(_), None) = (model_entries.next(), model_entries.next()) else {
            continue;
        };

        let background_reference = join_reference(&root, mode.background_file());
        let background = source
            .is_file(&background_reference)?
            .then_some(background_reference);
        let cover_reference = join_reference(&root, LEGACY_COVER);
        let cover = source.is_file(&cover_reference)?.then_some(cover_reference);

        let keyboard_root = join_reference(&root, LEGACY_KEYBOARD_DIRECTORY);
        let composites = !source.files_below(&keyboard_root, limits)?.is_empty();
        let mut slots = Vec::new();
        let mut references = BTreeSet::new();
        for binding in section.bindings(mode) {
            let names = legacy_key_names(mode, binding.control_code);
            if names.is_empty() {
                continue;
            }
            let hand = indexed_image_reference(&root, binding.hand_directory, binding.hand_index);
            if !source.is_file(&hand)? {
                continue;
            }
            let image = if composites {
                let keyboard = indexed_image_reference(
                    &root,
                    LEGACY_KEYBOARD_DIRECTORY,
                    binding.keyboard_index,
                );
                if !source.is_file(&keyboard)? {
                    continue;
                }
                MverSlotImage::Composite { hand, keyboard }
            } else {
                MverSlotImage::Verbatim(hand)
            };
            // One binding can address more than one key image — the legacy table
            // has codes for a whole key family rather than for a single key — and
            // each name gets the same composed overlay.
            for name in names {
                let reference = format!("{}/{name}.png", binding.output_directory);
                if !references.insert(reference.clone()) {
                    // Two legacy bindings that resolve to one BongoCat key image:
                    // the first wins, and the duplicate is not a second write.
                    continue;
                }
                slots.push(MverSlot {
                    reference,
                    image: image.clone(),
                });
            }
        }

        modes.push(MverModePlan {
            mode,
            root,
            model,
            background,
            cover,
            slots,
        });
    }

    if modes.is_empty() {
        return Ok(None);
    }
    Ok(Some(MverPlan { modes }))
}
