//! The key table, split across sections, and what a plan does with it.

use super::*;

#[test]
fn the_split_hand_sections_share_one_keyboard_list() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Keyboard,
            r#"{"lefthand":[[65]],"righthand":[[37]],"keyboard":[[65],[37]]}"#,
        )],
        true,
    );
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Keyboard);
    // The right hand continues where the left hand stopped, so it reads
    // keyboard image `1` rather than restarting at `0`.
    assert_eq!(
        mode.slots,
        vec![
            MverSlot {
                reference: "left-keys/KeyA.png".to_owned(),
                image: MverSlotImage::Composite {
                    hand: "img/keyboard/lefthand/0.png".to_owned(),
                    keyboard: "img/keyboard/keyboard/0.png".to_owned(),
                },
            },
            MverSlot {
                reference: "right-keys/LeftArrow.png".to_owned(),
                image: MverSlotImage::Composite {
                    hand: "img/keyboard/righthand/0.png".to_owned(),
                    keyboard: "img/keyboard/keyboard/1.png".to_owned(),
                },
            },
        ]
    );
}

#[test]
fn a_missing_layer_skips_its_binding_without_failing_the_conversion() {
    // Neither layer of a binding may be missing: a binding whose paw, or
    // whose companion key image, is absent has no overlay to install and is
    // left out without failing the rest of the conversion.
    for missing in ["img/standard/hand/1.png", "img/standard/keyboard/1.png"] {
        let root = tempdir().expect("root");
        legacy_source(
            root.path(),
            &[(
                MverInputMode::Standard,
                r#"{"hand":[[65],[66]],"keyboard":[[65],[66]]}"#,
            )],
            true,
        );
        fs::remove_file(root.path().join(missing)).expect("remove layer");
        let plan = inspect_directory(root.path()).expect("legacy plan");
        let mode = plan_mode(&plan, MverInputMode::Standard);
        assert_eq!(
            mode.slots
                .iter()
                .map(|slot| slot.reference.as_str())
                .collect::<Vec<_>>(),
            vec!["left-keys/KeyA.png"],
            "removing {missing}"
        );
    }
}

#[test]
fn two_legacy_keys_that_share_one_overlay_write_it_once() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65],[91]],"keyboard":[[65],[91]]}"#,
        )],
        true,
    );
    write(
        root.path(),
        "img/standard/keyboard/1.png",
        &flat([0, 0, 255, 255]),
    );
    write(
        root.path(),
        "img/standard/hand/1.png",
        &flat([255, 0, 0, 255]),
    );

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(
        mode.slots
            .iter()
            .map(|slot| slot.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["left-keys/KeyA.png", "left-keys/MetaLeft.png"]
    );

    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");
    assert!(
        staging
            .path()
            .join("resources/left-keys/KeyA.png")
            .is_file()
    );
    assert!(
        staging
            .path()
            .join("resources/left-keys/MetaLeft.png")
            .is_file()
    );
}
