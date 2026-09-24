//! The unified rule behind every "a switch gates the settings it controls"
//! scenario — the hover-hide delay on the Overlay page, the two shortcut
//! gates on the Shortcuts page, and every future one.
//!
//! A gate binds one switch to the settings below it. The rule fixes the
//! binding once so no scenario re-derives its own condition:
//!
//! - **The switch row is disabled only where editing is structurally
//!   impossible** ([`SettingGate::disables_switch`]) — never by its own
//!   state. A gate that dimmed its own switch while off could never be
//!   switched back on.
//! - **The gated controls are disabled when editing is structurally
//!   impossible *or* the switch is off** ([`SettingGate::disables_controls`]).
//!   Packaged fields take `SettingItem::disabled(...)`; custom-rendered rows
//!   dim themselves and stop registering their interaction handlers, so a
//!   disabled row is inert rather than merely painted lighter.
//! - **Every layer reads the same gate value.** The visible rows and the
//!   view's mutating methods guard on the same predicate.
//!
//! Two properties the rule deliberately fixes for every gate:
//!
//! - Turning a gate back on never rewrites the gated value. A gate only
//!   projects availability; the recorded binding, delay or any other value
//!   survives the switch exactly as the user left it.
//! - The transient in-flight save flag never feeds a gate. It flips on and
//!   off around every save and would visibly dim and re-enable the page on
//!   each control change.
//!
//! To add a gated group: name one predicate from the snapshot that reports
//! whether the switch is on (positive, no inversion), build one
//! [`SettingGate`] per group at render time, and read it from the three
//! layers above.

/// The availability state one gate switch imposes on the settings it
/// controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SettingGate {
    editing_blocked: bool,
    enabled: bool,
}

impl SettingGate {
    /// `editing_blocked` is true only where editing is structurally
    /// impossible (no snapshot yet, model import running); `enabled` is the
    /// gate switch's configuration truth without inversion.
    pub(super) fn new(editing_blocked: bool, enabled: bool) -> Self {
        Self {
            editing_blocked,
            enabled,
        }
    }

    /// Whether the gate's own switch is disabled.
    ///
    /// Only structurally blocked editing disables it, never the switch's own
    /// state: a gate that dimmed its own switch while off could never be
    /// turned back on.
    pub(super) fn disables_switch(self) -> bool {
        self.editing_blocked
    }

    /// Whether the controls the switch gates are disabled.
    pub(super) fn disables_controls(self) -> bool {
        self.editing_blocked || !self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_only_disabled_where_editing_is_structurally_blocked() {
        let gate = SettingGate::new(true, false);
        assert!(gate.disables_switch());
        let gate = SettingGate::new(false, false);
        assert!(
            !gate.disables_switch(),
            "a switched-off gate must keep its own switch operable"
        );
    }

    #[test]
    fn the_controls_are_disabled_by_structural_blocking_or_by_the_switch() {
        for (editing_blocked, enabled, expected) in [
            (false, true, false),
            (false, false, true),
            (true, true, true),
            (true, false, true),
        ] {
            let gate = SettingGate::new(editing_blocked, enabled);
            assert_eq!(
                gate.disables_controls(),
                expected,
                "gate {{ editing_blocked: {editing_blocked}, enabled: {enabled} }} must \
                 disable its controls exactly when editing is blocked or it is switched off"
            );
        }
    }
}
