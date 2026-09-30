//! What a plugin is: a descriptor and a set of callbacks.
//!
//! A plugin is a type that implements [`Plugin`]. It says who it is in
//! [`Plugin::descriptor`] and reacts to what the host sends in the other four
//! methods — all of which have a default, so a plugin implements only the ones it
//! needs and a counter that only wants a tick writes one method.
//!
//! # Why a trait and not a closure or an event loop of your own
//!
//! Because the tick is the unit of work, and a plugin that owns its own loop would
//! have to re-derive what "a tick" means. `elapsed_ms` is monotonic since the
//! session started and arrives at the host's cadence; a plugin that wants ten
//! updates a second divides, and one that wants one a second only redraws when the
//! number it shows has changed. Neither needs to know how often the host actually
//! ticks, and both are correct if the host's cadence moves.
//!
//! # The order of the callbacks
//!
//! [`Plugin::on_ready`] runs once, after the handshake and after the settings have
//! been read, before any other callback. Then the session is a loop: each message
//! from the host becomes at most one callback, in the order the host sent it. That
//! ordering is the contract a plugin's own logic can rely on — a press is never
//! delivered before the tick that preceded it, and a configuration change is never
//! delivered before the ready that read the first one.

use crate::settings::Settings;
use crate::{Host, Result};
use bongocat_plugin_protocol::{
    ConfigSchema, InputEvent, LocalizedText, MAXIMUM_PLUGIN_DESCRIPTION_CHARS,
    MAXIMUM_PLUGIN_NAME_CHARS, ModelOutcome, Subscription,
};

/// What the host tells a plugin on a tick.
///
/// Two facts, because that is what the host has: time passed, and the world may be
/// different. A plugin that only needs a clock reads [`Tick::elapsed_ms`]; one that
/// shows the model reads [`Tick::state`].
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    /// Milliseconds since the session started, monotonic.
    pub elapsed_ms: u64,
    /// The host's facts as of this tick.
    pub state: bongocat_plugin_protocol::HostState,
    /// The user's local wall clock, as the host's main thread read it.
    ///
    /// Republished rather than fetched, and the reason is worth a plugin author
    /// knowing: reading the local UTC offset is only sound when one thread at a time
    /// asks, so the host asks on its main thread and hands the answer to every plugin.
    /// A plugin that shows a clock formats this and never touches the operating
    /// system's timezone database.
    pub clock: bongocat_plugin_protocol::WallClock,
}

impl Tick {
    /// Whole seconds since the session started.
    ///
    /// The unit a countdown, a stopwatch and a rate limit actually want, and the
    /// one a plugin should convert to rather than dividing by a thousand at each use
    /// site.
    pub fn elapsed_seconds(&self) -> u64 {
        self.elapsed_ms / 1000
    }

    /// Milliseconds since the previous whole second, for a panel that shows a
    /// fraction of a second.
    pub fn sub_second(&self) -> u64 {
        self.elapsed_ms % 1000
    }

    /// Whether the model window is on screen.
    pub fn is_overlay_visible(&self) -> bool {
        self.state.overlay_visible
    }

    /// The local time as `HH:MM:SS`.
    ///
    /// Written here rather than left to each plugin, so every clock on the model
    /// window is spelled the same way. A plugin that wants a different format formats
    /// [`Tick::clock`] itself.
    pub fn time_text(&self) -> String {
        self.clock.to_hms()
    }
}

/// Something the host said, for a plugin that would rather have one callback.
///
/// [`Plugin::on_event`] is the whole session as one method, and the five specific
/// callbacks are sugar over it. A plugin that wants the specific ones writes them;
/// a plugin that wants to match on the message itself writes this one. Both are the
/// same dispatch, so a test that drives [`Plugin::on_event`] is testing the same
/// path a plugin's other callbacks take.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The handshake is done and the settings have been read.
    Ready,
    /// Time passed.
    Tick(Tick),
    /// Something happened, for a plugin subscribed to the input feed.
    Input(Vec<InputEvent>),
    /// A press on one of this plugin's own buttons.
    Press { id: String },
    /// The user changed a setting.
    ConfigChanged,
    /// The host's answer to a model request this plugin made.
    Answer { id: u64, outcome: ModelOutcome },
    /// The host is stopping this plugin.
    Shutdown,
}

/// A BongoCat plugin.
///
/// Four of the five methods have a default, so the smallest plugin is a descriptor
/// and nothing else.
pub trait Plugin {
    /// Who this plugin is.
    ///
    /// Asked once, at the handshake. The id here is checked against the archive the
    /// store installed, so a plugin that announces a different one is refused rather
    /// than shown under a name it did not claim.
    fn descriptor(&self) -> Descriptor;

    /// The settings the user can change, once the descriptor has been asked for.
    ///
    /// Separate from [`Plugin::descriptor`] because the descriptor is asked before
    /// the host knows whether the plugin wants settings, and a schema is a bigger
    /// thing to build than a name. A plugin with no settings leaves this alone.
    fn settings(&mut self) -> Settings {
        Settings::new()
    }

    /// The handshake is done, the settings have been read, and nothing has been
    /// drawn yet.
    ///
    /// The place to load whatever the plugin keeps between runs, and to draw its
    /// first panel — a panel that appears on the first ready rather than on the
    /// first tick is a panel that is there when the window opens.
    fn on_ready(&mut self, host: &mut Host) -> Result<()> {
        let _ = host;
        Ok(())
    }

    /// Time passed.
    ///
    /// The callback almost every plugin needs, and the only one that has to exist
    /// for a panel to appear.
    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        let _ = (tick, host);
    }

    /// Something happened.
    ///
    /// Only called for a plugin whose descriptor subscribed to [`Subscription::Input`].
    /// A plugin that did not ask is never called, which is what makes the
    /// subscription a statement of intent rather than a hint.
    fn on_input(&mut self, events: Vec<InputEvent>, host: &mut Host) {
        let _ = (events, host);
    }

    /// One of this plugin's own buttons was pressed.
    ///
    /// The id is the one the panel's button declared, so the plugin never has to
    /// work out which of its buttons a click landed in — the host's hit test did
    /// that, against the same rectangles the user saw.
    fn on_press(&mut self, id: &str, host: &mut Host) {
        let _ = (id, host);
    }

    /// The user changed a setting.
    ///
    /// [`Host::values`] already holds the new document, so a plugin reads it rather
    /// than being handed a patch it would have to apply.
    fn on_config_changed(&mut self, host: &mut Host) {
        let _ = host;
    }

    /// The host's answer to a model request this plugin made.
    fn on_answer(&mut self, id: u64, outcome: ModelOutcome, host: &mut Host) {
        let _ = (id, outcome, host);
    }

    /// The host is stopping this plugin.
    ///
    /// The last chance to write state. After this the process is expected to exit,
    /// and the host will not wait long for it — so this is a flush, not a shutdown
    /// sequence.
    fn on_shutdown(&mut self, host: &mut Host) {
        let _ = host;
    }

    /// The whole session as one method.
    ///
    /// Defaults to calling the specific callbacks, so overriding it replaces all of
    /// them at once rather than one — which is what a plugin that wants to match on
    /// the message itself needs.
    fn on_event(&mut self, event: Event, host: &mut Host) -> Result<()> {
        match event {
            Event::Ready => self.on_ready(host),
            Event::Tick(tick) => {
                self.on_tick(tick, host);
                Ok(())
            }
            Event::Input(events) => {
                self.on_input(events, host);
                Ok(())
            }
            Event::Press { id } => {
                self.on_press(&id, host);
                Ok(())
            }
            Event::ConfigChanged => {
                self.on_config_changed(host);
                Ok(())
            }
            Event::Answer { id, outcome } => {
                self.on_answer(id, outcome, host);
                Ok(())
            }
            Event::Shutdown => {
                self.on_shutdown(host);
                Ok(())
            }
        }
    }

    /// Read the handshake, then serve the session until the host stops.
    ///
    /// This is what a plugin's `main` calls, and it is on the trait rather than a
    /// free function so that the whole of a plugin's life — including how it starts —
    /// is one implementation.
    fn run(self) -> Result<()>
    where
        Self: Sized,
    {
        crate::run(self)
    }
}

/// Who a plugin says it is.
///
/// Every field but the id and the name has a default, so a plugin that shows a
/// panel and nothing else writes two calls. The version is checked against the
/// archive the store installed: a plugin that announces a different one is refused
/// rather than displayed, because a card that says one version and runs another is
/// worse than a plugin that will not start.
#[derive(Clone, Debug)]
pub struct Descriptor {
    id: String,
    name: LocalizedText,
    version: bongocat_plugin_protocol::PluginVersion,
    author: String,
    description: LocalizedText,
    icon: bongocat_plugin_protocol::PluginIcon,
    settings: Settings,
    subscriptions: Vec<Subscription>,
}

impl Descriptor {
    /// A plugin with this id and this name, at version 1.0.0.
    ///
    /// The version is a fixed point rather than "read from `Cargo.toml`": a plugin
    /// that also ships a `plugin.json` has two version numbers to keep in step, and
    /// the one in the descriptor is the one the host checks the running process
    /// against.
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.into(),
            version: bongocat_plugin_protocol::PluginVersion::new(1, 0, 0),
            author: String::new(),
            description: LocalizedText::default(),
            icon: bongocat_plugin_protocol::PluginIcon::default(),
            settings: Settings::new(),
            subscriptions: Vec::new(),
        }
    }

    /// This plugin, at this version.
    pub fn version(mut self, major: u64, minor: u64, patch: u64) -> Self {
        self.version = bongocat_plugin_protocol::PluginVersion::new(major, minor, patch);
        self
    }

    /// This plugin, by this author.
    pub fn author(mut self, author: &str) -> Self {
        self.author = author.to_string();
        self
    }

    /// This plugin, described in one sentence.
    pub fn description(mut self, description: &str) -> Self {
        self.description = description.into();
        self
    }

    /// This plugin, under the name it uses in the languages it has copy for.
    ///
    /// Separate from [`Self::name`] because a name is usually a proper noun that is the
    /// same in every language, and a description is almost never — so a plugin whose
    /// name needs no translation should not have to say so twice.
    pub fn named(mut self, name: LocalizedText) -> Self {
        self.name = name;
        self
    }

    /// This plugin, described in the languages it has copy for.
    pub fn described(mut self, description: LocalizedText) -> Self {
        self.description = description;
        self
    }

    /// This plugin's icon: a short emoji.
    ///
    /// The image half of [`bongocat_plugin_protocol::PluginIcon`] exists and the host
    /// reads it; this method takes the emoji because that is what a plugin can write
    /// without shipping a file. A plugin that has one can build the icon directly.
    pub fn icon(mut self, emoji: &str) -> Self {
        self.icon = bongocat_plugin_protocol::PluginIcon {
            emoji: Some(emoji.to_string()),
            image: None,
        };
        self
    }

    /// This plugin's icon, in full.
    pub fn icon_value(mut self, icon: bongocat_plugin_protocol::PluginIcon) -> Self {
        self.icon = icon;
        self
    }

    /// The settings the user can change.
    pub fn settings(mut self, settings: Settings) -> Self {
        self.settings = settings;
        self
    }

    /// Ask for a feed.
    ///
    /// Without a subscription a plugin is never sent input events at all, so a tally
    /// that forgot to ask reads zero rather than receiving events it ignored.
    pub fn subscribe(mut self, subscription: Subscription) -> Self {
        if !self.subscriptions.contains(&subscription) {
            self.subscriptions.push(subscription);
        }
        self
    }

    /// This plugin's id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// This plugin's name, unresolved.
    ///
    /// The text itself rather than a resolved string, because the host is the side that
    /// knows the user's language and a plugin cannot resolve its own name for a card it
    /// does not draw.
    pub fn name(&self) -> &LocalizedText {
        &self.name
    }

    /// This plugin's description, unresolved.
    pub fn description_text(&self) -> &LocalizedText {
        &self.description
    }

    /// This plugin's version.
    pub fn plugin_version(&self) -> &bongocat_plugin_protocol::PluginVersion {
        &self.version
    }

    /// The feeds this plugin asked for.
    pub fn subscriptions(&self) -> &[Subscription] {
        &self.subscriptions
    }

    /// Whether this plugin asked for a feed.
    pub fn subscribes_to(&self, subscription: Subscription) -> bool {
        self.subscriptions.contains(&subscription)
    }

    /// This plugin's settings.
    pub fn declared_settings(&self) -> &Settings {
        &self.settings
    }

    /// The protocol's descriptor, checked.
    ///
    /// Checked here rather than left to the host so a plugin's own test run says
    /// *which* field is wrong — an id with a space, a name that is too long, a
    /// default outside its range — instead of the host refusing to start the process
    /// and the author guessing.
    pub fn to_protocol(&self) -> Result<bongocat_plugin_protocol::PluginDescriptor> {
        let descriptor = bongocat_plugin_protocol::PluginDescriptor {
            id: bongocat_plugin_protocol::PluginId::new(self.id.clone()).map_err(invalid)?,
            name: self.name.clone(),
            version: self.version,
            author: self.author.clone(),
            description: self.description.clone(),
            icon: self.icon.clone(),
            config: self.settings.to_schema()?,
            subscriptions: self.subscriptions.clone(),
        };
        descriptor.validate().map_err(invalid)?;
        Ok(descriptor)
    }

    /// The settings schema this plugin declares.
    pub fn schema(&self) -> Result<ConfigSchema> {
        self.settings.to_schema()
    }

    /// Check what this descriptor says about itself, before a handshake.
    pub fn check(&self) -> Result<()> {
        self.to_protocol().map(|_| ())
    }
}

impl std::fmt::Display for Descriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The default, because a `Display` has no user's language to resolve against.
        // A card resolves the descriptor through the host, which does know it.
        write!(formatter, "{} {}", self.name.resolve(""), self.version)
    }
}

fn invalid(error: bongocat_plugin_protocol::PluginError) -> crate::Error {
    crate::Error::Failed(format!(
        "this plugin's own descriptor is not valid: {error}"
    ))
}

/// Check that a name and a description fit what the plugin center can show.
///
/// Exposed because a plugin author who is writing a long description will want to
/// know the bound before the host refuses to start, and the two numbers are the
/// whole of the rule.
#[allow(
    dead_code,
    reason = "read by a plugin's own tests, which live outside this crate"
)]
pub const LIMITS: Limits = Limits {
    name_characters: MAXIMUM_PLUGIN_NAME_CHARS,
    description_characters: MAXIMUM_PLUGIN_DESCRIPTION_CHARS,
};

/// The lengths a plugin's own copy is held to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub name_characters: usize,
    pub description_characters: usize,
}

impl Descriptor {
    /// Whether this descriptor names no icon at all.
    ///
    /// A test asserts on this rather than reaching into the field, so that adding a
    /// third icon form is a change here rather than a change to every assertion.
    pub fn has_no_icon(&self) -> bool {
        self.icon.emoji.is_none() && self.icon.image.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_call_descriptor_is_a_whole_plugin() {
        let descriptor = Descriptor::new("pomodoro", "Pomodoro");
        assert_eq!(descriptor.id(), "pomodoro");
        assert_eq!(descriptor.name().resolve("zh-CN"), "Pomodoro");
        assert_eq!(
            *descriptor.plugin_version(),
            bongocat_plugin_protocol::PluginVersion::new(1, 0, 0)
        );
        assert!(descriptor.has_no_icon());
        assert!(descriptor.subscriptions().is_empty());
        assert!(descriptor.check().is_ok());
    }

    #[test]
    fn an_icon_is_one_emoji_and_the_image_half_exists_for_the_day_it_is_needed() {
        let descriptor = Descriptor::new("pomodoro", "Pomodoro").icon("🍅");
        assert_eq!(descriptor.icon.emoji.as_deref(), Some("🍅"));
        assert!(descriptor.icon.image.is_none());

        let with_image = Descriptor::new("pomodoro", "Pomodoro").icon_value(
            bongocat_plugin_protocol::PluginIcon {
                emoji: None,
                image: Some("icon.png".to_string()),
            },
        );
        assert_eq!(
            with_image.icon.image.as_deref(),
            Some("icon.png"),
            "the image half is already in the protocol and the host already reads it"
        );
    }

    #[test]
    fn a_subscription_listed_twice_is_stored_once() {
        let descriptor = Descriptor::new("key-stats", "Key stats")
            .subscribe(Subscription::Input)
            .subscribe(Subscription::Input);
        assert_eq!(descriptor.subscriptions().len(), 1);
        assert!(descriptor.subscribes_to(Subscription::Input));
        assert!(!descriptor.subscribes_to(Subscription::HostState));
    }

    #[test]
    fn an_id_the_store_could_not_hold_is_refused_by_the_plugins_own_check() {
        for id in ["Key Stats", "../escape", "a/b", ""] {
            let descriptor = Descriptor::new(id, "Name");
            assert!(
                descriptor.check().is_err(),
                "{id:?} cannot be a directory name, so it cannot be a plugin id"
            );
        }
        assert!(Descriptor::new("key-stats", "Name").check().is_ok());
    }

    #[test]
    fn a_name_or_description_too_long_is_refused_by_the_plugins_own_check() {
        let long_name = "x".repeat(LIMITS.name_characters + 1);
        assert!(Descriptor::new("a-plugin", &long_name).check().is_err());
        let long_description = "x".repeat(LIMITS.description_characters + 1);
        assert!(
            Descriptor::new("a-plugin", "Name")
                .description(&long_description)
                .check()
                .is_err()
        );
        assert!(
            Descriptor::new("a-plugin", "  ").check().is_err(),
            "a blank name is not a name"
        );
    }

    #[test]
    fn a_settings_schema_the_plugin_cannot_build_is_refused_by_its_own_check() {
        let descriptor = Descriptor::new("a-plugin", "Name")
            .settings(Settings::new().with(crate::Integer::ranged("n", "N", 0, 1, 10).into()));
        assert!(descriptor.check().is_err());
    }

    #[test]
    fn a_tick_converts_to_the_units_a_plugin_actually_uses() {
        let tick = Tick {
            elapsed_ms: 65_400,
            state: bongocat_plugin_protocol::HostState::new(Some("Cat".to_string()), true),
            clock: bongocat_plugin_protocol::WallClock::new(9, 5, 3),
        };
        assert_eq!(tick.elapsed_seconds(), 65);
        assert_eq!(tick.sub_second(), 400);
        assert!(tick.is_overlay_visible());
        assert_eq!(
            tick.time_text(),
            "09:05:03",
            "and the clock is formatted once here, so every clock a plugin shows is spelled the \
             same way"
        );
    }

    #[test]
    fn the_display_of_a_descriptor_reads_the_way_a_card_does() {
        let descriptor = Descriptor::new("pomodoro", "Pomodoro").version(2, 1, 0);
        assert_eq!(descriptor.to_string(), "Pomodoro 2.1.0");
    }
}
