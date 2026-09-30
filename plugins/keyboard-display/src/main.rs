//! A panel that shows the keys you are holding.
//!
//! Issue #74 asked for the keys on screen "in a corner of the desktop". This is that, and
//! it is the first plugin whose whole life is a stream of events rather than a clock, so
//! it is where the input subscription gets exercised for real.
//!
//! * **All of the state is here.** Which keys are held, in the order they were pressed,
//!   in this process. Nothing about "is shift down" is asked of the host, because the host
//!   does not keep a pressed set to give — it keeps one to *drive the cat*.
//! * **All of the layout is here.** The panel has no notion of wrapping; a plugin that
//!   wants three rows of four has to count, and counting is the plugin's business because
//!   the keys' widths are the plugin's business.
//! * **All of the copy is here**, in [`copy`].
//! * **All of the settings are here** — three switches, and the window renders them.
//!
//! The one thing it asks of the host is the input feed and a panel to draw. It cannot ask
//! to see another key's state, cannot ask what the model is doing, and cannot ask for a
//! key to be released: every pressed key is cleared by a release, by a reset, or by the
//! process ending, and those are the only three ways it can end.

mod copy;

use bongocat_plugin_sdk::prelude::*;

/// The keys shown when the user has not chosen.
///
/// Eight, because that is two tidy rows on a panel of this size and because a display that
/// has to scroll to show what your hands are doing is a display you cannot read while
/// typing.
const DEFAULT_MAXIMUM_KEYS: i64 = 8;

/// The most keys this plugin will show.
///
/// A bound rather than a preference: a hundred keycaps is a hundred labels, the panel
/// would be taller than the model window, and the host would be laying out a scene with a
/// thousand nodes at a hundred and twenty times a second. Sixteen is past what a person's
/// hands cover and short of what a window cannot show.
const MAXIMUM_KEYS: i64 = 16;

/// How many keys fit on one row.
///
/// Derived from the panel's width and one key's width, both of which are this plugin's
/// numbers: the panel is this plugin's, and the keycap is the product's font at the size
/// this plugin chose. A host that decided it would mean a host that had to know what a
/// keycap is.
const PANEL_WIDTH: f32 = 260.0;
const KEY_WIDTH: f32 = 34.0;
const PANEL_PADDING: f32 = 14.0;
const PANEL_HEIGHT: u32 = 150;

/// What the user configured.
///
/// Read through the SDK's typed accessors, so a field nobody has touched reads as its own
/// default and there is no `unwrap_or` written twice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub maximum_keys: usize,
    pub include_mouse: bool,
    pub hide_when_idle: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            maximum_keys: DEFAULT_MAXIMUM_KEYS as usize,
            include_mouse: true,
            hide_when_idle: false,
        }
    }
}

impl Preferences {
    fn read(values: &Values) -> Self {
        Self {
            maximum_keys: values.integer("maximum_keys").clamp(1, MAXIMUM_KEYS) as usize,
            include_mouse: values.flag("include_mouse"),
            hide_when_idle: values.flag("hide_when_idle"),
        }
    }
}

/// The settings this plugin declares, which *are* the settings panel.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Integer::ranged(
                "maximum_keys",
                copy::maximum_keys_label(),
                DEFAULT_MAXIMUM_KEYS,
                1,
                MAXIMUM_KEYS,
            )
            .described(copy::maximum_keys_help())
            .into(),
        )
        .with(
            Toggle::new("include_mouse", copy::mouse_label())
                .described(copy::mouse_help())
                .into(),
        )
        .with(
            Toggle::new("hide_when_idle", copy::hide_when_idle_label())
                .described(copy::hide_when_idle_help())
                .into(),
        )
}

/// The whole plugin.
pub struct KeyDisplay {
    /// The panel this plugin draws.
    panel: Panel,
    /// The keys currently held, oldest first.
    ///
    /// A `Vec` rather than a set because the order is the display: the keys you pressed
    /// most recently are the ones you are most likely to be explaining, so a set that sorted
    /// them would put `A` before `Z` on every row forever.
    held: Vec<String>,
    /// What the user configured.
    preferences: Preferences,
    /// Whether the panel is up, so a hide and a show are one fact rather than two.
    showing: bool,
    /// The keys the panel last showed, so a tick that changed nothing builds nothing.
    painted: Option<Vec<String>>,
}

impl KeyDisplay {
    /// A display at rest, with a starting guess at the user's settings.
    pub fn new(preferences: Preferences) -> Self {
        Self {
            panel: Panel::new(PANEL_WIDTH as u32, PANEL_HEIGHT)
                .anchored(PluginAnchor::BottomRight)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.30)
                .with_opacity(0.94),
            held: Vec::new(),
            preferences,
            showing: false,
            painted: None,
        }
    }

    /// A key went down.
    ///
    /// Auto-repeat is ignored: the keyboard saying the same thing again is not a second
    /// key, and a display that showed `A A A` while one key was held would be showing the
    /// keyboard's timer rather than the person's hands.
    fn press(&mut self, control: String) {
        if self.held.contains(&control) {
            return;
        }
        self.held.push(control);
        // The oldest go first, so the panel keeps the keys a person is most likely to be
        // pressing *now* — and so a long chord does not push the key you just pressed off
        // the display.
        let keep = self.preferences.maximum_keys;
        if self.held.len() > keep {
            let excess = self.held.len() - keep;
            self.held.drain(..excess);
        }
    }

    /// A key came up.
    fn release(&mut self, control: &str) {
        self.held.retain(|held| held != control);
    }

    /// The platform forgot what was held.
    ///
    /// The whole set goes, not a guess at which keys: a reconciliation is the platform
    /// saying its own pressed set is not what it told anybody, and the only set this plugin
    /// can be sure of afterwards is the empty one. A tally that keeps a key the platform
    /// has already forgotten is a display that lies until the key is pressed again.
    fn forget_everything(&mut self) {
        self.held.clear();
    }

    /// One event, as this plugin reacts to it.
    fn react(&mut self, event: &InputEvent) {
        match event {
            InputEvent::KeyDown { control, repeat } => {
                if !repeat {
                    self.press(control.clone());
                }
            }
            InputEvent::KeyUp { control } => self.release(control),
            InputEvent::MouseButton { button, pressed } => {
                if !self.preferences.include_mouse {
                    return;
                }
                if *pressed {
                    self.press(button.to_owned());
                } else {
                    self.release(button);
                }
            }
            InputEvent::Reset { .. } => self.forget_everything(),
            // Pointer movement is not this plugin's business. A display of held keys that
            // also showed a cursor would be a second cursor, and the model window already
            // has one.
            InputEvent::MouseMove { .. } => {}
        }
    }

    /// The keys as keycaps, oldest first.
    fn labels(&self) -> Vec<String> {
        self.held
            .iter()
            .map(|control| control_label(control).to_owned())
            .collect()
    }

    /// How many keycaps fit on one row of this panel.
    ///
    /// Arithmetic rather than a constant, because both numbers are this plugin's: the panel
    /// is this plugin's and the keycap is the product's font at the size this plugin chose.
    /// A host that decided it would be a host that had to know what a keycap is.
    fn columns(&self) -> usize {
        let usable = PANEL_WIDTH - PANEL_PADDING * 2.0;
        ((usable / KEY_WIDTH).floor() as usize).max(1)
    }

    /// Rebuild the panel if what it would show has changed, and put it up or take it down.
    fn draw(&mut self, host: &mut Host) {
        let visible = !self.held.is_empty() || !self.preferences.hide_when_idle;
        if !visible {
            if self.showing {
                host.hide_panel();
                self.showing = false;
                self.painted = None;
            }
            return;
        }
        let labels = self.labels();
        if self.painted.as_ref() == Some(&labels) && self.showing {
            return;
        }
        let idle = self.held.is_empty();
        let note = if idle {
            copy::say(host, &copy::idle())
        } else {
            String::new()
        };
        let columns = self.columns();
        self.panel.rebuild(|panel| {
            panel.surface(6.0, [PANEL_PADDING, 10.0], |content| {
                if idle {
                    content.push(muted(&note, 12.0));
                    return;
                }
                for row in labels.chunks(columns) {
                    content.row_spaced(4.0, |line| {
                        for key in row {
                            line.push(key_cap(key));
                        }
                    });
                }
            })
        });
        if host.panel(&mut self.panel) || !self.showing {
            self.showing = true;
        }
        self.painted = Some(labels);
    }
}

/// One key, as it looks on a keycap.
///
/// A keycap rather than a label: the point of the display is that it reads as *the key you
/// are pressing*, and a bare word in a row is a list of words. A small surface with the
/// letter centred in it is the difference between a caption and a keyboard.
fn key_cap(label: &str) -> SceneNode {
    chip(label, 13.0, [10.0, 6.0], 6.0)
}

impl Plugin for KeyDisplay {
    fn descriptor(&self) -> Descriptor {
        Descriptor::new("keyboard-display", copy::plugin_name().resolve(""))
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
        self.draw(host);
        Ok(())
    }

    fn on_input(&mut self, events: Vec<InputEvent>, host: &mut Host) {
        for event in &events {
            self.react(event);
        }
        self.draw(host);
    }

    fn on_tick(&mut self, _tick: Tick, host: &mut Host) {
        // A tick draws nothing. The panel changes when a key does, and this plugin is
        // subscribed to the feed, so a redraw here would rebuild the same tree sixty times
        // a second to produce the one the user already has. What a tick *is* good for is
        // the case where the panel is up and the feed has gone quiet — a keyboard unplugged
        // without a reset — and the host's own `Reset` is what covers that.
        self.draw(host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // The bound may have shrunk below what is held, and the panel has to obey it now
        // rather than at the next press.
        let keep = self.preferences.maximum_keys;
        if self.held.len() > keep {
            let excess = self.held.len() - keep;
            self.held.drain(..excess);
        }
        // A mouse button that is held while the setting turns it off is no longer shown, so
        // it has to leave the set rather than come back when it is released.
        if !self.preferences.include_mouse {
            self.held.retain(|control| !is_mouse_button(control));
        }
        self.painted = None;
        self.draw(host);
    }
}

/// Whether a wire name is one of the protocol's mouse buttons.
///
/// The four the protocol names, matched rather than checked against a list of every
/// possible name: a name this build does not know is a key, and a key that happens to be
/// spelled `left_thing` is a key.
fn is_mouse_button(control: &str) -> bool {
    matches!(control, "left" | "right" | "middle" | "back" | "forward")
}

fn main() -> bongocat_plugin_sdk::Result<()> {
    KeyDisplay::new(Preferences::default()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, panels, values_from,
    };
    use bongocat_plugin_sdk::{ConfigSchema, Host, Session};

    fn harness() -> WrittenMessages {
        // A schema is built per `serve` call, because a plugin's own test has no reason to
        // hold one: the settings a session runs with are the point, and they differ per test.
        WrittenMessages::new()
    }

    fn serve(
        plugin: &mut KeyDisplay,
        written: &WrittenMessages,
        locale: &str,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        let host = Host::new(
            written.writer(),
            IdentityBuilder::new()
                .id("keyboard-display")
                .locale(locale)
                .build(),
            schema.clone(),
            values_from(&config, &schema),
        )
        .expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("announced");
        session.serve(plugin, messages).expect("served");
    }

    fn configured(maximum_keys: i64, include_mouse: bool, hide_when_idle: bool) -> ConfigDocument {
        document(
            [
                (
                    "maximum_keys".to_string(),
                    ConfigValue::Integer(maximum_keys),
                ),
                (
                    "include_mouse".to_string(),
                    ConfigValue::Bool(include_mouse),
                ),
                (
                    "hide_when_idle".to_string(),
                    ConfigValue::Bool(hide_when_idle),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }

    fn the_defaults() -> ConfigDocument {
        configured(DEFAULT_MAXIMUM_KEYS, true, false)
    }

    fn a_key(control: &str) -> InputEvent {
        InputEvent::KeyDown {
            control: control.to_owned(),
            repeat: false,
        }
    }

    #[test]
    fn a_key_appears_when_it_goes_down_and_leaves_when_it_comes_up() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(InputEvent::KeyUp {
                    control: "KeyA".to_owned(),
                })
                .into_messages(),
        );
        assert!(
            plugin.held.is_empty(),
            "so the panel is showing no keys rather than a key that is not down"
        );
        let last = panels(&written);
        let labels = labels_in(&last.last().expect("a panel").scene);
        assert!(
            !labels.iter().any(|label| label == "A"),
            "and the last thing drawn does not still show it: {labels:?}"
        );
    }

    #[test]
    fn the_keys_are_shown_in_the_order_they_were_pressed() {
        // A set would sort them, and a sorted row puts `A` before `Z` forever — so the key
        // you pressed most recently, the one you are most likely to be explaining, is the
        // one at the end.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyZ"))
                .input(a_key("KeyA"))
                .input(a_key("KeyM"))
                .into_messages(),
        );
        assert_eq!(plugin.held, ["KeyZ", "KeyA", "KeyM"]);
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert_eq!(
            labels
                .iter()
                .rev()
                .take(3)
                .rev()
                .cloned()
                .collect::<Vec<_>>(),
            ["Z", "A", "M"],
            "so the row reads in the order the hands moved"
        );
    }

    #[test]
    fn a_held_key_the_keyboard_repeats_is_one_key() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(InputEvent::KeyDown {
                    control: "KeyA".to_owned(),
                    repeat: true,
                })
                .input(InputEvent::KeyDown {
                    control: "KeyA".to_owned(),
                    repeat: true,
                })
                .into_messages(),
        );
        assert_eq!(
            plugin.held,
            ["KeyA"],
            "because a display of held keys that shows A A A is showing the keyboard's timer \\
             rather than the person's hands"
        );
    }

    #[test]
    fn a_second_press_of_a_key_already_held_does_not_add_a_second_keycap() {
        let mut plugin = KeyDisplay::new(Preferences::default());
        plugin.press("KeyA".to_owned());
        plugin.press("KeyA".to_owned());
        assert_eq!(plugin.held, ["KeyA"]);
    }

    #[test]
    fn holding_more_keys_than_the_bound_keeps_the_newest() {
        let mut plugin = KeyDisplay::new(Preferences {
            maximum_keys: 3,
            ..Preferences::default()
        });
        for key in ["KeyA", "KeyB", "KeyC", "KeyD", "KeyE"] {
            plugin.press(key.to_owned());
        }
        assert_eq!(
            plugin.held,
            ["KeyC", "KeyD", "KeyE"],
            "so the key you just pressed is never the one that got pushed off the display"
        );
    }

    #[test]
    fn a_release_of_a_key_this_plugin_does_not_hold_changes_nothing() {
        // The host tests a release against the set it believes; a plugin that acted on an
        // unmatched release would be defending against a bug that costs nothing to ignore.
        let mut plugin = KeyDisplay::new(Preferences::default());
        plugin.press("KeyA".to_owned());
        plugin.release("KeyQ");
        assert_eq!(plugin.held, ["KeyA"]);
    }

    #[test]
    fn a_reset_forgets_every_key_because_the_platforms_set_is_not_this_plugins_to_guess() {
        // A lock screen, a sleep, a session switch or an unplugged keyboard all arrive as
        // one event with no detail. A plugin that kept the keys it could account for would
        // be showing keys that are not down until each is pressed again.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("LeftShift"))
                .input(a_key("KeyA"))
                .input(InputEvent::Reset {
                    reason: "session_lock".to_owned(),
                })
                .into_messages(),
        );
        assert!(
            plugin.held.is_empty(),
            "so a locked screen does not leave a shift key drawn for ever"
        );
    }

    #[test]
    fn the_mouse_buttons_are_shown_or_not_according_to_one_setting() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(8, true, false),
            Inbox::new()
                .input(InputEvent::MouseButton {
                    button: "left".to_owned(),
                    pressed: true,
                })
                .into_messages(),
        );
        assert_eq!(plugin.held, ["left"]);
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.iter().any(|label| label == "LMB"),
            "and it reads as a button rather than as the word left: {labels:?}"
        );

        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(8, false, false),
            Inbox::new()
                .input(InputEvent::MouseButton {
                    button: "left".to_owned(),
                    pressed: true,
                })
                .into_messages(),
        );
        assert!(
            plugin.held.is_empty(),
            "a keyboard-only display shows no mouse button, and does not remember one either"
        );
    }

    #[test]
    fn a_pointer_moving_changes_nothing() {
        // The model window already has a cursor. A held-keys display that also showed one
        // would be a second cursor, and one that nobody asked for.
        let mut plugin = KeyDisplay::new(Preferences::default());
        let before = plugin.held.clone();
        plugin.react(&InputEvent::MouseMove {
            dx: 1.0,
            dy: 1.0,
            distance: 1.4,
        });
        assert_eq!(plugin.held, before);
    }

    #[test]
    fn a_panel_can_hide_itself_when_there_is_nothing_to_say() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(8, true, true),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(InputEvent::KeyUp {
                    control: "KeyA".to_owned(),
                })
                .into_messages(),
        );
        assert!(
            !plugin.showing,
            "because a panel that says nothing is a box on the user's desktop"
        );
        assert!(
            bongocat_plugin_sdk::testing::panels(&written).len() < 3,
            "and the idle line is never drawn either, so there is nothing to take down"
        );
    }

    #[test]
    fn an_idle_panel_says_so_rather_than_being_blank() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.iter().any(|label| label == "No keys held"),
            "because an empty box on the desktop is a box somebody has to work out what it \\
             means: {labels:?}"
        );
    }

    #[test]
    fn a_setting_that_shrinks_the_bound_is_obeyed_before_the_next_press() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(a_key("KeyB"))
                .input(a_key("KeyC"))
                .config(configured(1, true, false))
                .into_messages(),
        );
        assert_eq!(
            plugin.held,
            ["KeyC"],
            "and the panel has to obey the new bound now, because the user just set it"
        );
    }

    #[test]
    fn turning_the_mouse_off_removes_a_mouse_button_that_is_already_held() {
        // Otherwise the button comes back the moment it is released — it was in the set the
        // whole time, it was just not drawn, and a keycap that appears on release is a bug
        // a user would report as "it shows a key when I let go of the mouse".
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(InputEvent::MouseButton {
                    button: "left".to_owned(),
                    pressed: true,
                })
                .config(configured(8, false, false))
                .into_messages(),
        );
        assert_eq!(plugin.held, ["KeyA"]);
    }

    #[test]
    fn the_row_breaks_where_the_panel_is_too_narrow_for_another_keycap() {
        // The host's layout has no notion of wrapping, so the plugin counts. Both numbers
        // are this plugin's: the panel is this plugin's, and the keycap is the product's
        // font at the size this plugin chose.
        let plugin = KeyDisplay::new(Preferences::default());
        let columns = plugin.columns();
        assert!(
            (4..=6).contains(&columns),
            "so a panel of {PANEL_WIDTH}px holds {columns} keycaps of {KEY_WIDTH}px and not a \\
             keycap that would be cut off"
        );
    }

    #[test]
    fn the_panel_answers_in_the_language_the_user_reads() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "zh-CN",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.iter().any(|label| label == "未按任何键"),
            "and a keycap is still a keycap in every language: {labels:?}"
        );
    }

    #[test]
    fn a_panel_this_plugin_builds_is_one_the_host_accepts() {
        // The SDK's builders and the protocol's checks are two halves of one contract, and
        // a keycap on a row of keycaps is the shape most likely to break it.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(16, true, false),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(a_key("KeyB"))
                .input(a_key("KeyC"))
                .input(a_key("KeyD"))
                .input(a_key("KeyE"))
                .input(a_key("KeyF"))
                .input(a_key("KeyG"))
                .input(a_key("KeyH"))
                .input(a_key("KeyI"))
                .input(a_key("KeyJ"))
                .input(a_key("KeyK"))
                .input(a_key("KeyL"))
                .input(a_key("KeyM"))
                .into_messages(),
        );
        let panels = panels(&written);
        for panel in &panels {
            panel
                .validate()
                .expect("a panel this plugin builds is one the host accepts");
        }
        assert!(
            panels.len() > 1,
            "and there were several: a full panel and the ones that grew into it"
        );
    }

    #[test]
    fn a_ticket_builds_nothing_when_no_key_changed() {
        // A display that rebuilt its panel on every tick would be a host that rasterized
        // sixty identical panels a second, which is the most expensive thing the product
        // could be asked to do for nothing.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        let mut inbox = Inbox::new().input(a_key("KeyA"));
        for frame in 0..60 {
            inbox = inbox.tick(frame * 16);
        }
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            inbox.into_messages(),
        );
        assert_eq!(
            panels(&written).len(),
            2,
            "one panel from the ready and one from the key, and nothing from the sixty ticks"
        );
    }

    #[test]
    fn the_descriptor_is_valid_and_says_which_feed_it_wants() {
        let plugin = KeyDisplay::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "keyboard-display");
        assert_eq!(descriptor.name().resolve("zh-CN"), "按键显示");
        descriptor
            .check()
            .expect("this plugin's own descriptor is one the host accepts");
        assert!(
            descriptor.subscribes_to(Subscription::Input),
            "a display of held keys that is not told about held keys is a display of nothing"
        );
        assert!(
            !descriptor.subscribes_to(Subscription::HostState),
            "and it has no use for the model's name or the window's visibility"
        );
    }

    #[test]
    fn a_mouse_button_name_is_only_a_mouse_button_when_the_protocol_says_so() {
        assert!(is_mouse_button("left"));
        assert!(is_mouse_button("forward"));
        assert!(!is_mouse_button("KeyA"));
        assert!(
            !is_mouse_button("left_thing"),
            "a name this build does not know is a key, and a key spelled like a button is a key"
        );
    }
}
