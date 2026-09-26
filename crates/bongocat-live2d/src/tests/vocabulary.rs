//! A key name only counts if a shipped model can draw it.

use super::*;

#[test]
fn vertex_layout_is_tightly_packed_for_gpu_upload() {
    assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
    assert_eq!(align_of::<bongocat_render::Vertex>(), 4);
}

#[test]
fn supported_blend_modes_are_explicit() {
    assert_ne!(BlendMode::Normal, BlendMode::Additive);
    assert_ne!(BlendMode::Additive, BlendMode::Multiplicative);
}

#[test]
fn key_overlay_resolution_is_side_scoped_and_resource_strict() {
    use bongocat_render::{KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources};
    use std::path::PathBuf;

    let asset = |id: usize, side: KeySide, name: &str| KeyAsset {
        id: KeyAssetId::new(id),
        side,
        name: name.to_owned(),
        path: PathBuf::from(format!("{name}.png")),
        width: 612,
        height: 354,
    };
    let resources = RenderResources {
        textures: Vec::new(),
        key_assets: vec![
            asset(0, KeySide::Left, "KeyA"),
            asset(1, KeySide::Left, "Fn"),
            asset(2, KeySide::Left, "Shift"),
            asset(3, KeySide::Left, "ShiftLeft"),
            asset(4, KeySide::Right, "UpArrow"),
        ],
        background: None,
    };

    let mut presses = KeyPressSet::default();
    presses.push(KeyPress::keyboard(0x04, KeySide::Left));
    assert_eq!(
        resolve_key_overlays(&resources, presses)
            .into_iter()
            .map(|overlay| overlay.asset_id.index())
            .collect::<Vec<_>>(),
        vec![0]
    );

    let mut presses = KeyPressSet::default();
    presses.push(KeyPress::keyboard(0x3a, KeySide::Left));
    assert_eq!(
        resolve_key_overlays(&resources, presses)[0]
            .asset_id
            .index(),
        1
    );

    let mut presses = KeyPressSet::default();
    presses.push(KeyPress::keyboard(0xe1, KeySide::Left));
    assert_eq!(
        resolve_key_overlays(&resources, presses)[0]
            .asset_id
            .index(),
        3
    );

    let mut presses = KeyPressSet::default();
    presses.push(KeyPress::keyboard(0x52, KeySide::Left));
    assert!(resolve_key_overlays(&resources, presses).is_empty());
}

/// The globe key and the F1 … F24 fallback are two different keys with two
/// different images, and neither can reach the other's artwork.
///
/// `Fn` is the shared function-row image the old `rdev` layer derived from
/// an unsupported `F<number>`, so every model authored against it — the two
/// shipped keyboard presets included — carries that stem. `Globe` is the
/// globe key's own name and `Function` is the pre-rename spelling of that
/// same key. The two candidate lists are disjoint, which is the whole point:
/// a model shipping only `Fn.png` cannot draw the globe key, and a model
/// shipping only `Globe.png` cannot draw a function key.
#[test]
fn the_globe_key_never_shares_an_image_with_the_function_row() {
    use bongocat_render::{KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources};
    use std::path::PathBuf;

    assert_eq!(
        key_name_candidates(bongocat_render::GLOBE_KEY_USAGE),
        vec!["Globe", "Function"],
        "the globe key's own name, then its pre-rename spelling"
    );
    for hid_usage in (0x3a..=0x45u16).chain(0x68..=0x73) {
        let candidates = key_name_candidates(hid_usage);
        assert!(
            candidates.contains(&"Fn"),
            "0x{hid_usage:02x} keeps the shared function-row fallback"
        );
        assert!(
            !candidates
                .iter()
                .any(|name| matches!(*name, "Globe" | "Function")),
            "0x{hid_usage:02x} must not inherit a globe name: {candidates:?}"
        );
    }

    let asset = |name: &str| KeyAsset {
        id: KeyAssetId::new(0),
        side: KeySide::Left,
        name: name.to_owned(),
        path: PathBuf::from(format!("{name}.png")),
        width: 612,
        height: 354,
    };
    let resources = |name: &str| RenderResources {
        textures: Vec::new(),
        key_assets: vec![asset(name)],
        background: None,
    };
    let resolve = |name: &str, hid_usage: u16| {
        let mut presses = KeyPressSet::default();
        presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
        resolve_key_overlays(&resources(name), presses)
            .first()
            .map(|overlay| overlay.asset_id.index())
    };

    assert_eq!(
        resolve("Fn", 0x3b),
        Some(0),
        "F2 draws the shared row image"
    );
    assert_eq!(
        resolve("Fn", bongocat_render::GLOBE_KEY_USAGE),
        None,
        "the shared row image is not the globe key's"
    );
    assert_eq!(resolve("Globe", bongocat_render::GLOBE_KEY_USAGE), Some(0));
    assert_eq!(
        resolve("Globe", 0x3b),
        None,
        "the globe image is not a function key's"
    );
    assert_eq!(
        resolve("Function", bongocat_render::GLOBE_KEY_USAGE),
        Some(0),
        "a package that still carries the pre-rename spelling draws"
    );
    assert_eq!(
        resolve("Function", 0x3b),
        None,
        "and it is not a function key's image either"
    );
}

#[test]
fn function_keys_prefer_their_own_image_and_fall_back_to_the_shared_fn_asset() {
    use bongocat_render::{KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources};
    use std::path::PathBuf;

    let asset = |id: usize, name: &str| KeyAsset {
        id: KeyAssetId::new(id),
        side: KeySide::Left,
        name: name.to_owned(),
        path: PathBuf::from(format!("{name}.png")),
        width: 612,
        height: 354,
    };
    // One model draws F1, F5 and F13 individually next to the shared image;
    // the other ships nothing but the shared image.
    let partially_specific = RenderResources {
        textures: Vec::new(),
        key_assets: vec![
            asset(0, "Fn"),
            asset(1, "F1"),
            asset(2, "F5"),
            asset(3, "F13"),
        ],
        background: None,
    };
    let shared_only = RenderResources {
        textures: Vec::new(),
        key_assets: vec![asset(0, "Fn")],
        background: None,
    };
    let resolve = |resources: &RenderResources, hid_usage: u16| {
        let mut presses = KeyPressSet::default();
        presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
        resolve_key_overlays(resources, presses)
            .first()
            .map(|overlay| overlay.asset_id.index())
    };

    assert_eq!(
        resolve(&partially_specific, 0x3a),
        Some(1),
        "F1 has its own"
    );
    assert_eq!(
        resolve(&partially_specific, 0x3e),
        Some(2),
        "F5 has its own"
    );
    assert_eq!(
        resolve(&partially_specific, 0x68),
        Some(3),
        "F13 has its own, past the F12 boundary"
    );
    assert_eq!(resolve(&partially_specific, 0x3b), Some(0), "F2 uses Fn");
    assert_eq!(resolve(&partially_specific, 0x45), Some(0), "F12 uses Fn");
    assert_eq!(resolve(&partially_specific, 0x73), Some(0), "F24 uses Fn");

    for hid_usage in 0x3a..=0x45 {
        assert_eq!(
            resolve(&shared_only, hid_usage),
            Some(0),
            "0x{hid_usage:02x} must fall back to the shared Fn image"
        );
    }
    for hid_usage in 0x68..=0x73 {
        assert_eq!(
            resolve(&shared_only, hid_usage),
            Some(0),
            "0x{hid_usage:02x} must fall back to the shared Fn image"
        );
    }
    // PrintScreen (0x46), Keypad = (0x67) and Execute (0x74) sit next to the
    // two function-key ranges and must not inherit the `Fn` fallback.
    for hid_usage in [0x46, 0x67, 0x74] {
        assert_eq!(
            resolve(&shared_only, hid_usage),
            None,
            "0x{hid_usage:02x} is not a function key"
        );
    }
    assert_eq!(
        resolve(&shared_only, 0x29),
        None,
        "the fallback covers function keys only"
    );
}

/// The two Alt keys are distinct physical keys and must resolve to distinct
/// artwork. Before the bundled models were renamed, both HID codes fell
/// through to the same `Alt.png`, so pressing right Alt drew the *left*
/// artwork and the model's own `AltGr.png` was unreachable.
#[test]
fn alt_keys_resolve_their_own_image_and_keep_the_legacy_alias() {
    use bongocat_render::{KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources};
    use std::path::PathBuf;

    // HID usages: `0xe2` is AltLeft, `0xe6` is AltRight.
    assert_eq!(
        key_name_candidates(0xe2),
        vec!["AltLeft", "Alt"],
        "left Alt prefers its own image over the shared family image"
    );
    assert_eq!(
        key_name_candidates(0xe6),
        vec!["AltRight", "AltGr", "Alt"],
        "right Alt must never land on the left artwork while the model still \
         speaks the legacy naming"
    );

    // Every asset sits in `left-keys`: the runtime binds both Alt keys to
    // the left hand, so the side dimension is not what this test varies.
    let resources = |names: &[&str]| RenderResources {
        textures: Vec::new(),
        key_assets: names
            .iter()
            .enumerate()
            .map(|(index, name)| KeyAsset {
                id: KeyAssetId::new(index),
                side: KeySide::Left,
                name: (*name).to_owned(),
                path: PathBuf::from(format!("{name}.png")),
                width: 612,
                height: 354,
            })
            .collect(),
        background: None,
    };
    let resolve = |model: &RenderResources, hid_usage: u16| {
        let mut presses = KeyPressSet::default();
        presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
        resolve_key_overlays(model, presses)
            .first()
            .map(|overlay| model.key_assets[overlay.asset_id.index()].name.clone())
    };

    // A renamed (or freshly imported) model: each side draws its own image.
    let renamed = resources(&["AltLeft", "AltRight"]);
    assert_eq!(resolve(&renamed, 0xe2).as_deref(), Some("AltLeft"));
    assert_eq!(resolve(&renamed, 0xe6).as_deref(), Some("AltRight"));

    // A model that predates the rename: `AltGr` is right Alt's old name, and
    // the left key must not fall back to the right artwork.
    let legacy = resources(&["Alt", "AltGr"]);
    assert_eq!(resolve(&legacy, 0xe2).as_deref(), Some("Alt"));
    assert_eq!(resolve(&legacy, 0xe6).as_deref(), Some("AltGr"));

    // A model with a single shared `Alt` image keeps drawing it on both
    // sides, which is the best an ambiguous model can do.
    let shared = resources(&["Alt"]);
    assert_eq!(resolve(&shared, 0xe2).as_deref(), Some("Alt"));
    assert_eq!(resolve(&shared, 0xe6).as_deref(), Some("Alt"));
}

/// Main Enter and keypad Enter are distinct physical keys (HID `0x28` and
/// `0x58`). The main key keeps its pre-rename `Return` name as a legacy
/// alias, the keypad key prefers a dedicated `KpEnter.png` and falls back
/// to the main `Enter` artwork when the model did not draw one.
#[test]
fn enter_keys_resolve_distinct_names_with_a_keypad_fallback() {
    use bongocat_render::{KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources};
    use std::path::PathBuf;

    assert_eq!(
        key_name_candidates(0x28),
        vec!["Enter", "Return"],
        "main Enter prefers the canonical name and keeps the legacy alias"
    );
    assert_eq!(
        key_name_candidates(0x58),
        vec!["KpEnter", "Enter"],
        "keypad Enter prefers its own image and falls back to the main artwork"
    );

    // Every asset sits in `left-keys`: the side dimension is not what this
    // test varies.
    let resources = |names: &[&str]| RenderResources {
        textures: Vec::new(),
        key_assets: names
            .iter()
            .enumerate()
            .map(|(index, name)| KeyAsset {
                id: KeyAssetId::new(index),
                side: KeySide::Left,
                name: (*name).to_owned(),
                path: PathBuf::from(format!("{name}.png")),
                width: 612,
                height: 354,
            })
            .collect(),
        background: None,
    };
    let resolve = |model: &RenderResources, hid_usage: u16| {
        let mut presses = KeyPressSet::default();
        presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
        resolve_key_overlays(model, presses)
            .first()
            .map(|overlay| model.key_assets[overlay.asset_id.index()].name.clone())
    };

    // A renamed (or freshly imported) model without keypad artwork: the
    // keypad key draws the main Enter image.
    let renamed = resources(&["Enter"]);
    assert_eq!(resolve(&renamed, 0x28).as_deref(), Some("Enter"));
    assert_eq!(resolve(&renamed, 0x58).as_deref(), Some("Enter"));

    // A model that drew both keys uses each artwork for its own key.
    let dedicated = resources(&["Enter", "KpEnter"]);
    assert_eq!(resolve(&dedicated, 0x28).as_deref(), Some("Enter"));
    assert_eq!(resolve(&dedicated, 0x58).as_deref(), Some("KpEnter"));

    // A model that predates the rename: the legacy `Return` image still
    // draws for the main key. The keypad key has no candidate for it —
    // `Return` was never the keypad key's name — so nothing is drawn.
    let legacy = resources(&["Return"]);
    assert_eq!(resolve(&legacy, 0x28).as_deref(), Some("Return"));
    assert_eq!(resolve(&legacy, 0x58), None);

    // The two keys never collapse into one candidate list.
    let none = resources(&[]);
    assert_eq!(resolve(&none, 0x28), None);
    assert_eq!(resolve(&none, 0x58), None);
}

/// The bundled keyboard models must speak the renamed vocabulary: no
/// `Return.png` on disk, the main Enter key resolves to the renamed file,
/// and the keypad Enter key falls back to the very same artwork because
/// the presets ship no dedicated `KpEnter.png`.
#[test]
fn shipped_keyboard_models_draw_both_enter_keys_from_the_renamed_artwork() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
    use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
    for id in ["standard", "keyboard"] {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: load_key_assets(model.root()).expect("key assets"),
            background: None,
        };
        assert!(
            !resources
                .key_assets
                .iter()
                .any(|asset| asset.name == "Return"),
            "{id} must not ship the pre-rename `Return` image"
        );

        let resolve = |hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
            let overlays = resolve_key_overlays(&resources, presses);
            let asset = &resources.key_assets[overlays[0].asset_id.index()];
            (asset.name.clone(), asset.path.clone())
        };
        let (main_name, main_path) = resolve(0x28);
        let (keypad_name, keypad_path) = resolve(0x58);
        assert_eq!(main_name, "Enter", "{id} main Enter");
        assert_eq!(keypad_name, "Enter", "{id} keypad Enter falls back");
        assert!(main_path.ends_with("resources/left-keys/Enter.png"));
        assert_eq!(main_path, keypad_path, "{id} shares the Enter artwork");
    }
}

/// The whole keypad block carries the `Kp*` vocabulary the Mver conversion
/// has always emitted, plus `NumLock`. Every keypad key that duplicates a
/// main keyboard key lists that key's name as its second candidate; the five
/// keys with no counterpart on the main keyboard keep their exact name and
/// nothing else, because there is no artwork to fall back to.
#[test]
fn keypad_keys_name_themselves_and_fall_back_to_their_main_keyboard_twin() {
    for (hid_usage, expected) in [
        (0x53, vec!["NumLock"]),
        (0x54, vec!["KpDivide", "Slash"]),
        (0x55, vec!["KpMultiply"]),
        (0x56, vec!["KpMinus", "Minus"]),
        (0x57, vec!["KpPlus"]),
        (0x58, vec!["KpEnter", "Enter"]),
        (0x59, vec!["Kp1", "Num1"]),
        (0x5a, vec!["Kp2", "Num2"]),
        (0x5b, vec!["Kp3", "Num3"]),
        (0x5c, vec!["Kp4", "Num4"]),
        (0x5d, vec!["Kp5", "Num5"]),
        (0x5e, vec!["Kp6", "Num6"]),
        (0x5f, vec!["Kp7", "Num7"]),
        (0x60, vec!["Kp8", "Num8"]),
        (0x61, vec!["Kp9", "Num9"]),
        (0x62, vec!["Kp0", "Num0"]),
        (0x63, vec!["KpDecimal", "Dot"]),
    ] {
        assert_eq!(
            key_name_candidates(hid_usage),
            expected,
            "keypad 0x{hid_usage:02x}"
        );
    }
}

/// The fallback is what a user actually sees: no model in the repository and
/// none of the collected community samples draws a dedicated `Kp*.png`. Each
/// shipped keyboard preset must therefore draw the digit, Enter and Slash
/// artwork for the keypad keys that duplicate them, and must keep drawing
/// nothing for the five keys that have no counterpart to borrow from.
#[test]
fn shipped_keyboard_models_draw_the_keypad_from_the_main_keyboard_artwork() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
    use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
    for id in ["standard", "keyboard"] {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: load_key_assets(model.root()).expect("key assets"),
            background: None,
        };
        assert!(
            !resources
                .key_assets
                .iter()
                .any(|asset| asset.name.starts_with("Kp")),
            "{id} ships no keypad artwork, so every keypad key is a fallback"
        );

        let resolve = |hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
            resolve_key_overlays(&resources, presses)
                .first()
                .map(|overlay| resources.key_assets[overlay.asset_id.index()].name.clone())
        };

        for (hid_usage, artwork) in [
            (0x54, "Slash"),
            (0x58, "Enter"),
            (0x59, "Num1"),
            (0x5a, "Num2"),
            (0x5b, "Num3"),
            (0x5c, "Num4"),
            (0x5d, "Num5"),
            (0x5e, "Num6"),
            (0x5f, "Num7"),
            (0x60, "Num8"),
            (0x61, "Num9"),
            (0x62, "Num0"),
        ] {
            assert_eq!(
                resolve(hid_usage).as_deref(),
                Some(artwork),
                "{id} keypad 0x{hid_usage:02x} must fall back to {artwork}"
            );
        }

        // Nothing to draw either way: 0x53, 0x55 and 0x57 have no main
        // keyboard counterpart at all, and 0x56 / 0x63 have one (`Minus`,
        // `Dot`) whose artwork the shipped models do not ship.
        for hid_usage in [0x53, 0x55, 0x56, 0x57, 0x63] {
            assert_eq!(
                resolve(hid_usage),
                None,
                "{id} keypad 0x{hid_usage:02x} has no artwork to draw"
            );
        }
    }
}

/// The vocabulary has no holes: every key the platform adapters can report
/// carries at least one candidate name, so a model that ships artwork for it
/// is honoured without a product change. That set is the main block, keypad
/// `=` (macOS reports it separately), F13 … F24, the eight modifier usages
/// `0xe0..=0xe7` and the Apple Fn / globe key; HID `0x66` (Power) is the only
/// usage in the block neither adapter ever produces, so it stays unnamed.
#[test]
fn every_key_the_platform_adapters_can_report_has_a_name() {
    for usage in adapter_keyboard_usages() {
        assert!(
            !key_name_candidates(usage).is_empty(),
            "0x{usage:02x} has no candidate name"
        );
    }
    assert!(
        key_name_candidates(0x66).is_empty(),
        "Power is not a key either adapter maps"
    );
}

/// Every image name the Mver conversion can install is a name the runtime
/// resolves, and resolves *first*.
///
/// Two separate things have to hold, and only checking the first is not
/// enough. The name must reach a key at all — otherwise the conversion
/// installs an image nothing can draw, and the key does nothing whatsoever,
/// because a press without a hand assignment is dropped before the resolver
/// ever sees it (ADR-0042). And it must be the name the runtime reaches
/// first, not a legacy alias that happens to save it: `Backslash` was
/// precisely the second failure, spelled `Backslash` by the conversion while
/// the product spells it `BackSlash`, so every converted backslash image was
/// unreachable (ADR-0050). A name that only resolves through an alias would
/// let that drift back in silently, because the alias keeps the artwork
/// reachable either way.
///
/// `Shift` and `Control` are the two deliberate exceptions: the legacy chart
/// gives each of them a single code for both sides, so the conversion emits
/// the family name and the runtime resolves it for either side (ADR-0038
/// decision 4). They are asserted to still need the exception, so the list
/// cannot quietly become dead. The set of conversion outputs comes from
/// `bongocat-model`, so a new code in the legacy table is covered here
/// without touching this test.
#[test]
fn every_key_image_name_the_conversion_can_install_resolves_to_a_key() {
    const FAMILY_NAMES: [&str; 2] = ["Shift", "Control"];

    let candidates = |usage: u16| key_name_candidates(usage);
    let resolvable: std::collections::BTreeSet<&str> = adapter_keyboard_usages()
        .into_iter()
        .flat_map(key_name_candidates)
        .collect();
    let canonical: std::collections::BTreeSet<&str> = adapter_keyboard_usages()
        .into_iter()
        .filter_map(|usage| candidates(usage).first().copied())
        .collect();

    let outputs = bongocat_model_store::legacy_keyboard_key_image_names();
    let unresolvable: Vec<&str> = outputs
        .iter()
        .copied()
        .filter(|name| !resolvable.contains(name))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "no key resolves these conversion outputs: {unresolvable:?}"
    );
    let alias_only: Vec<&str> = outputs
        .iter()
        .copied()
        .filter(|name| !canonical.contains(name) && !FAMILY_NAMES.contains(name))
        .collect();
    assert!(
        alias_only.is_empty(),
        "these conversion outputs are only reachable through a legacy alias: {alias_only:?}"
    );

    for family in FAMILY_NAMES {
        assert!(
            !canonical.contains(family),
            "{family} has a canonical name now; drop it from the exception list"
        );
    }
}

/// A name is a contract with model authors, not a list of the images the
/// shipped models happen to carry. A model that provides a key image the
/// presets never shipped must draw it with no product change — and a model
/// that provides none of them must keep drawing nothing.
#[test]
fn a_model_providing_a_named_key_image_draws_it() {
    use bongocat_render::{KeyAsset, KeyAssetId, KeyPress, KeyPressSet, KeySide, RenderResources};
    use std::path::PathBuf;

    let resources = |names: &[&str]| RenderResources {
        textures: Vec::new(),
        key_assets: names
            .iter()
            .enumerate()
            .map(|(index, name)| KeyAsset {
                id: KeyAssetId::new(index),
                side: KeySide::Left,
                name: (*name).to_owned(),
                path: PathBuf::from(format!("{name}.png")),
                width: 612,
                height: 354,
            })
            .collect(),
        background: None,
    };
    let resolve = |model: &RenderResources, hid_usage: u16| {
        let mut presses = KeyPressSet::default();
        presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
        resolve_key_overlays(model, presses)
            .first()
            .map(|overlay| model.key_assets[overlay.asset_id.index()].name.clone())
    };

    // A model that draws the punctuation block, the navigation cluster and
    // the keypad keys no shipped model ever drew.
    let future = resources(&[
        "Minus",
        "Equal",
        "LeftBracket",
        "RightBracket",
        "BackSlash",
        "IntlHash",
        "SemiColon",
        "Quote",
        "Comma",
        "Dot",
        "PrintScreen",
        "ScrollLock",
        "Pause",
        "Insert",
        "Home",
        "PageUp",
        "Delete",
        "End",
        "PageDown",
        "NumLock",
        "KpMultiply",
        "KpMinus",
        "KpPlus",
        "KpDecimal",
        "IntlBackslash",
        "Apps",
        "KpEqual",
    ]);
    for (hid_usage, name) in [
        (0x2d, "Minus"),
        (0x2e, "Equal"),
        (0x2f, "LeftBracket"),
        (0x30, "RightBracket"),
        (0x31, "BackSlash"),
        (0x32, "IntlHash"),
        (0x33, "SemiColon"),
        (0x34, "Quote"),
        (0x36, "Comma"),
        (0x37, "Dot"),
        (0x46, "PrintScreen"),
        (0x47, "ScrollLock"),
        (0x48, "Pause"),
        (0x49, "Insert"),
        (0x4a, "Home"),
        (0x4b, "PageUp"),
        (0x4c, "Delete"),
        (0x4d, "End"),
        (0x4e, "PageDown"),
        (0x53, "NumLock"),
        (0x55, "KpMultiply"),
        (0x56, "KpMinus"),
        (0x57, "KpPlus"),
        (0x63, "KpDecimal"),
        (0x64, "IntlBackslash"),
        (0x65, "Apps"),
        (0x67, "KpEqual"),
    ] {
        assert_eq!(
            resolve(&future, hid_usage).as_deref(),
            Some(name),
            "0x{hid_usage:02x} must draw {name}.png when a model ships it"
        );
    }

    // The shipped vocabulary draws nothing for them, because it ships none
    // of these images — the reason is the resource, not the name.
    let shipped = resources(&["KeyA", "Num1", "Enter", "Slash"]);
    for hid_usage in [0x37, 0x2d, 0x4c, 0x63] {
        assert_eq!(
            resolve(&shipped, hid_usage),
            None,
            "0x{hid_usage:02x} has no artwork in the shipped vocabulary"
        );
    }
}

/// The inventory exists to be asked before the product reacts to a key, so
/// it has to name exactly the assets the renderer will load: a name the
/// inventory reports and the loader does not have would move the paw for an
/// image that can never appear, and the reverse would drop a key that draws
/// perfectly well.
#[test]
fn key_image_inventory_lists_exactly_the_assets_the_renderer_loads() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
    for id in ["standard", "keyboard", "gamepad"] {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let inventory = KeyImageInventory::read(model.root());
        let assets = load_key_assets(model.root()).expect("key assets");
        assert!(
            !assets.is_empty(),
            "{id} ships no key artwork, so the test proves nothing"
        );
        for (side, names) in [
            (KeySide::Left, inventory.names(KeySide::Left)),
            (KeySide::Right, inventory.names(KeySide::Right)),
        ] {
            let loaded = assets
                .iter()
                .filter(|asset| asset.side == side)
                .map(|asset| asset.name.clone())
                .collect::<BTreeSet<_>>();
            assert_eq!(names, &loaded, "{id} {side:?}");
        }
    }
}

/// The rule the product applies before it reacts to any key: a model can
/// draw a key only when it ships artwork that key resolves to. `standard`
/// draws the letters, `Delete` and the keypad digits that fall back to the
/// number row, and cannot draw `.`, PrintScreen, NumLock, keypad `.` or the
/// arrow cluster — none of which it ships.
#[test]
fn a_shipped_model_can_draw_only_the_keys_it_ships_artwork_for() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
    let load = |id: &str| {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        KeyImageInventory::read(model.root())
    };

    let standard = load("standard");
    for (hid_usage, drawable, why) in [
        (0x04, true, "KeyA.png"),
        (0x1e, true, "Num1.png"),
        (0x28, true, "Enter.png"),
        (0x3a, true, "Fn.png covers the function row"),
        (0x68, true, "Fn.png covers F13, past the F12 boundary"),
        (0x73, true, "Fn.png covers F24"),
        (0x4c, true, "Delete.png"),
        (0x58, true, "keypad Enter falls back to Enter.png"),
        (0x59, true, "keypad 1 falls back to Num1.png"),
        (0x2d, false, "Minus.png is not shipped"),
        (0x37, false, "Dot.png is not shipped"),
        (0x46, false, "PrintScreen.png is not shipped"),
        (0x53, false, "NumLock.png is not shipped"),
        (0x63, false, "keypad . has no artwork to fall back to"),
        (0x67, false, "keypad = is not shipped"),
        (
            bongocat_render::GLOBE_KEY_USAGE,
            false,
            "Globe.png is not shipped, and Fn.png is not its image",
        ),
    ] {
        assert_eq!(
            standard.can_draw(KeySide::Left, hid_usage),
            drawable,
            "standard 0x{hid_usage:02x}: {why}"
        );
    }
    // The shared function-row image is the only function-key artwork the
    // keyboard presets ship, so the whole row is drawable from it — F13 …
    // F24 included. `can_draw` is what gates the binding
    // (`bongocat-app::input_bindings_for_model`), so a model that ships
    // `Fn.png` alone still binds all 24 keys and still draws whichever one a
    // keyboard reports. F21 … F24 are named and bound but unreachable on both
    // platforms today, which is exactly why the fallback has to hold for the
    // whole range: the vocabulary promises them whether or not hardware can
    // press them.
    //
    // Both keyboard presets are checked because both ship `Fn.png` and
    // neither ships a per-key `F<number>.png`: the fallback is the contract,
    // not a property of `standard`.
    let keyboard = load("keyboard");
    for (id, inventory) in [("standard", &standard), ("keyboard", &keyboard)] {
        for hid_usage in (0x3a..=0x45u16).chain(0x68..=0x73) {
            assert!(
                inventory.can_draw(KeySide::Left, hid_usage),
                "{id} 0x{hid_usage:02x} must be drawable from Fn.png"
            );
        }
    }
    // The arrow cluster is the other hand's artwork, and `standard` ships no
    // `right-keys` directory at all.
    assert!(
        !standard.can_draw(KeySide::Left, 0x52),
        "standard left UpArrow"
    );
    assert!(
        !standard.can_draw(KeySide::Right, 0x52),
        "standard right UpArrow"
    );

    assert!(keyboard.can_draw(KeySide::Right, 0x52), "keyboard UpArrow");
    assert!(
        !keyboard.can_draw(KeySide::Left, 0x52),
        "keyboard left UpArrow"
    );
    assert!(!keyboard.can_draw(KeySide::Left, 0x37), "keyboard Dot.png");

    let gamepad = load("gamepad");
    assert!(
        !gamepad.can_draw(KeySide::Left, 0x04),
        "the gamepad model ships no keyboard artwork"
    );
}

/// The shipped contract: the bundled keyboard models must expose both Alt
/// keys, with different artwork, under the names the resolver asks for.
#[test]
fn shipped_keyboard_models_draw_both_alt_keys_with_their_own_artwork() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
    use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
    for id in ["standard", "keyboard"] {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: load_key_assets(model.root()).expect("key assets"),
            background: None,
        };
        for legacy in ["Alt", "AltGr"] {
            assert!(
                !resources
                    .key_assets
                    .iter()
                    .any(|asset| asset.name == legacy),
                "{id} must not ship the pre-rename `{legacy}` image"
            );
        }

        let resolve = |hid_usage: u16| {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
            let overlays = resolve_key_overlays(&resources, presses);
            let asset = &resources.key_assets[overlays[0].asset_id.index()];
            (asset.name.clone(), asset.path.clone())
        };
        let (left_name, left_path) = resolve(0xe2);
        let (right_name, right_path) = resolve(0xe6);
        assert_eq!(left_name, "AltLeft", "{id} left Alt");
        assert_eq!(right_name, "AltRight", "{id} right Alt");
        assert!(left_path.ends_with("resources/left-keys/AltLeft.png"));
        assert!(right_path.ends_with("resources/left-keys/AltRight.png"));
        assert_ne!(
            fs::read(&left_path).expect("left Alt artwork"),
            fs::read(&right_path).expect("right Alt artwork"),
            "{id} must draw a different image for each Alt key"
        );
    }
}

#[test]
fn shipped_keyboard_models_draw_every_function_key_with_the_shipped_fn_image() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
    use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default()).expect("catalog");
    // The gamepad model is driven by gamepad buttons and ships no `Fn.png`,
    // so only the two keyboard-vocabulary models are covered here.
    for id in ["standard", "keyboard"] {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let resources = RenderResources {
            textures: Vec::new(),
            key_assets: load_key_assets(model.root()).expect("key assets"),
            background: None,
        };
        assert!(
            !resources
                .key_assets
                .iter()
                .any(|asset| asset.name.starts_with('F') && asset.name != "Fn"),
            "{id} must not ship a dedicated function-key image yet"
        );
        for hid_usage in (0x3a..=0x45u16).chain(0x68..=0x73) {
            let mut presses = KeyPressSet::default();
            presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
            let overlays = resolve_key_overlays(&resources, presses);
            let asset = &resources.key_assets[overlays[0].asset_id.index()];
            assert_eq!(asset.name, "Fn", "{id} 0x{hid_usage:02x}");
            assert!(
                asset.path.ends_with("resources/left-keys/Fn.png"),
                "{id} 0x{hid_usage:02x} resolved to {}",
                asset.path.display()
            );
        }
    }
}

/// The other half of the contract: when the model *does* ship a dedicated
/// image, the loader must pick it up from disk and the resolver must prefer
/// it over the shared one.
#[test]
fn a_model_shipping_a_dedicated_function_key_image_uses_it() {
    use bongocat_render::{KeyPress, KeyPressSet, KeySide, RenderResources};
    use tempfile::tempdir;

    let shipped = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/models/standard/resources/left-keys");
    let root = tempdir().expect("root");
    let left_keys = root.path().join("resources/left-keys");
    fs::create_dir_all(&left_keys).expect("left keys directory");
    // The shipped standard model has no dedicated function-key image, so its
    // `Fn.png` bytes stand in for one: this test asserts which file wins, not
    // what the file contains.
    let shared = fs::read(shipped.join("Fn.png")).expect("shipped Fn.png");
    for name in ["Fn", "F13"] {
        fs::write(left_keys.join(format!("{name}.png")), &shared)
            .unwrap_or_else(|error| panic!("write {name}.png: {error}"));
    }

    let resources = RenderResources {
        textures: Vec::new(),
        key_assets: load_key_assets(root.path()).expect("key assets"),
        background: None,
    };
    let resolve = |hid_usage: u16| {
        let mut presses = KeyPressSet::default();
        presses.push(KeyPress::keyboard(hid_usage, KeySide::Left));
        let overlays = resolve_key_overlays(&resources, presses);
        resources.key_assets[overlays[0].asset_id.index()]
            .path
            .clone()
    };

    assert!(
        resolve(0x68).ends_with("left-keys/F13.png"),
        "F13 has its own"
    );
    assert!(resolve(0x69).ends_with("left-keys/Fn.png"), "F14 uses Fn");
    assert!(resolve(0x3a).ends_with("left-keys/Fn.png"), "F1 uses Fn");
    assert!(resolve(0x73).ends_with("left-keys/Fn.png"), "F24 uses Fn");
}

#[test]
fn preset_models_expose_valid_background_assets() {
    use bongocat_model::{ModelPackageLimits, PresetModelCatalog};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(&root, ModelPackageLimits::default())
        .expect("preset model catalog");
    for id in ["standard", "keyboard", "gamepad"] {
        let model = catalog
            .load(&bongocat_model::ModelId::parse(id).expect("model id"))
            .expect("preset model");
        let background = load_background_asset(model.root())
            .expect("background can be decoded")
            .expect("preset background");
        assert_eq!(background.width, 612);
        assert_eq!(background.height, 354);
    }
}
