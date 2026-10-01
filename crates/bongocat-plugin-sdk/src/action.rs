//! Declaring a control the host should draw on this plugin's card.
//!
//! A builder rather than a struct literal for the same reason [`crate::settings`] is
//! one: a plugin author writes `Action::new("toggle", copy::start())` and reads it
//! back, where the raw form is four fields and three of them are things the author did
//! not choose. The defaults are chosen so the common case — one control, a label, no
//! icon — is a single line.
//!
//! The declaration goes out with [`Host::offer_actions`], which is where the list is
//! checked and where the "replace, never merge" rule lives.

use bongocat_plugin_protocol::{ActionGlyph, PluginAction};

/// One control the host draws for this plugin.
///
/// Not `Eq`, because its label is a table of translations and that is the same
/// reason `LocalizedText` is not — a plugin compares actions through
/// [`PluginAction`]'s own `PartialEq`, which is derived from the same fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    action: PluginAction,
}

impl Action {
    /// A control with this id and this label, and no icon.
    ///
    /// The id is what a press of the host's own button sends back to
    /// [`Plugin::on_press`](crate::Plugin::on_press), so it is worth spelling the same
    /// way the panel's own buttons are: `"toggle"`, `"play"`, `"restart-session"`. It is
    /// opaque to the host, which routes by it and never reads meaning into it.
    pub fn new(id: &str, label: impl Into<PluginActionLabel>) -> Self {
        Self {
            action: PluginAction {
                id: id.to_string(),
                label: label.into().0,
                glyph: ActionGlyph::None,
                disabled: false,
            },
        }
    }

    /// Which of the host's icons stands for this control.
    ///
    /// Optional, and the label alone is a complete control. Worth an icon when the
    /// control is one of a few shapes a person recognises before reading — play,
    /// pause, reset — and not worth one when the label is a sentence that an icon
    /// would only get in the way of.
    pub fn glyph(mut self, glyph: ActionGlyph) -> Self {
        self.action.glyph = glyph;
        self
    }

    /// Present but not pressable.
    ///
    /// For a control that exists in every state but would do nothing in this one. A
    /// plugin that simply omits it draws no button at all, which is the better answer
    /// for a control that is meaningless rather than temporarily unavailable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.action.disabled = disabled;
        self
    }

    /// The protocol value, for sending.
    pub fn to_protocol(&self) -> PluginAction {
        self.action.clone()
    }

    /// Check this declaration against what the host will accept.
    ///
    /// Called by [`Host::offer_actions`](crate::Host::offer_actions) on every action
    /// in the set, so a plugin does not normally need this — it is here so a test can
    /// assert a declaration is one the host will draw, and so the failure has a place
    /// to be read from.
    pub fn check(&self) -> Result<(), String> {
        self.action.validate().map_err(|error| error.to_string())
    }
}

impl From<Action> for PluginAction {
    fn from(action: Action) -> Self {
        action.action
    }
}

/// The label a control carries, however the author wrote it.
///
/// A plain string is one language and a [`bongocat_plugin_protocol::LocalizedText`] is
/// several, and which one an author meant is exactly the kind of thing a `From` pair
/// settles at the call site rather than in every plugin. A `&str` is the common case
/// and reads as one language; a plugin with translations passes the table and says so.
#[derive(Clone, Debug)]
pub struct PluginActionLabel(pub bongocat_plugin_protocol::LocalizedText);

impl From<&str> for PluginActionLabel {
    fn from(label: &str) -> Self {
        Self(label.into())
    }
}

impl From<String> for PluginActionLabel {
    fn from(label: String) -> Self {
        Self(label.into())
    }
}

impl From<&PluginActionLabel> for PluginActionLabel {
    fn from(label: &PluginActionLabel) -> Self {
        label.clone()
    }
}

impl From<&bongocat_plugin_protocol::LocalizedText> for PluginActionLabel {
    fn from(label: &bongocat_plugin_protocol::LocalizedText) -> Self {
        Self(label.clone())
    }
}

impl From<bongocat_plugin_protocol::LocalizedText> for PluginActionLabel {
    fn from(label: bongocat_plugin_protocol::LocalizedText) -> Self {
        Self(label)
    }
}
