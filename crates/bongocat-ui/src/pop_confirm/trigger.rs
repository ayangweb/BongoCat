//! The adapter that satisfies GPUI Kit's `Selectable` bound for a caller's
//! trigger, and nothing else.
//!
//! `Popover::trigger` asks for a `Selectable` element because it marks the trigger
//! while the surface is open — the call a `Button` turns into its selected style.
//! A confirmation's trigger is an ordinary element the caller already built, so
//! this satisfies the bound and keeps the selection it is handed without painting
//! it: the open state is visible as the surface itself.

use super::*;

/// Carries a caller's trigger into `Popover::trigger`.
///
/// `Popover::trigger` asks for a `Selectable` element because it marks the
/// trigger while the surface is open — the call a `Button` turns into its
/// selected style. A confirmation's trigger is an ordinary element, so this
/// satisfies the bound and keeps the selection it is handed without anywhere to
/// paint it: the open state is visible as the surface itself.
pub(crate) struct TriggerSlot {
    pub(crate) element: AnyElement,
    pub(crate) open: bool,
}

impl Selectable for TriggerSlot {
    fn selected(self, selected: bool) -> Self {
        Self {
            open: selected,
            ..self
        }
    }

    fn is_selected(&self) -> bool {
        self.open
    }
}

impl IntoElement for TriggerSlot {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        self.element
    }
}
