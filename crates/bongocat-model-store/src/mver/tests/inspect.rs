//! Deciding a folder is a legacy source, and is not.

use super::*;

#[test]
fn a_configured_mode_without_a_model_is_skipped_rather_than_failing() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );
    // The keyboard mode is configured, but its model was never installed.
    write(
        root.path(),
        LEGACY_CONFIG_FILE,
        &legacy_config(&[
            (
                MverInputMode::Standard,
                r#"{"hand":[[65]],"keyboard":[[65]]}"#,
            ),
            (
                MverInputMode::Keyboard,
                r#"{"lefthand":[[65]],"keyboard":[[65]]}"#,
            ),
        ]),
    );

    let plan = inspect_directory(root.path()).expect("legacy plan");
    assert_eq!(
        plan.modes().collect::<Vec<_>>(),
        vec![MverInputMode::Standard]
    );
}

#[test]
fn a_source_without_a_legacy_config_is_not_a_legacy_source() {
    let root = tempdir().expect("root");
    assert!(inspect_directory(root.path()).is_none());

    // A `config.json` that parses but names no mode with a model is still
    // not a legacy source: the second condition is what makes detection
    // unambiguous.
    write(
        root.path(),
        LEGACY_CONFIG_FILE,
        &legacy_config(&[(MverInputMode::Standard, r#"{"hand":[[65]]}"#)]),
    );
    assert!(inspect_directory(root.path()).is_none());

    // A config file that does not parse is the ordinary package import's
    // problem to report, not this module's.
    write(root.path(), LEGACY_CONFIG_FILE, b"not json");
    assert!(inspect_directory(root.path()).is_none());
}

#[test]
fn detection_requires_exactly_one_model_entry_per_mode() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );
    assert!(inspect_directory(root.path()).is_some());

    // A second `.model3.json` in the same mode directory is ambiguous, the
    // same way two entries at a package root are.
    write(
        root.path(),
        "img/standard/cat_model/other.model3.json",
        br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    );
    assert!(inspect_directory(root.path()).is_none());
}
