//! A label is the caller's, or the catalog's when the caller gave none.

use super::*;

/// A label is the caller's, or the catalog's when the caller gave none.
///
/// The catalog returns the key itself for a missing entry, so comparing the
/// resolved label against the raw key is what catches a label nobody wrote.
/// Comparing it against the same lookup would agree with itself.
#[test]
fn a_label_is_the_callers_or_the_catalogs() {
    let (confirm, cancel) = resolved_labels(Some("Delete it".into()), Some("Keep it".into()));
    assert_eq!(confirm, "Delete it");
    assert_eq!(cancel, "Keep it");

    let (confirm, cancel) = resolved_labels(None, None);
    for (label, key) in [
        (confirm.as_ref(), "actions.confirm"),
        (cancel.as_ref(), "actions.cancel"),
    ] {
        assert_ne!(
            label, key,
            "a catalog entry nobody wrote leaves the raw key on the button"
        );
        assert!(
            !label.trim().is_empty(),
            "a blank label tells the user nothing about what the button does"
        );
    }
}
