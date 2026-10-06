//! The random-playback picker: what it lists, and what a click sends.
//!
//! These are the two halves of a set-valued control, and they can disagree in ways
//! that look right on screen. The list can offer a behavior the stored answer does
//! not contain; a click can send a set that loses the behaviors the current mode
//! happens to be hiding. Both would leave the user looking at checkboxes that do not
//! describe what the model plays.

use super::*;

fn active_model() -> SettingsModelKey {
    SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    }
}

fn motion(index: usize) -> SettingsModelBehavior {
    SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index,
    }
}

fn expression(index: usize) -> SettingsModelBehavior {
    SettingsModelBehavior::Expression {
        name: format!("live2d_expression{index}.exp3.json"),
    }
}

fn entries() -> Vec<SettingsModelEntry> {
    vec![model_entry(
        "standard",
        SettingsModelOrigin::BuiltIn,
        SettingsModelAvailability::Ready {
            behaviors: vec![motion(0), motion(1), expression(0), expression(1)],
        },
    )]
}

/// A snapshot with the picker in a given state.
///
/// `included` is passed through verbatim, because the difference between "no
/// selection" and "an empty selection" is the whole point and must not be smoothed
/// over by a helper.
fn snapshot_with(
    mode: SettingsRandomBehaviorMode,
    included: Option<Vec<SettingsModelBehavior>>,
) -> SettingsSnapshot {
    let mut snapshot = crate::tests::snapshot(1, true, true);
    snapshot.active_model = Some(active_model());
    snapshot.model_catalog.entries = entries();
    snapshot.random_behavior.mode = mode;
    snapshot.random_behavior_inclusion = included;
    snapshot
}

/// Nothing chosen yet draws every visible row checked.
///
/// That is the unfiltered answer the scheduler also has, so the page must not open
/// showing an empty list for a model that plays everything.
#[test]
fn an_absent_selection_draws_every_visible_behavior_checked() {
    let snapshot = snapshot_with(SettingsRandomBehaviorMode::MotionsAndExpressions, None);
    let candidates = random_behavior_candidates(&snapshot, SettingsLanguage::English);
    assert_eq!(candidates.len(), 4);
    assert!(
        candidates.iter().all(|candidate| candidate.checked),
        "no choice made yet means every visible behavior is checked"
    );
}

/// The mode decides which rows exist, and narrowing it does not uncheck the rest.
///
/// A motion the user checked stays checked while the mode is expressions-only, so
/// widening the mode again does not present them as something they have to re-pick.
#[test]
fn the_mode_narrows_the_list_without_unchecking_the_rest() {
    let both = snapshot_with(
        SettingsRandomBehaviorMode::MotionsAndExpressions,
        Some(vec![motion(0), expression(0)]),
    );
    let motions = snapshot_with(
        SettingsRandomBehaviorMode::Motions,
        Some(vec![motion(0), expression(0)]),
    );

    let wide = random_behavior_candidates(&both, SettingsLanguage::English);
    let narrow = random_behavior_candidates(&motions, SettingsLanguage::English);
    assert_eq!(wide.len(), 4);
    assert_eq!(narrow.len(), 2);
    assert_eq!(
        narrow
            .iter()
            .filter(|candidate| candidate.checked)
            .map(|candidate| candidate.behavior.clone())
            .collect::<Vec<_>>(),
        vec![motion(0)],
        "the motion the user chose is still chosen under the narrower mode"
    );

    let back = random_behavior_candidates(&both, SettingsLanguage::English);
    assert_eq!(
        back.iter().filter(|candidate| candidate.checked).count(),
        2,
        "widening the mode again restores the user's own selection, not a new one"
    );
}

/// The mode being off leaves nothing to check.
///
/// `None` would draw the list checked, which would be a page claiming a selection
/// the mode cannot act on — so this is a distinct answer rather than "unknown".
#[test]
fn an_off_mode_offers_no_rows_to_check() {
    let snapshot = snapshot_with(SettingsRandomBehaviorMode::Off, None);
    assert!(
        random_behavior_candidates(&snapshot, SettingsLanguage::English).is_empty(),
        "nothing is admitted, so nothing can be checked"
    );
}

/// A model's own answer is what the page draws, checked or not.
#[test]
fn a_written_selection_drives_every_checkbox() {
    let snapshot = snapshot_with(
        SettingsRandomBehaviorMode::MotionsAndExpressions,
        Some(vec![motion(1), expression(1)]),
    );
    let candidates = random_behavior_candidates(&snapshot, SettingsLanguage::English);
    let checked = candidates
        .iter()
        .filter(|candidate| candidate.checked)
        .map(|candidate| candidate.behavior.clone())
        .collect::<Vec<_>>();
    assert_eq!(checked, vec![motion(1), expression(1)]);
}

/// An empty selection is a model that plays nothing, and it is not the same as the
/// absent one.
#[test]
fn an_empty_selection_draws_nothing_checked() {
    let snapshot = snapshot_with(
        SettingsRandomBehaviorMode::MotionsAndExpressions,
        Some(vec![]),
    );
    let candidates = random_behavior_candidates(&snapshot, SettingsLanguage::English);
    assert_eq!(candidates.len(), 4, "the rows are still offered");
    assert!(
        candidates.iter().all(|candidate| !candidate.checked),
        "an explicit empty selection means nothing plays"
    );
}

/// A click sends the whole set, and "nothing checked" is a state it can send.
#[test]
fn a_click_sends_the_whole_set_including_nothing() {
    let snapshot = snapshot_with(SettingsRandomBehaviorMode::MotionsAndExpressions, None);

    let after_uncheck = random_behavior_selection_after(&snapshot, &motion(0), false);
    assert_eq!(
        after_uncheck,
        vec![motion(1), expression(0), expression(1)],
        "unchecking one box sends the rest of the answer, not the one change"
    );

    let all_off = snapshot_with(
        SettingsRandomBehaviorMode::MotionsAndExpressions,
        Some(vec![]),
    );
    let after_checking = random_behavior_selection_after(&all_off, &motion(0), true);
    assert_eq!(
        after_checking,
        vec![motion(0)],
        "checking a box on an empty selection sends exactly that one"
    );
}

/// Narrowing the mode and clicking does not delete the rows it hid.
///
/// The stored answer is the user's choice across modes, so a click under
/// expressions-only must still carry the motions they picked earlier — otherwise
/// switching the mode back would present them as un-picked.
#[test]
fn a_click_under_a_narrower_mode_keeps_the_rows_it_hides() {
    let motions = snapshot_with(
        SettingsRandomBehaviorMode::Motions,
        Some(vec![motion(0), motion(1), expression(0)]),
    );
    let after = random_behavior_selection_after(&motions, &motion(1), false);
    assert_eq!(
        after,
        vec![motion(0), expression(0)],
        "the expression the mode hides is carried across, not dropped"
    );
    assert_eq!(
        after.len(),
        2,
        "one box was unchecked and nothing else changed"
    );
}

/// The rows are labelled the way the Shortcuts page labels the same behaviors.
///
/// The user is checking a box against "Motion 3"; if the two pages numbered the same
/// behavior differently, the picker would be answering a question the page above it
/// phrased differently.
#[test]
fn the_rows_are_labelled_the_way_the_shortcuts_page_labels_them() {
    let snapshot = snapshot_with(SettingsRandomBehaviorMode::MotionsAndExpressions, None);
    let candidates = random_behavior_candidates(&snapshot, SettingsLanguage::English);
    let shortcut_names = shortcut_behavior_rows(
        &snapshot.shortcuts,
        snapshot.active_model.as_ref(),
        &snapshot.model_catalog.entries,
    )
    .iter()
    .map(|row| row.name(SettingsLanguage::English))
    .collect::<Vec<_>>();

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.label.clone())
            .collect::<Vec<_>>(),
        shortcut_names,
        "the same behavior must read the same way on both pages"
    );
}

/// A model the page cannot read offers no rows rather than an error.
///
/// The picker is one row on a page that is otherwise usable, so a package that fails
/// to load must not take the page with it.
#[test]
fn a_model_without_declared_behaviors_offers_no_rows() {
    let mut snapshot = crate::tests::snapshot(1, true, true);
    snapshot.active_model = Some(active_model());
    snapshot.random_behavior.mode = SettingsRandomBehaviorMode::MotionsAndExpressions;
    snapshot.model_catalog.entries = vec![model_entry(
        "standard",
        SettingsModelOrigin::BuiltIn,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    )];
    assert!(random_behavior_candidates(&snapshot, SettingsLanguage::English).is_empty());

    snapshot.active_model = None;
    assert!(
        random_behavior_candidates(&snapshot, SettingsLanguage::English).is_empty(),
        "with no live model the page has nothing to offer"
    );
}

/// The gate the interval row uses also governs the picker, so neither control acts
/// while the mode is off.
#[test]
fn the_picker_is_inert_while_the_mode_is_off() {
    let snapshot = snapshot_with(SettingsRandomBehaviorMode::Off, None);
    assert!(
        !snapshot.random_behavior.mode.is_active(),
        "the gate reads the mode, and the mode is off"
    );
    assert!(
        random_behavior_candidates(&snapshot, SettingsLanguage::English).is_empty(),
        "an inert row has nothing to offer either"
    );
}
