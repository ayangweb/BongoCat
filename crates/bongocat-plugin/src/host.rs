//! What the host tells a plugin about right now.
//!
//! A plugin cannot ask the runtime anything — it is data, and it runs on its own
//! thread outside the real-time path. So the host reads the runtime snapshot and
//! writes down the handful of facts a panel might display. Which facts is a
//! closed list in this file, and it is short on purpose: each one is a value the
//! runtime already publishes, so adding one is a change to the product's surface
//! rather than a change to the runtime.
//!
//! The two that exist are the ones the motivating plugins need. A pomodoro panel
//! wants to know the model window is visible, so it can stop counting while
//! hidden. A panel that shows model information wants the active model's name,
//! because "which cat is this" is the other thing a person looks at the model
//! window to find out.

use bongocat_plugin_protocol::BindingValue;
use bongocat_plugin_protocol::scene::value::BindingTable;

/// The prefix a host-provided binding starts with.
///
/// Reserved, and the rule that makes it safe is in the protocol: a behavior id may
/// not contain a `.`, so a scene cannot name a behavior that would collide with
/// this. The paths themselves are declared in the protocol too, so the set a
/// plugin may bind to is visible in the vocabulary rather than only in the engine.
pub const HOST_PREFIX: &str = "host";

/// What the host publishes to every plugin, in one snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostFacts {
    /// Whether the model window is currently presenting.
    pub overlay_visible: bool,
    /// The active model's display entry, when there is one.
    pub model_name: Option<String>,
    /// How many keys are held right now.
    pub pressed_key_count: u32,
}

impl HostFacts {
    /// Read the facts out of a runtime snapshot.
    ///
    /// Takes the snapshot rather than a `RuntimeClient` so the extraction is a
    /// pure function: the test that pins these bindings hands it a snapshot it
    /// built, and nothing here has to be started to be checked.
    pub fn from_runtime(snapshot: &bongocat_runtime::RuntimeSnapshot) -> Self {
        Self {
            overlay_visible: snapshot.overlay_visible,
            model_name: snapshot
                .active_model
                .as_ref()
                .map(|model| model.entry.clone()),
            pressed_key_count: u32::try_from(snapshot.input.pressed_key_count).unwrap_or(u32::MAX),
        }
    }

    /// Write every fact into a binding table, under the reserved host prefix.
    pub fn write_into(&self, table: &mut BindingTable) {
        table.set(
            format!("{HOST_PREFIX}.overlay_visible"),
            BindingValue::Flag(self.overlay_visible),
        );
        table.set(
            format!("{HOST_PREFIX}.pressed_key_count"),
            BindingValue::Text(self.pressed_key_count.to_string()),
        );
        match &self.model_name {
            Some(name) => {
                table.set(
                    format!("{HOST_PREFIX}.model_name"),
                    BindingValue::Text(name.clone()),
                );
            }
            None => {
                // Written as an empty string rather than omitted, so a scene that
                // binds to it shows nothing instead of its own fallback — the
                // fallback is there for a plugin that got the path wrong, and
                // "no model" is not that.
                table.set(
                    format!("{HOST_PREFIX}.model_name"),
                    BindingValue::Text(String::new()),
                );
            }
        }
    }

    /// Every path this writes, for a check that a scene's bindings resolve.
    pub fn paths() -> &'static [&'static str] {
        bongocat_plugin_protocol::HOST_BINDING_PATHS
    }
}
