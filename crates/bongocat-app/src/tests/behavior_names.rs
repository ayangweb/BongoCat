//! What a motion or expression is called, end to end.
//!
//! The rows name a behavior by position — "Motion 3" — because the resource names
//! inside a package are internal numbering the user cannot see, so a name is the only
//! way to find the behavior they meant. It is stored per model and per behavior, which
//! means the document keeps one row per pair and a model switch has to show the other
//! model's names rather than the ones it left behind.
//!
//! The identifier travels as the canonical `behavior_id` string, spelled by
//! `bongocat-config` and read back by both the configuration validator and the window.
//! These tests cover that round trip and the pruning a removed model needs; the label a
//! name produces is covered where the label is drawn.

use super::*;

fn standard() -> ModelIdentity {
    ModelIdentity {
        id: "standard".to_owned(),
        source: ModelSource::BuiltIn,
    }
}

fn started() -> (tempfile::TempDir, Application) {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let application = Application::start_with_layout_internal(
        layout,
        repository_preset_root().as_path(),
        false,
        Language::English,
    )
    .expect("start application");
    (base, application)
}

/// A name is stored, read back, and applied to that model's rows.
#[test]
fn a_behavior_name_is_stored_and_read_back_for_its_own_model() {
    let (_base, mut application) = started();
    assert!(
        application.config().model.behavior_names.is_empty(),
        "a fresh configuration has named nothing"
    );

    application
        .set_model_behavior_name(
            standard(),
            "motion:CAT_motion:0".to_owned(),
            "the sleepy one".to_owned(),
        )
        .expect("name a motion");

    let rows = &application.config().model.behavior_names;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].model, standard());
    assert_eq!(rows[0].behavior_id, "motion:CAT_motion:0");
    assert_eq!(rows[0].name, "the sleepy one");
    application
        .config()
        .validate()
        .expect("a named behavior is a valid document");
}

/// Renaming the same behavior twice replaces the row rather than adding one.
///
/// Two rows for one behavior would leave the label up to which the parser saw last, so
/// this is the property the command exists to guarantee.
#[test]
fn renaming_the_same_behavior_replaces_its_row() {
    let (_base, mut application) = started();
    for name in ["first", "second"] {
        application
            .set_model_behavior_name(
                standard(),
                "motion:CAT_motion:0".to_owned(),
                name.to_owned(),
            )
            .expect("rename");
    }
    let rows = &application.config().model.behavior_names;
    assert_eq!(rows.len(), 1, "a second rename replaces the first");
    assert_eq!(rows[0].name, "second");
}

/// Clearing the field removes the row, because "go back to the numbered name" is a
/// removal and not a name.
#[test]
fn clearing_a_behavior_name_removes_its_row() {
    let (_base, mut application) = started();
    application
        .set_model_behavior_name(
            standard(),
            "motion:CAT_motion:0".to_owned(),
            "the sleepy one".to_owned(),
        )
        .expect("name");
    assert_eq!(application.config().model.behavior_names.len(), 1);

    for blank in ["", "   "] {
        application
            .set_model_behavior_name(
                standard(),
                "motion:CAT_motion:0".to_owned(),
                blank.to_owned(),
            )
            .expect("clear");
        assert!(
            application.config().model.behavior_names.is_empty(),
            "a blank name must not be stored as {blank:?}"
        );
    }
}

/// Names are per model, and the same behavior of two models is two rows.
///
/// A model switch has to show the other model's names rather than the ones it left
/// behind, which is what the model on each row is for.
#[test]
fn the_same_behavior_of_two_models_is_two_rows() {
    let (_base, mut application) = started();
    let keyboard = ModelIdentity {
        id: "keyboard".to_owned(),
        source: ModelSource::BuiltIn,
    };
    application
        .set_model_behavior_name(
            standard(),
            "motion:CAT_motion:0".to_owned(),
            "standard's own".to_owned(),
        )
        .expect("name for standard");
    application
        .set_model_behavior_name(
            keyboard.clone(),
            "motion:CAT_motion:0".to_owned(),
            "keyboard's own".to_owned(),
        )
        .expect("name for keyboard");

    let rows = &application.config().model.behavior_names;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .find(|row| row.model == standard())
            .map(|row| row.name.as_str()),
        Some("standard's own")
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.model == keyboard)
            .map(|row| row.name.as_str()),
        Some("keyboard's own")
    );

    // Clearing one model's row leaves the other's alone.
    application
        .set_model_behavior_name(standard(), "motion:CAT_motion:0".to_owned(), String::new())
        .expect("clear one");
    let rows = &application.config().model.behavior_names;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].model, keyboard);
}

/// A name is display text, so the service stores what the window would accept.
///
/// The window filters control characters and bounds the value before it sends; the
/// service bounds and trims again because the document is reachable by a hand-edited
/// file too. Two places, one rule, and the field cannot be wider than either.
#[test]
fn a_name_is_trimmed_before_it_is_stored() {
    let (_base, mut application) = started();
    application
        .set_model_behavior_name(
            standard(),
            "motion:CAT_motion:0".to_owned(),
            "   spaced out   ".to_owned(),
        )
        .expect("name");
    assert_eq!(
        application.config().model.behavior_names[0].name,
        "spaced out",
        "a stored name carries no surrounding whitespace"
    );
}
