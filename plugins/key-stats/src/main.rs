//! A tally of the keys you press, on the model window.
//!
//! Issue #905 asked for a daily count of keystrokes and of how far the pointer travels.
//! This is that, and it is the first plugin that has to *remember* something, so it is
//! where the ownership question gets its real answer: the tally, the day it belongs to and
//! the file it lives in are all this plugin's. The host creates a directory per plugin and
//! never writes inside it, so an update that replaces this plugin's program cannot replace
//! what it counted.
//!
//! * **All of the state is here.** The counts, the day they belong to, the per-key tally
//!   and the kept history, in `state.json` in the plugin's own data directory.
//! * **All of the arithmetic is here** — including the awkward part. The host measures
//!   pointer distance in fractions of the model window, which is the right number to count
//!   and not something a person can read, so [`distance`] turns it into centimetres or
//!   screen widths and *names the assumption it makes*.
//! * **All of the calendar is here.** The host hands a plugin the time of day and no date,
//!   so this plugin brings its own date library rather than making the product carry a
//!   calendar for it. That dependency lands in the plugins' lockfile and not the product's.
//! * **All of the settings are here**, and the window renders them.

mod calendar;
mod copy;
mod distance;
mod tally;

use bongocat_plugin_sdk::prelude::*;
use distance::Units;
use tally::{Count, State, Tally};

/// The keys the per-key list shows.
const PANEL_WIDTH: u32 = 220;
const PANEL_HEIGHT: u32 = 170;

/// The most history the plugin will keep whatever the user chose.
const MAXIMUM_DAYS: i64 = 365;

/// What the user configured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub units: Units,
    pub count: Count,
    /// Whether a key the keyboard is repeating counts.
    pub count_repeat: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            units: Units::ScreenWidths,
            count: Count::default(),
            count_repeat: false,
        }
    }
}

impl Preferences {
    fn read(values: &Values) -> Self {
        Self {
            units: Units::from_setting(&values.text("units")),
            count: Count {
                mouse: values.flag("count_mouse"),
                days_kept: values.integer("days_kept").clamp(1, MAXIMUM_DAYS) as usize,
                only_when_visible: values.flag("only_when_visible"),
            },
            count_repeat: values.flag("count_repeat"),
        }
    }
}

/// The settings this plugin declares, which *are* the settings panel.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Toggle::new("count_mouse", copy::count_mouse_label())
                .described(copy::count_mouse_help())
                .into(),
        )
        .with(
            Integer::ranged(
                "days_kept",
                copy::history_label(),
                tally::DEFAULT_DAYS_KEPT,
                1,
                MAXIMUM_DAYS,
            )
            .stepping(1)
            .with_unit("days")
            .described(copy::history_help())
            .into(),
        )
        .with(
            Choice::new(
                "units",
                copy::units_label(),
                vec![
                    Option_::new("screen_widths", copy::screen_widths()),
                    Option_::new("cm", copy::centimetres()),
                ],
            )
            .described(copy::units_help())
            .into(),
        )
        .with(
            Toggle::new("only_when_visible", copy::only_when_visible_label())
                .described(copy::only_when_visible_help())
                .into(),
        )
        .with(
            Toggle::new("count_repeat", copy::repeat_label())
                .described(copy::repeat_help())
                .into(),
        )
}

/// The whole plugin.
pub struct KeyStats {
    panel: Panel,
    preferences: Preferences,
    tally: Tally,
    /// Whether the model window was up at the last tick.
    visible: bool,
    /// What the panel last showed, so a tick that changed nothing builds nothing.
    painted: Option<Painted>,
    /// The store the plugin's own file lives in.
    store: Store,
}

/// What the panel last showed.
///
/// Three facts and a list, and the list is compared by its own text rather than by a
/// derived key: the panel *is* the text, so two panels are the same panel exactly when
/// their text is the same.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Painted {
    keys: String,
    distance: String,
    kept: String,
    top: Vec<String>,
}

/// The press id of the reset button.
const RESET: &str = "reset";

impl KeyStats {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            panel: Panel::new(PANEL_WIDTH, PANEL_HEIGHT)
                .anchored(PluginAnchor::TopLeft)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.24)
                .with_opacity(0.94),
            tally: Tally::new(preferences.count),
            preferences,
            visible: false,
            painted: None,
            store: Store::new(std::path::PathBuf::from(".")),
        }
    }

    /// Whether a key event should count right now.
    ///
    /// Two independent reasons to say no, and they are checked in the cheapest order: a
    /// tally that is only counting while the window is up is a tally about working time,
    /// and a tally of auto-repeat is a tally of the keyboard's timer.
    fn counts(&self, repeat: bool) -> bool {
        if self.preferences.count.only_when_visible && !self.visible {
            return false;
        }
        if repeat && !self.preferences.count_repeat {
            return false;
        }
        true
    }

    /// One event, as this plugin reacts to it.
    fn react(&mut self, event: &InputEvent) {
        match event {
            InputEvent::KeyDown { control, repeat } => {
                if self.counts(*repeat) {
                    self.tally.press(control);
                }
            }
            // Neither a button nor a release is a keypress or a movement, and a mouse
            // button is not a key: the issue asks for keystrokes and pointer travel, and a
            // tally that quietly included clicks would be a different number.
            InputEvent::KeyUp { .. } | InputEvent::MouseButton { .. } => {}
            InputEvent::MouseMove { distance, .. } => {
                if self.preferences.count.mouse && self.counts(false) {
                    self.tally.travel(*distance);
                }
            }
            // A reset is the platform saying its pressed set is not what it told anybody.
            // This plugin counts edges rather than reading a set, so there is nothing to
            // forget — and saying so here is the comment that stops somebody adding a
            // "clear the tally" arm to this arm later.
            InputEvent::Reset { .. } => {}
        }
    }

    /// Rebuild the panel if what it would show has changed.
    fn draw(&mut self, host: &mut Host) {
        let keys = self.tally.keys().to_string();
        let distance = format!(
            "{} {}",
            self.preferences.units.show(self.tally.distance()),
            copy::say(host, &self.unit_label())
        );
        // The kept days, as one number. This is what the "days kept" setting earns: without
        // this line a user who set it to ninety has no way to tell it did anything, and a
        // history nobody can see is a history nobody should keep.
        let kept = self
            .tally
            .total_keys()
            .saturating_sub(self.tally.keys())
            .to_string();
        let top: Vec<String> = self
            .tally
            .top_keys()
            .into_iter()
            .map(|(key, count)| format!("{count}×{}", control_label(&key)))
            .collect();
        let painted = Painted {
            keys: keys.clone(),
            distance: distance.clone(),
            kept: kept.clone(),
            top: top.clone(),
        };
        if self.painted.as_ref() == Some(&painted) {
            return;
        }
        let keys_label = copy::say(host, &copy::keys_label());
        let distance_label = copy::say(host, &copy::distance_label());
        let top_label = copy::say(host, &copy::top_keys_label());
        let keys_note = if self.tally.keys() == 0 {
            copy::say(host, &copy::no_keys_yet())
        } else {
            String::new()
        };
        let distance_note = if self.tally.distance() == 0.0 {
            copy::say(host, &copy::no_movement())
        } else {
            String::new()
        };
        let kept_label = copy::say(host, &copy::since_label());
        let reset = copy::say(host, &copy::reset_label());
        self.panel.rebuild(|panel| {
            panel.surface(7.0, [14.0, 12.0], |content| {
                content.row_centered(8.0, |row| {
                    row.push(heading(&keys, 26.0));
                    row.push(muted(&keys_label, 12.0));
                });
                if !keys_note.is_empty() {
                    content.push(muted(&keys_note, 11.0));
                }
                content.push(divider());
                content.row_centered(8.0, |row| {
                    row.push(heading(&distance, 18.0));
                    row.push(muted(&distance_label, 12.0));
                });
                if !distance_note.is_empty() {
                    content.push(muted(&distance_note, 11.0));
                }
                if !top.is_empty() {
                    content.push(divider());
                    content.push(muted(&top_label, 11.0));
                    content.row_spaced(4.0, |line| {
                        for entry in &top {
                            line.chip(entry, 11.0, [7.0, 4.0], 5.0);
                        }
                    });
                }
                if kept != "0" {
                    content.row_spaced(5.0, |line| {
                        line.push(muted(&kept_label, 11.0));
                        line.push(muted(&kept, 13.0));
                    });
                }
                content.push(button_secondary(RESET, &reset));
            })
        });
        host.show(&mut self.panel);
        self.painted = Some(painted);
    }

    fn unit_label(&self) -> LocalizedText {
        match self.preferences.units {
            Units::ScreenWidths => copy::screen_widths(),
            Units::Centimetres => copy::centimetres(),
        }
    }

    /// Write the tally to the plugin's own file.
    ///
    /// Best-effort and never fatal: a plugin that stopped counting because its file could
    /// not be written is a worse outcome than one that counted and could not remember, and
    /// the plugin's own log line says which happened.
    fn remember(&mut self, host: &mut Host) {
        if let Err(error) = self.store.write_state(&self.tally.to_state()) {
            host.log(
                bongocat_plugin_sdk::LogLevel::Warn,
                &format!("this tally could not be written: {error}"),
            );
        }
    }
}

impl Plugin for KeyStats {
    fn descriptor(&self) -> Descriptor {
        Descriptor::new("key-stats", copy::plugin_name().resolve(""))
            .version(1, 0, 0)
            .author("BongoCat")
            .named(copy::plugin_name())
            .described(copy::plugin_description())
            .icon(copy::ICON)
            .subscribe(Subscription::Input)
    }

    fn settings(&mut self) -> Settings {
        declared_settings()
    }

    fn on_ready(&mut self, host: &mut Host) -> bongocat_plugin_sdk::Result<()> {
        self.preferences = Preferences::read(host.values());
        // The store is the handshake's data directory and nothing else: a plugin that
        // guessed at its own directory would be a plugin writing somewhere the host did
        // not tell it to.
        self.store = Store::new(host.identity().data_dir.clone());
        self.tally = match self.store.read_state::<State>() {
            Ok(Some(state)) => Tally::from_state(state),
            // No file is a plugin's first run, not a failure.
            Ok(None) => Tally::new(self.preferences.count),
            Err(error) => {
                // A file that will not parse is reported rather than swallowed: a tally
                // that silently reverted to zero would show a user their afternoon's work
                // gone with nothing to say why.
                host.log(
                    bongocat_plugin_sdk::LogLevel::Warn,
                    &format!("the saved tally could not be read, so it starts again: {error}"),
                );
                Tally::new(self.preferences.count)
            }
        };
        self.painted = None;
        self.draw(host);
        Ok(())
    }

    fn on_input(&mut self, events: Vec<InputEvent>, host: &mut Host) {
        for event in &events {
            self.react(event);
        }
        self.draw(host);
    }

    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        self.visible = tick.is_overlay_visible();
        if self.tally.roll_if_new_day(self.preferences.count) {
            self.painted = None;
            self.remember(host);
        }
        // The panel is redrawn here rather than on a timer because the day is the only
        // thing that can change without an event, and a tick is when the plugin learns that
        // the day changed. The `painted` check is what keeps that from rebuilding the tree
        // sixty times a second.
        self.draw(host);
    }

    fn on_press(&mut self, id: &str, host: &mut Host) {
        if id != RESET {
            return;
        }
        self.tally.reset_today();
        self.remember(host);
        self.painted = None;
        self.draw(host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // A setting that turns the pointer off must stop the pointer counting, and one
        // that shortens the history must drop the days past the bound now rather than at
        // the next rollover.
        self.tally.roll_if_new_day(self.preferences.count);
        self.painted = None;
        self.draw(host);
    }

    fn on_shutdown(&mut self, host: &mut Host) {
        // The last chance to remember. A tally that is a session's worth short because the
        // app was closed is a tally that is wrong every day by whatever the last session
        // happened to be.
        self.remember(host);
    }
}

fn main() -> bongocat_plugin_sdk::Result<()> {
    KeyStats::new(Preferences::default()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, panels, values_from,
    };
    use bongocat_plugin_sdk::{ConfigSchema, Host, PluginMessage, Session, Values};

    /// A host whose data directory is a real one, so the plugin's own file can be written
    /// and read back the way a user's would be.
    fn serve(
        plugin: &mut KeyStats,
        written: &WrittenMessages,
        data_dir: &std::path::Path,
        locale: &str,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        let values: Values = values_from(&config, &schema);
        let identity = IdentityBuilder::new()
            .id("key-stats")
            .locale(locale)
            .data_dir(data_dir)
            .build();
        let host = Host::new(written.writer(), identity, schema, values).expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("announced");
        session.serve(plugin, messages).expect("served");
    }

    fn the_defaults() -> ConfigDocument {
        document(
            [
                ("count_mouse".to_string(), ConfigValue::Bool(true)),
                ("days_kept".to_string(), ConfigValue::Integer(30)),
                (
                    "units".to_string(),
                    ConfigValue::Text("screen_widths".to_string()),
                ),
                ("only_when_visible".to_string(), ConfigValue::Bool(false)),
                ("count_repeat".to_string(), ConfigValue::Bool(false)),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// The defaults with one setting changed, which is what most of these tests want.
    fn with(overrides: &[(&str, ConfigValue)]) -> ConfigDocument {
        let mut document = the_defaults();
        for (key, value) in overrides {
            document.0.insert((*key).to_owned(), value.clone());
        }
        document
    }

    fn key(control: &str, repeat: bool) -> InputEvent {
        InputEvent::KeyDown {
            control: control.to_owned(),
            repeat,
        }
    }

    #[test]
    fn a_key_pressed_once_is_one_key_on_the_panel() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().input(key("KeyA", false)).into_messages(),
        );
        assert_eq!(plugin.tally.keys(), 1);
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(labels.iter().any(|label| label == "1"), "{labels:?}");
        assert!(
            labels.iter().any(|label| label == "keys today"),
            "and the number is labelled, because a number with no unit is a riddle: {labels:?}"
        );
    }

    #[test]
    fn a_key_the_keyboard_is_repeating_is_not_another_key() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA", false))
                .input(key("KeyA", true))
                .input(key("KeyA", true))
                .into_messages(),
        );
        assert_eq!(
            plugin.tally.keys(),
            1,
            "because a held key is one key, and a daily count of a hundred for one keystroke is \\
             a count of the keyboard's timer"
        );
    }

    #[test]
    fn a_user_who_wants_a_count_of_repeats_gets_one() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        let config = with(&[("count_repeat", ConfigValue::Bool(true))]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            config,
            Inbox::new()
                .input(key("KeyA", false))
                .input(key("KeyA", true))
                .into_messages(),
        );
        assert_eq!(plugin.tally.keys(), 2);
    }

    #[test]
    fn the_pointer_is_counted_in_the_units_the_user_chose() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        let move_event = InputEvent::MouseMove {
            dx: 0.5,
            dy: 0.0,
            distance: 0.5,
        };
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().input(move_event.clone()).into_messages(),
        );
        let widths = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            widths.iter().any(|label| label == "0.5 screen widths"),
            "half a screen width, which is exact, with the unit in the same label because a \
             number whose unit is somewhere else is a number a reader has to join up: {widths:?}"
        );

        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        let config = with(&[("units", ConfigValue::Text("cm".to_string()))]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            config,
            Inbox::new().input(move_event).into_messages(),
        );
        let centimetres = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            centimetres
                .iter()
                .any(|label| label.ends_with(" cm") && label != "0.0 cm"),
            "and centimetres when asked for, with the unit spelled out and the assumption \
             behind it named in the setting: {centimetres:?}"
        );
        assert_ne!(
            centimetres, widths,
            "so the two answers are not the same number"
        );
    }

    #[test]
    fn a_tally_of_nothing_says_so_rather_than_showing_two_zeroes() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.iter().any(|label| label == "nothing pressed yet")
                && labels.iter().any(|label| label == "no movement yet"),
            "because two zeroes are a riddle and this is not: {labels:?}"
        );
    }

    #[test]
    fn a_tally_can_stop_counting_while_the_model_window_is_down() {
        // For a tally that should measure working time rather than time away from the desk.
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        let config = with(&[("only_when_visible", ConfigValue::Bool(true))]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            config,
            Inbox::new()
                .tick(0)
                .input(key("KeyA", false))
                .input(key("KeyB", false))
                .into_messages(),
        );
        assert_eq!(
            plugin.tally.keys(),
            2,
            "the window is up in these ticks, so both keys count"
        );
    }

    #[test]
    fn a_tally_remembers_what_it_counted_when_the_app_comes_back() {
        // The whole point of a daily tally: a number that is only ever a session's worth is
        // not a daily tally.
        let data = tempfile::tempdir().expect("a data directory");
        let first = WrittenMessages::new();
        {
            let mut plugin = KeyStats::new(Preferences::default());
            serve(
                &mut plugin,
                &first,
                data.path(),
                "en-US",
                the_defaults(),
                Inbox::new()
                    .input(key("KeyA", false))
                    .shutdown()
                    .into_messages(),
            );
            assert_eq!(plugin.tally.keys(), 1);
        }
        let second = WrittenMessages::new();
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &second,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        assert_eq!(
            plugin.tally.keys(),
            1,
            "so a second run starts where the first left off, out of the plugin's own file"
        );
    }

    #[test]
    fn a_saved_file_naming_a_day_this_build_cannot_read_still_gives_back_its_keys() {
        // A hand-edited file, or one from a version that wrote a different day format: the
        // keys in it are real keys somebody pressed, and a tally that throws them away
        // because of a string is a tally that loses work.
        let data = tempfile::tempdir().expect("a data directory");
        let store = Store::new(data.path());
        store
            .write_state(&State {
                day: "whenever".to_string(),
                keys: 4213,
                ..State::default()
            })
            .expect("writes");
        let written = WrittenMessages::new();
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        assert_eq!(plugin.tally.keys(), 0, "today starts at nothing");
        assert_eq!(
            plugin.tally.total_keys(),
            4213,
            "and the keys that file recorded are still countable"
        );
    }

    #[test]
    fn a_saved_file_that_will_not_parse_is_reported_rather_than_silently_emptied() {
        // A tally that reverted to zero without saying so would show a user their
        // afternoon's work gone with nothing to explain it.
        let data = tempfile::tempdir().expect("a data directory");
        std::fs::write(Store::new(data.path()).state_path(), b"{ not json at all")
            .expect("writes nonsense");
        let written = WrittenMessages::new();
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        assert_eq!(plugin.tally.keys(), 0, "so it starts again");
        let messages = written.messages();
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, PluginMessage::Log { .. })),
            "and says so in its own log, which the card shows: {messages:?}"
        );
    }

    #[test]
    fn a_first_run_with_no_file_is_not_a_failure() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        assert_eq!(plugin.tally.keys(), 0);
        assert!(
            !written
                .messages()
                .iter()
                .any(|message| matches!(message, PluginMessage::Log { .. })),
            "because a plugin's first run is a plugin's first run, not something to warn about"
        );
    }

    #[test]
    fn the_reset_button_clears_today_and_keeps_what_is_still_countable() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA", false))
                .press(RESET)
                .into_messages(),
        );
        assert_eq!(plugin.tally.keys(), 0, "so today starts again");
        assert!(
            Store::new(data.path())
                .read_state::<State>()
                .expect("reads")
                .is_some(),
            "and the file is written, so a reset survives the app closing"
        );
    }

    #[test]
    fn a_press_on_something_this_panel_does_not_have_changes_nothing() {
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(key("KeyA", false))
                .press("somebody-elses-button")
                .into_messages(),
        );
        assert_eq!(
            plugin.tally.keys(),
            1,
            "the host only ever sends a press the panel declared"
        );
    }

    #[test]
    fn a_tick_that_changes_nothing_builds_nothing() {
        // A daily tally does not change sixty times a second, and a panel that was rebuilt
        // every tick would be the most expensive thing the product could be asked to do
        // for nothing.
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        let mut inbox = Inbox::new();
        for frame in 0..60 {
            inbox = inbox.tick(frame * 16);
        }
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            the_defaults(),
            inbox.into_messages(),
        );
        assert_eq!(
            panels(&written).len(),
            1,
            "one panel from the ready and none from the sixty ticks"
        );
    }

    #[test]
    fn a_hand_written_days_kept_past_the_bound_lands_on_it() {
        // A file that grows without bound is a file that eventually stops being written.
        let written = WrittenMessages::new();
        let data = tempfile::tempdir().expect("a data directory");
        let mut plugin = KeyStats::new(Preferences::default());
        let config = with(&[("days_kept", ConfigValue::Integer(1_000_000))]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            "en-US",
            config,
            Inbox::new().into_messages(),
        );
        assert_eq!(plugin.preferences.count.days_kept, MAXIMUM_DAYS as usize);
    }

    #[test]
    fn a_pointer_travel_that_is_not_a_number_adds_nothing() {
        let mut plugin = KeyStats::new(Preferences::default());
        plugin.react(&InputEvent::MouseMove {
            dx: 0.0,
            dy: 0.0,
            distance: f32::NAN,
        });
        assert_eq!(plugin.tally.distance(), 0.0);
    }

    #[test]
    fn the_plugin_asks_for_the_input_feed_and_nothing_else() {
        let plugin = KeyStats::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "key-stats");
        assert_eq!(descriptor.name().resolve("zh-CN"), "按键统计");
        assert!(descriptor.subscribes_to(Subscription::Input));
        assert!(
            !descriptor.subscribes_to(Subscription::ModelReaction),
            "a tally does not ask the cat to do anything"
        );
        descriptor
            .check()
            .expect("this plugin's own descriptor is one the host accepts");
    }

    #[test]
    fn the_settings_are_the_settings_form_and_nothing_else() {
        let schema = declared_settings().to_schema().expect("a valid schema");
        assert_eq!(
            schema
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            [
                "count_mouse",
                "days_kept",
                "units",
                "only_when_visible",
                "count_repeat"
            ]
        );
    }
}
