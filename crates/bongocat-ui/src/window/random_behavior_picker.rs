//! Which of a model's behaviors take part in random playback.
//!
//! The mode already answers "which kinds"; this answers "which members of those
//! kinds", which is a different question with a different shape: a set the user
//! edits by checking boxes rather than a value the user picks from a dropdown.
//!
//! Three states have to stay apart, because two of them look identical on the page
//! and mean different things:
//!
//! - the document says nothing (`None`): nothing has been chosen, so every visible
//!   behavior is checked and the scheduler draws from the whole declared set;
//! - a set with members: exactly those play;
//! - an empty set: the model plays nothing on its own, which the page says out
//!   loud rather than leaving the user to infer it from an unchecked list.
//!
//! The rows come from the same ordered behavior list the Shortcuts page numbers, so
//! a row here and a row there that name the same thing read the same way — the user
//! is checking a box against "Motion 3", and that label is built in one place.
//!
//! The stored set is never narrowed by the mode: a motion the user checked stays
//! checked while the mode is expressions-only, and reappears when the mode goes
//! back. Narrowing the *list* is what the mode does; narrowing the *answer* would
//! make switching modes a silent edit of the user's choice.

use super::*;

/// One row of the picker: the behavior, its label, and whether it plays on its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RandomBehaviorCandidate {
    pub(super) label: String,
    pub(super) behavior: SettingsModelBehavior,
    pub(super) checked: bool,
}

/// The rows the page draws, in the order the model declares its behaviors.
///
/// Empty whenever there is nothing to offer — no live model, a package that cannot
/// be read, or a mode that admits nothing — which is the same answer the rest of the
/// page gives for an absent model. The picker's own "nothing is checked" state is
/// rows that exist with every box clear, not an empty list, so the two never render
/// the same.
pub(super) fn random_behavior_candidates(
    snapshot: &SettingsSnapshot,
    language: SettingsLanguage,
) -> Vec<RandomBehaviorCandidate> {
    let mode = snapshot.random_behavior.mode;
    let checked = snapshot.random_behavior_inclusion.as_ref();
    shortcut_behavior_rows(
        &snapshot.shortcuts,
        snapshot.active_model.as_ref(),
        &snapshot.model_catalog.entries,
    )
    .into_iter()
    .filter_map(|row| {
        // The label is built before the row's playable half is moved out: it is the
        // same numbering the Shortcuts page shows, so borrowing it here is what keeps
        // "Motion 3" meaning one thing across both pages.
        let label = row.name(language);
        let behavior = row.playable?.behavior;
        if !random_behavior_mode_admits(mode, &behavior) {
            return None;
        }
        let checked = checked.is_none_or(|checked| checked.contains(&behavior));
        Some(RandomBehaviorCandidate {
            label,
            behavior,
            checked,
        })
    })
    .collect()
}

/// Whether the mode draws from this kind of behavior at all.
///
/// This is the page's mirror of the runtime's own filter, and it exists so the list
/// shows only what the mode can act on. The runtime remains the authority: if the
/// two ever disagreed, the runtime would draw from a set the page never offered.
pub(super) const fn random_behavior_mode_admits(
    mode: SettingsRandomBehaviorMode,
    behavior: &SettingsModelBehavior,
) -> bool {
    match (mode, behavior) {
        (SettingsRandomBehaviorMode::Off, _) => false,
        (SettingsRandomBehaviorMode::Expressions, SettingsModelBehavior::Expression { .. }) => true,
        (SettingsRandomBehaviorMode::Expressions, SettingsModelBehavior::Motion { .. }) => false,
        (SettingsRandomBehaviorMode::Motions, SettingsModelBehavior::Motion { .. }) => true,
        (SettingsRandomBehaviorMode::Motions, SettingsModelBehavior::Expression { .. }) => false,
        (SettingsRandomBehaviorMode::MotionsAndExpressions, _) => true,
    }
}

/// The whole checked set after one row moved.
///
/// The command carries the set rather than the single toggle because the stored
/// value *is* the set, and an empty one is a state the user can reach: sending only
/// the change would leave the service guessing between "add" and "remove", and would
/// make "nothing checked" unrepresentable.
///
/// The baseline is the stored answer rather than the visible rows, which is what makes
/// the mode a filter over the *list* and not over the *answer*: a motion the user
/// checked stays in the set while the mode is expressions-only, so widening the mode
/// back does not present it as something to pick again. The toggle then applies to that
/// baseline authoritatively — merging it in instead would mean unchecking a motion
/// under the motions-only mode left it in the set, which is the opposite of the click.
pub(super) fn random_behavior_selection_after(
    snapshot: &SettingsSnapshot,
    toggled: &SettingsModelBehavior,
    checked: bool,
) -> Vec<SettingsModelBehavior> {
    let mut selection = snapshot
        .random_behavior_inclusion
        .clone()
        .unwrap_or_else(|| declared_behaviors(snapshot));
    if checked {
        if !selection.contains(toggled) {
            selection.push(toggled.clone());
        }
    } else {
        selection.retain(|behavior| behavior != toggled);
    }
    declaration_order(snapshot, selection)
}

/// Everything the live model declares, which is the unfiltered answer a document
/// with no per-behavior choice means.
fn declared_behaviors(snapshot: &SettingsSnapshot) -> Vec<SettingsModelBehavior> {
    let Some(model) = snapshot.active_model.as_ref() else {
        return Vec::new();
    };
    shortcut_behavior_rows(
        &snapshot.shortcuts,
        Some(model),
        &snapshot.model_catalog.entries,
    )
    .into_iter()
    .filter_map(|row| row.playable.map(|playable| playable.behavior))
    .collect()
}

/// The selection in the order the model package declares its behaviors.
///
/// Declaration order rather than the visible rows' order, so a row the current mode
/// hides still lands where the package put it. A stored document a person reads to
/// work out why a model stopped playing something should not change shape because
/// the mode dropdown moved.
fn declaration_order(
    snapshot: &SettingsSnapshot,
    selection: Vec<SettingsModelBehavior>,
) -> Vec<SettingsModelBehavior> {
    let declared = declared_behaviors(snapshot);
    if declared.is_empty() {
        return selection;
    }
    // Declaration order rather than the visible rows' order, so a row the current
    // mode hides still lands where the package put it. A stored document a person
    // reads to work out why a model stopped playing something should not change shape
    // because the mode dropdown moved.
    let mut ordered = declared
        .iter()
        .filter(|behavior| selection.contains(behavior))
        .cloned()
        .collect::<Vec<_>>();
    // A stored behavior the model no longer declares is kept at the end rather than
    // dropped: whether a behavior still exists is a property of the package, and the
    // write that eventually removes it is the service validating the document.
    let undeclared = selection
        .into_iter()
        .filter(|behavior| !declared.contains(behavior))
        .collect::<Vec<_>>();
    ordered.extend(undeclared);
    ordered
}
