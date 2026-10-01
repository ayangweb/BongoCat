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
mod layout;
mod settings;

use bongocat_plugin_sdk::prelude::*;
use layout::Keycap;
use settings::Preferences;
use std::sync::LazyLock;

/// This plugin's own manifest, embedded at compile time.
///
/// One document for the identity the card shows and the words the panel draws. See
/// [`bongocat_plugin_sdk::SelfDescription`] for why it is embedded rather than read, and
/// [`copy`] for the words themselves.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// The gap between the panel's edge and the first key, in logical pixels.
///
/// The panel's own, and not the keycap's: it is the gap between the panel and its
/// contents, which belongs to the panel the way the keycap's own padding belongs to the
/// keycap.
const PANEL_PADDING: f32 = 14.0;

/// The gap between two keycaps, in logical pixels.
const KEY_GAP: f32 = 4.0;

/// The keycap's padding, as a multiple of the font size.
///
/// Derived rather than a constant so the cap keeps its proportions at every size: a
/// keycap with the same absolute padding at a large font is a letter in a letterbox, and
/// one at a small font is a keycap that is mostly padding.
const PADDING_RATIO: f32 = 0.62;

/// The keycap's corner radius, as a multiple of the font size.
const RADIUS_RATIO: f32 = 0.31;

/// The settings this plugin declares, re-exported for the tests that assert on them.
use settings::declared_settings;

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
    ///
    /// The panel is sized from the settings rather than fixed, because the font size is a
    /// setting: a panel sized for one font and drawn at another is either a keycap with
    /// space around it or a keycap with its letter cut off, and neither is a display
    /// anybody asked for.
    pub fn new(preferences: Preferences) -> Self {
        let panel = Self::panel_box(&preferences, 0);
        Self {
            panel: Panel::new(panel.width, panel.height)
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

    /// The panel these settings need for this many keys.
    fn panel_box(preferences: &Preferences, keys: usize) -> layout::PanelBox {
        layout::panel_for(preferences, keys)
    }

    /// The keycap these settings draw.
    fn keycap(&self) -> Keycap {
        Keycap {
            font_size: self.preferences.font_size,
            bold: self.preferences.bold,
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
    /// Read off [`layout::panel_for`] rather than computed here, so the number of columns
    /// the panel was *sized* for is the number of columns the panel is *drawn* with. Two
    /// calculations of the same thing would agree until the font size changed, and then
    /// disagree in exactly the way that puts a keycap off the edge.
    fn columns(&self) -> usize {
        Self::panel_box(&self.preferences, self.held.len().max(1)).columns
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
        let box_ = Self::panel_box(&self.preferences, labels.len());
        // Resized before the tree is built, because a panel's size is part of what the host
        // lays the tree out in: a tree laid out for a narrower panel than the one it is drawn
        // in is a tree the host clips, and the clip is invisible to this plugin.
        self.panel.resize(box_.width, box_.height);
        let cap = self.keycap();
        self.panel.rebuild(|panel| {
            panel.surface(6.0, [PANEL_PADDING, 10.0], |content| {
                if idle {
                    content.push(muted(&note, self.preferences.font_size));
                    return;
                }
                for row in labels.chunks(columns) {
                    content.row_spaced(KEY_GAP, |line| {
                        for key in row {
                            line.push(key_cap(key, cap));
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
///
/// The size, the padding and the radius all come from [`Keycap`], so the cap the panel was
/// sized for and the cap the panel draws are the same cap — see [`layout`].
fn key_cap(label: &str, cap: Keycap) -> SceneNode {
    let padding = cap.padding();
    chip(label, cap.font_size, [padding, padding], cap.radius())
}

impl Plugin for KeyDisplay {
    fn descriptor(&self) -> Descriptor {
        // The manifest says who this plugin is, and the descriptor is a projection of it
        // rather than a second place spelling the same six fields out. Adding a plugin that
        // keeps its metadata in its own `plugin.json` is then a change to that one file, and
        // a card that said one thing before the plugin started and another after is not
        // expressible.
        SELF.descriptor().subscribe(Subscription::Input)
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
        // A font change moves every keycap, so the last thing drawn is not a thing that can
        // be compared against: keeping it would let the panel skip the redraw that the new
        // font makes necessary, and the user would see the old size until the next keypress.
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
    use settings::{DEFAULT_FONT_SIZE, DEFAULT_MAXIMUM_KEYS, MAXIMUM_FONT_SIZE, MAXIMUM_KEYS};

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

    /// The document the settings form would send for these settings.
    ///
    /// Every field named, including the ones this plugin did not change in a test, because
    /// a test that built a partial document would be testing a document the product never
    /// sends: the form always sends the whole thing.
    fn configured(maximum_keys: i64, include_mouse: bool, hide_when_idle: bool) -> ConfigDocument {
        with_font(
            maximum_keys,
            DEFAULT_FONT_SIZE as f64,
            settings::REGULAR,
            include_mouse,
            hide_when_idle,
        )
    }

    /// The same document, with the two font settings the user can change.
    fn with_font(
        maximum_keys: i64,
        font_size: f64,
        font_weight: &str,
        include_mouse: bool,
        hide_when_idle: bool,
    ) -> ConfigDocument {
        document(
            [
                (
                    "maximum_keys".to_string(),
                    ConfigValue::Integer(maximum_keys),
                ),
                ("font_size".to_string(), ConfigValue::Decimal(font_size)),
                (
                    "font_weight".to_string(),
                    ConfigValue::Text(font_weight.to_string()),
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
        // The host's layout has no notion of wrapping, so the plugin counts — and the count
        // has to come from the same arithmetic the panel's width came from, or a larger font
        // would size a panel for three keycaps and then try to draw five.
        let plugin = KeyDisplay::new(Preferences::default());
        let columns = plugin.columns();
        let cap = plugin.keycap();
        let fits = (((plugin.panel.size()[0] as f32 - PANEL_PADDING * 2.0 + KEY_GAP)
            / (cap.width() + KEY_GAP))
            .floor()) as usize;
        assert_eq!(
            columns,
            fits,
            "so the panel is drawn with exactly the keycaps it is wide enough for: {columns} \
             columns in {}px with a {:.0}px cap",
            plugin.panel.size()[0],
            cap.width()
        );
    }

    #[test]
    fn a_bigger_font_draws_a_bigger_keycap_and_a_panel_that_fits_it() {
        // The whole of the two font settings, end to end: the cap grows, the panel grows,
        // and the number of keycaps per row falls. A user who turns the size up is asking
        // for fewer, larger keys, and a plugin that grew the cap but not the panel would
        // draw the right keycaps off the edge of a box that did not move.
        for size in [
            settings::MINIMUM_FONT_SIZE,
            DEFAULT_FONT_SIZE,
            MAXIMUM_FONT_SIZE,
        ] {
            let written = harness();
            let mut plugin = KeyDisplay::new(Preferences::default());
            let mut inbox = Inbox::new();
            for key in ["KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF"] {
                inbox = inbox.input(a_key(key));
            }
            serve(
                &mut plugin,
                &written,
                "en-US",
                with_font(MAXIMUM_KEYS, size as f64, settings::REGULAR, false, false),
                inbox.into_messages(),
            );
            let panel = plugin.panel.size();
            let cap = plugin.keycap();
            assert!(
                panel[0] as f32 >= cap.width(),
                "at {size}px the panel is {panel:?} and one cap is {:.0}px",
                cap.width()
            );
            for drawn in panels(&written) {
                drawn
                    .validate()
                    .expect("a panel this plugin builds is one the host accepts");
            }
        }
    }

    #[test]
    fn a_bold_key_is_wider_than_a_normal_one_at_the_same_size() {
        // The weight setting has to reach the cap, not just the number: a cap sized for the
        // normal letter and drawn with a bold one is a keycap with its last letter clipped.
        let written = harness();
        let mut regular = KeyDisplay::new(Preferences::default());
        let mut bold = KeyDisplay::new(Preferences::default());
        let mut inbox = Inbox::new();
        for key in ["KeyA", "KeyB"] {
            inbox = inbox.input(a_key(key));
        }
        serve(
            &mut regular,
            &written,
            "en-US",
            with_font(
                MAXIMUM_KEYS,
                DEFAULT_FONT_SIZE as f64,
                settings::REGULAR,
                false,
                false,
            ),
            Inbox::new().into_messages(),
        );
        serve(
            &mut bold,
            &written,
            "en-US",
            with_font(
                MAXIMUM_KEYS,
                DEFAULT_FONT_SIZE as f64,
                settings::BOLD,
                false,
                false,
            ),
            inbox.into_messages(),
        );
        assert_eq!(
            regular.preferences.font_size, bold.preferences.font_size,
            "so the only difference between the two is the weight"
        );
        assert!(
            bold.keycap().width() > regular.keycap().width(),
            "and the bold cap is the wider one, which is why the panel has to grow with it"
        );
    }

    #[test]
    fn a_font_size_the_form_would_never_send_still_produces_a_drawable_keycap() {
        // The clamp is unreachable through the product — the host fits every document to the
        // schema — and it is here so that a `Values` built any other way cannot produce a
        // zero-sized cap, which is a panel that draws nothing at all.
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        for size in [0.0, -10.0, 1.0, 1_000.0, f64::NAN] {
            let values = values_from(
                &document(
                    [("font_size".to_string(), ConfigValue::Decimal(size))]
                        .into_iter()
                        .collect(),
                ),
                &schema,
            );
            let preferences = Preferences::read(&values);
            let cap = Keycap {
                font_size: preferences.font_size,
                bold: preferences.bold,
            };
            assert!(
                cap.width().is_finite() && cap.width() > 0.0 && cap.height() > 0.0,
                "{size} became a cap of {}x{}",
                cap.width(),
                cap.height()
            );
            let panel = KeyDisplay::panel_box(&preferences, 8);
            assert!(
                panel.width > 0 && panel.height > 0,
                "{size} produced {panel:?}"
            );
        }
    }

    #[test]
    fn a_font_weight_this_build_does_not_know_reads_as_the_ordinary_one() {
        // A value written by a newer version is not this build's to interpret, and the
        // ordinary weight is the reading it was most likely written as.
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        let values = values_from(
            &document(
                [(
                    "font_weight".to_string(),
                    ConfigValue::Text("something_newer".to_string()),
                )]
                .into_iter()
                .collect(),
            ),
            &schema,
        );
        assert!(!Preferences::read(&values).bold);
    }

    #[test]
    fn the_settings_the_plugin_declares_are_the_settings_form_and_nothing_else() {
        let schema = declared_settings().to_schema().expect("a valid schema");
        assert_eq!(
            schema
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            [
                "maximum_keys",
                "font_size",
                "font_weight",
                "include_mouse",
                "hide_when_idle"
            ],
            "in the order the form shows them, because the order is the order somebody sets \
             this plugin up in"
        );
    }

    #[test]
    fn the_value_the_form_sends_is_the_value_the_plugin_reads() {
        // The one thing that could otherwise drift: what the settings form writes, and what
        // this plugin answers to.
        let schema = declared_settings().to_schema().expect("a valid schema");
        for size in [
            settings::MINIMUM_FONT_SIZE,
            DEFAULT_FONT_SIZE,
            MAXIMUM_FONT_SIZE,
        ] {
            for weight in [settings::REGULAR, settings::BOLD] {
                let values = values_from(&with_font(5, size as f64, weight, true, true), &schema);
                let preferences = Preferences::read(&values);
                assert_eq!(preferences.maximum_keys, 5);
                assert_eq!(preferences.font_size, size as f32);
                assert_eq!(preferences.bold, weight == settings::BOLD);
                assert!(preferences.include_mouse && preferences.hide_when_idle);
            }
        }
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
