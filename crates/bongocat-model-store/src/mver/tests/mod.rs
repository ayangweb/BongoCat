//! The converter's tests, split by the module they cover.
//!
//! The drivers live here rather than in each module: inspecting a source,
//! converting a mode and naming a button are the same three helpers wherever
//! the assertion is, and repeating them per module would say less than writing
//! them once.

use super::*;

use super::fixture::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use tempfile::tempdir;

/// Inspect a directory source, panicking when it is not recognized.
fn inspect_directory(root: &Path) -> Option<MverPlan> {
    inspect(
        &MverSource::directory(root).expect("open legacy source"),
        ModelPackageLimits::default(),
    )
    .expect("inspect")
}

/// Convert a directory-source mode into a fresh staging directory.
fn convert(root: &Path, plan: &MverModePlan, staging: &Path) -> Result<(), ModelStoreError> {
    let mut statistics = CopyStatistics::default();
    let mut observe = |_progress| {};
    let mut cancelled = || false;
    let mut observation = ImportObservation::new(&mut observe, &mut cancelled);
    convert_mode(
        &MverSource::directory(root).expect("open legacy source"),
        plan,
        staging,
        ModelPackageLimits::default(),
        &mut statistics,
        &mut observation,
    )
}

fn plan_mode(plan: &MverPlan, mode: MverInputMode) -> MverModePlan {
    plan.mode(mode).expect("planned mode").clone()
}

/// Every XInput button index, in the order a pad reports them, paired with
/// the product button it is and the hand its key table would put it on.
///
/// The order is XInput's own: face buttons, then the shoulders, then the two
/// analog triggers, then the two menu buttons, then the two stick clicks, then
/// the D-pad. `legacy_gamepad_button_name` is written against this column
/// order, and this is the one place the whole order is stated, so the table
/// and the layout it claims to describe cannot disagree.
const XINPUT_BUTTONS: [(i64, &str, bool); 16] = [
    (0, "South", false),
    (1, "East", false),
    (2, "West", false),
    (3, "North", false),
    (4, "LeftShoulder", true),
    (5, "RightShoulder", false),
    (6, "LeftTrigger", true),
    (7, "RightTrigger", false),
    (8, "Select", false),
    (9, "Start", false),
    (10, "LeftStick", true),
    (11, "RightStick", false),
    (12, "DpadUp", true),
    (13, "DpadDown", true),
    (14, "DpadLeft", true),
    (15, "DpadRight", true),
];

/// The XInput order, split into the two hand lists a real key table uses.
///
/// The left list is the D-pad, the left shoulder, the left analog trigger and
/// the left stick click; the right list is the face buttons, the right
/// shoulder, the right analog trigger, both menu buttons and the right stick
/// click. `Select` is the one genuinely centred button and lands on the right
/// here purely so the two lists differ in length, which is what exercises the
/// shared keyboard atlas's hand offset.
const XINPUT_LEFT_HAND: [i64; 7] = [12, 13, 14, 15, 4, 6, 10];
const XINPUT_RIGHT_HAND: [i64; 9] = [0, 1, 2, 3, 5, 7, 11, 8, 9];

/// The composed gamepad key images must land on the product's own button
/// names, in the directory the hand list chose, with that button's own
/// artwork.
///
/// This is the assertion that the bundled `gamepad` preset's rename and the
/// Mver conversion agree: the preset ships `LeftShoulder`, `LeftTrigger`,
/// `Dpad*`, `South`… and so must a converted model, or a converted model is a
/// parallel naming scheme the runtime resolves nothing from. It also pins the
/// *pairing*, not just the names — each legacy hand image is a distinct
/// opaque colour, so a table that named the right file with the wrong
/// artwork, or paired one button's index with another's name, fails here
/// rather than producing a model that looks right in a file listing.
#[test]
fn the_gamepad_mode_produces_the_product_button_vocabulary() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Gamepad,
            r#"{"lefthand":[[4],[6]],"righthand":[[0],[9]],"keyboard":[[4],[6],[0],[9]]}"#,
        )],
        true,
    );
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Gamepad);
    assert_eq!(
        mode.slots
            .iter()
            .map(|slot| slot.reference.as_str())
            .collect::<Vec<_>>(),
        vec![
            "left-keys/LeftShoulder.png",
            "left-keys/LeftTrigger.png",
            "right-keys/South.png",
            "right-keys/Start.png",
        ]
    );
}

/// A per-button opaque colour, so a composed image identifies the button it
/// was composed for.
///
/// The composite is `hand` drawn over `keyboard`, and an opaque source
/// replaces the destination outright, so an opaque hand layer makes the
/// result the hand's colour and nothing else. That is what lets this test
/// check *which* legacy binding produced *which* installed file.
fn button_colour(button: i64) -> [u8; 4] {
    let value = u8::try_from(button).expect("button index");
    [value, 255 - value, value / 2, 255]
}

/// Write a legacy gamepad source that binds all sixteen XInput buttons, with
/// one opaque hand image per hand-list entry.
///
/// The keyboard atlas is shared by both hands (ADR-0037 §3: the right hand's
/// indices continue from the left hand's length), so it needs one image per
/// binding in total, and each hand directory needs one per entry in its own
/// list.
fn full_gamepad_source(root: &Path) {
    let section = format!(
        r#"{{"lefthand":[{}],"righthand":[{}],"keyboard":[{}]}}"#,
        XINPUT_LEFT_HAND
            .iter()
            .map(|button| format!("[{button}]"))
            .collect::<Vec<_>>()
            .join(","),
        XINPUT_RIGHT_HAND
            .iter()
            .map(|button| format!("[{button}]"))
            .collect::<Vec<_>>()
            .join(","),
        // The shared atlas is read positionally, left hand first, so the
        // keyboard list repeats the two hand lists in that order.
        XINPUT_LEFT_HAND
            .iter()
            .chain(XINPUT_RIGHT_HAND.iter())
            .map(|button| format!("[{button}]"))
            .collect::<Vec<_>>()
            .join(","),
    );
    write(
        root,
        LEGACY_CONFIG_FILE,
        &legacy_config(&[(MverInputMode::Gamepad, Box::leak(section.into_boxed_str()))]),
    );
    let base = "img/gamepad";
    write(
        root,
        &format!("{base}/{LEGACY_MODEL_DIRECTORY}/cat.model3.json"),
        br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    );
    write(
        root,
        &format!("{base}/{LEGACY_MODEL_DIRECTORY}/model.moc3"),
        b"moc",
    );
    write(root, &format!("{base}/bg.png"), &flat([10, 20, 30, 255]));
    write(root, &format!("{base}/cat.png"), &flat([40, 50, 60, 255]));

    // The shared keyboard atlas: one image per *binding*, in the keyboard
    // list's own order. That order is the left hand's buttons followed by the
    // right hand's, which is the order the section below writes, and it is
    // what makes the right hand's offset land on the right key cap.
    let keyboard_order = XINPUT_LEFT_HAND
        .iter()
        .chain(XINPUT_RIGHT_HAND.iter())
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(keyboard_order.len(), XINPUT_BUTTONS.len());
    for (index, button) in keyboard_order.iter().enumerate() {
        write(
            root,
            &format!("{base}/{LEGACY_KEYBOARD_DIRECTORY}/{index}.png"),
            // Neutral, so a composed image shows the hand layer's colour.
            &flat([1, 1, 1, 255]),
        );
        assert!(
            XINPUT_BUTTONS.iter().any(|(other, ..)| other == button),
            "the keyboard list may only address real XInput buttons, got {button}"
        );
    }
    for (hand, buttons) in [
        (LEGACY_LEFT_HAND_DIRECTORY, &XINPUT_LEFT_HAND[..]),
        (LEGACY_RIGHT_HAND_DIRECTORY, &XINPUT_RIGHT_HAND[..]),
    ] {
        for (index, button) in buttons.iter().enumerate() {
            write(
                root,
                &format!("{base}/{hand}/{index}.png"),
                &flat(button_colour(*button)),
            );
        }
    }
}

mod compose;
mod convert;
mod inspect;
mod plan;
mod source;
mod vocabulary;
