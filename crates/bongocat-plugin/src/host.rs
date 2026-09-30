//! What the host tells a plugin about the product right now.
//!
//! A plugin is a separate process, so it can see nothing about the product unless the
//! product says so. This file is the whole of what it says: two facts in
//! [`HostState`], republished on every tick.
//!
//! The list is short on purpose. Each fact is a value the runtime already publishes,
//! so adding one is a change to the product's surface rather than a change to the
//! runtime — and a plugin that wanted more than this is a plugin that wants to be the
//! application. What a panel actually displays is its *own* state; the host's job is
//! only to say which model is on screen and which language the user reads.

use bongocat_plugin_protocol::HostState;

/// What the host publishes to every plugin, in one snapshot.
///
/// A thin wrapper over the protocol's own type, kept because the two are not the same
/// thing: this one is *extracted from the runtime*, and the protocol's is what a
/// plugin receives. Keeping the extraction here means a test can pin it against a
/// snapshot it built, with nothing started.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostFacts {
    /// Whether the model window is currently presenting.
    pub overlay_visible: bool,
    /// The active model's display entry, when there is one.
    pub model_name: Option<String>,
}

impl HostFacts {
    /// Read the facts out of a runtime snapshot.
    ///
    /// Takes the snapshot rather than a `RuntimeClient` so the extraction is a pure
    /// function: the test that pins these fields hands it a snapshot it built, and
    /// nothing here has to be started to be checked.
    pub fn from_runtime(snapshot: &bongocat_runtime::RuntimeSnapshot) -> Self {
        Self {
            overlay_visible: snapshot.overlay_visible,
            model_name: snapshot
                .active_model
                .as_ref()
                .map(|model| model.entry.clone()),
        }
    }

    /// What a plugin is told, in the protocol's own shape.
    ///
    /// `locale` and `app_version` are filled in by the worker rather than here,
    /// because both are properties of the *product build* rather than of the runtime,
    /// and a fact about the build belongs where the build is known.
    pub fn to_host_state(&self, locale: &str, app_version: &str) -> HostState {
        HostState::new(self.model_name.clone(), self.overlay_visible)
            .with_locale(locale)
            .with_app_version(app_version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runtime_with_no_model_publishes_no_name_rather_than_an_empty_one() {
        // "The model is not loaded" and "the model is called the empty string" are
        // different facts, and only one of them is true here. A plugin that showed a
        // model's name must not render a blank where a name should be, nor a blank
        // that looks like a model it failed to load.
        let facts = HostFacts {
            overlay_visible: true,
            model_name: None,
        };
        assert_eq!(facts.to_host_state("en-US", "2.0.1").model_name, None);
    }

    #[test]
    fn a_models_own_name_is_published_verbatim() {
        let facts = HostFacts {
            overlay_visible: true,
            model_name: Some("Cat".to_string()),
        };
        assert_eq!(
            facts.to_host_state("en-US", "2.0.1").model_name.as_deref(),
            Some("Cat")
        );
    }

    #[test]
    fn the_locale_and_version_come_from_the_build_not_the_runtime() {
        let state = HostFacts::default().to_host_state("zh-CN", "2.0.1");
        assert_eq!(state.locale, "zh-CN");
        assert_eq!(state.app_version, "2.0.1");
        assert!(
            !state.overlay_visible,
            "and a runtime that has not started publishes a hidden window, which is the honest \\
             answer rather than an optimistic one"
        );
    }

    #[test]
    fn a_blank_model_name_is_treated_as_no_model() {
        // A model whose display entry is blank is one the user never named, and a
        // panel that showed " " as a name would look broken.
        let state = HostState::new(Some("   ".to_string()), true);
        assert_eq!(state.model_name, None);
    }
}
