//! Deciding when the Windows taskbar icon has settled.

use super::*;

/// A sample where every surface agrees on `visible`.
fn settled(visible: bool) -> TaskbarIconSample {
    TaskbarIconSample::new(visible, visible, true, true)
}

#[test]
fn an_agreeing_sample_has_no_gap() {
    assert_eq!(taskbar_icon_settle_gap(&settled(true), true), None);
    assert_eq!(taskbar_icon_settle_gap(&settled(false), false), None);
}

/// The two window reads are live shell reads, so a gap in either is a surface
/// that may yet catch up rather than a verdict. Every gap is therefore reported
/// for the caller to re-check, and none of them is a final answer on its own.
#[test]
fn every_gap_is_named_rather_than_bundled_into_one_verdict() {
    let gaps = [
        (
            TaskbarIconSample::new(true, true, false, true),
            TaskbarIconSettleGap::SettingsWindowButtonLost,
        ),
        (
            TaskbarIconSample::new(true, true, true, false),
            TaskbarIconSettleGap::ModelWindowHidden,
        ),
        (
            TaskbarIconSample::new(false, true, true, true),
            TaskbarIconSettleGap::PublishedPreferenceStale,
        ),
        (
            TaskbarIconSample::new(true, false, true, true),
            TaskbarIconSettleGap::ModelWindowButtonStale,
        ),
    ];
    for (sample, expected) in gaps {
        assert_eq!(taskbar_icon_settle_gap(&sample, true), Some(expected));
    }
    let reasons: std::collections::BTreeSet<&str> = [
        TaskbarIconSettleGap::SettingsWindowButtonLost,
        TaskbarIconSettleGap::ModelWindowHidden,
        TaskbarIconSettleGap::PublishedPreferenceStale,
        TaskbarIconSettleGap::ModelWindowButtonStale,
    ]
    .into_iter()
    .map(TaskbarIconSettleGap::reason)
    .collect();
    assert_eq!(reasons.len(), 4, "every gap needs its own failure text");
    assert!(reasons.iter().all(|reason| !reason.is_empty()));
}

/// The window shape is judged before the preference, so a run that lost the
/// button is never reported as a stale snapshot — the two would send whoever
/// reads the failure looking in the wrong place.
#[test]
fn the_settings_window_button_outranks_a_stale_preference() {
    let lost = TaskbarIconSample::new(false, false, false, true);
    assert_eq!(
        taskbar_icon_settle_gap(&lost, true),
        Some(TaskbarIconSettleGap::SettingsWindowButtonLost)
    );
    let hidden = TaskbarIconSample::new(false, false, true, false);
    assert_eq!(
        taskbar_icon_settle_gap(&hidden, true),
        Some(TaskbarIconSettleGap::ModelWindowHidden)
    );
}

/// The sequence the fix exists for. A write reaches the native surface first and
/// the republished snapshot second, and the shell needs a moment to show a style
/// the product has already applied, so the startup check observes disagreement
/// for a few looks before everything agrees. Sampling that first look is what
/// failed on the runner; the budget below is what absorbs it.
#[test]
fn the_startup_divergence_settles_inside_the_budget() {
    let looks = [
        TaskbarIconSample::new(true, false, true, true),
        TaskbarIconSample::new(false, true, true, true),
        TaskbarIconSample::new(true, true, false, true),
        TaskbarIconSample::new(true, true, true, true),
    ];
    for sample in &looks[..looks.len() - 1] {
        assert!(
            taskbar_icon_settle_gap(sample, true).is_some(),
            "{sample:?} should still read as unsettled"
        );
    }
    assert_eq!(taskbar_icon_settle_gap(&looks[looks.len() - 1], true), None);
    assert!(
        looks.len() <= TASKBAR_ICON_SETTLE_ATTEMPTS as usize,
        "the whole sequence has to fit inside the budget"
    );
}

/// A surface that never agrees is still a failure: the budget bounds the wait,
/// it does not excuse the outcome.
#[test]
fn a_button_that_never_comes_back_keeps_reporting_its_gap() {
    let lost = TaskbarIconSample::new(false, false, false, true);
    for _ in 0..TASKBAR_ICON_SETTLE_ATTEMPTS {
        assert_eq!(
            taskbar_icon_settle_gap(&lost, false),
            Some(TaskbarIconSettleGap::SettingsWindowButtonLost)
        );
    }
}

/// The budget is bounded and long enough for the shell to catch up, short enough
/// that a missing button cannot hold a runner slot.
#[test]
fn the_settle_budget_stays_bounded() {
    let budget = TASKBAR_ICON_SETTLE_INTERVAL * TASKBAR_ICON_SETTLE_ATTEMPTS;
    assert!(
        budget >= Duration::from_millis(500),
        "{budget:?} is too eager"
    );
    assert!(
        budget <= Duration::from_secs(5),
        "{budget:?} is too patient"
    );
}
