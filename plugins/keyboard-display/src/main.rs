//! A panel that shows the keys you press.
//!
//! Issue #74 asked for the keys on screen "in a corner of the desktop". This is that, and
//! it is the first plugin whose whole life is a stream of events rather than a clock, so
//! it is where the input subscription gets exercised for real.
//!
//! # What it shows
//!
//! **The keys you pressed, not the keys you are holding.** This follows KeyCastr, and the
//! difference is the whole feature: a panel of *held* keys is empty for the ninety
//! milliseconds a fast keystroke is down, which on a screencast is a panel that is never
//! there. A transcript stays, so what a viewer saw is what you pressed.
//!
//! # All of the state is here
//!
//! * **Which keys are down**, in this process. Nothing about "is shift down" is asked of the
//!   host, because the host does not keep a pressed set to give — it keeps one to *drive
//!   the cat*, and it does not hand it to anybody.
//! * **The line of keycaps on screen**, and when it began and when it ends.
//! * **The layout**, which has no host notion of wrapping: a plugin that wants three rows of
//!   four has to count, and counting is the plugin's business because the keys' widths are
//!   the plugin's business.
//! * **The copy**, in [`copy`], and **the settings**, which the window renders.
//!
//! The one thing it asks of the host is the input feed and a panel to draw. It cannot ask
//! to see another key's state, cannot ask what the model is doing, and cannot ask for a key
//! to be released: a key it believes is down is cleared by a release, by a reset, or by the
//! process ending, and those are the only three ways that can happen. What it cannot be told
//! is where its panel goes — it names one corner and pins itself there, because a corner the
//! user could move is a corner that is wrong half the time.

mod copy;
mod layout;
mod settings;

use bongocat_plugin_sdk::prelude::*;
use layout::Keycap;
use settings::Preferences;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// This plugin's own manifest, embedded at compile time.
///
/// One document for the identity the card shows and the words the panel draws. See
/// [`bongocat_plugin_sdk::SelfDescription`] for why it is embedded rather than read, and
/// [`copy`] for the words themselves.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// Where this plugin's panel sits in the model window.
///
/// One corner, and it is not a preference. A key display a viewer has to find is not a key
/// display, so the panel is pinned to the top left and the user is offered no position to
/// move it to — which the host honours from the descriptor's pin, not from this constant.
const ANCHOR: PluginAnchor = PluginAnchor::TopLeft;

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

/// The modifier glyphs a chord is written with, in the order a person reads them.
///
/// KeyCastr's order and glyphs: control, option, shift, then command — the one that reads
/// as the order the modifiers are stacked on a keycap rather than the order the platform
/// happened to deliver them in.
///
/// Also the set [`layout`] measures as full-width, which is why the two are one list: a
/// glyph estimated as a letter is a `⌘` with its right-hand side clipped.
pub const MODIFIER_GLYPHS: [char; 4] = ['⌃', '⌥', '⇧', '⌘'];

/// How long a quiet spell lasts before the next key starts a new line.
///
/// KeyCastr's `keystrokeDelay`: a burst of typing is one line, because the keys in a burst
/// are being said together, and a key pressed after a pause is a new sentence. Half a
/// second is a longer pause than any typist's inter-key gap and shorter than anybody's
/// thought between two words.
const LINE_BREAK: Duration = Duration::from_millis(500);

/// How long a line stays up after its last key.
///
/// KeyCastr's `fadeDelay`. This is the plugin's answer to "nothing is held", which is the
/// only answer a key display can give: it is not that nothing is pressed — a line is up for
/// two seconds after the hands have left the keyboard — it is that nothing is *recent*.
const LINE_LIFETIME: Duration = Duration::from_millis(2000);

/// The settings this plugin declares, re-exported for the tests that assert on them.
use settings::declared_settings;

/// The whole plugin.
pub struct KeyDisplay {
    /// The panel this plugin draws.
    panel: Panel,
    /// The keycaps on screen, in the order they were pressed.
    ///
    /// A `Vec` rather than a set because the order is the display: the keys you pressed
    /// most recently are the ones you are most likely to be explaining, so a set that sorted
    /// them would put `A` before `Z` on every row forever.
    shown: Vec<String>,
    /// Which controls are physically down, so a chord knows what to write on its key.
    held: Vec<String>,
    /// What the user configured.
    preferences: Preferences,
    /// Whether the panel is up, so a hide and a show are one fact rather than two.
    showing: bool,
    /// The keycaps the panel last showed, so a tick that changed nothing builds nothing.
    painted: Option<Vec<String>>,
    /// When the last key was pressed, or `None` while there is nothing to show.
    ///
    /// The whole of the lifetime is measured from here, which is why it is one field rather
    /// than a countdown the tick decrements: the display's timing must not drift with the
    /// host's tick cadence.
    pressed_at: Option<Duration>,
    /// When this plugin started, which is what [`Self::now`] counts from.
    started: Instant,
}

impl KeyDisplay {
    /// A display at rest, with a starting guess at the user's settings.
    ///
    /// The panel is sized from the settings rather than fixed, because the font size is a
    /// setting: a panel sized for one font and drawn at another is either a keycap with
    /// space around it or a keycap with its letter cut off, and neither is a display
    /// anybody asked for.
    pub fn new(preferences: Preferences) -> Self {
        let panel = Self::panel_box(&preferences, &[]);
        Self {
            panel: Panel::new(panel.width, panel.height)
                .anchored(ANCHOR)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.30)
                .with_opacity(0.94),
            shown: Vec::new(),
            held: Vec::new(),
            preferences,
            showing: false,
            painted: None,
            pressed_at: None,
            started: Instant::now(),
        }
    }

    /// This plugin's own reading of how long it has been running.
    ///
    /// Its own monotonic clock rather than the tick's `elapsed_ms`, for one reason: the host
    /// sends a tick at most every quarter of a second, so a plugin deciding "was that a
    /// pause?" from a tick would be deciding it from a reading up to a quarter of a second
    /// old — which at a typist's speed is enough to break a line in the middle of a word.
    /// A `Duration` rather than an `Instant` throughout, so a test can move time forward
    /// instead of waiting two seconds for a line to expire.
    fn now(&self) -> Duration {
        self.started.elapsed()
    }

    /// The panel these settings need for these labels.
    fn panel_box(preferences: &Preferences, labels: &[String]) -> layout::PanelBox {
        layout::panel_for(preferences, labels)
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
    ///
    /// The chord is composed *before* the key joins the held set, because a key is not part
    /// of its own modifier prefix: `⇧` then `⇧A`, and never `⇧⇧A`.
    fn press(&mut self, control: String, now: Duration) {
        // A key this plugin already holds is an edge it has already reported. The host can
        // deliver one — a keyboard that loses a release is the whole of issue #47 — and a
        // display that answered it would show `A A` for one key and restart its lifetime on
        // every repeat the platform invents.
        if self.held.contains(&control) {
            return;
        }
        // The chord is composed *before* the key joins the held set, because a key is not part
        // of its own modifier prefix: `⇧` then `⇧A`, and never `⇧⇧A`.
        let cap = self.chord_cap(&control);
        self.held.push(control);
        // A command is something you did on purpose, so it reads on its own rather than at
        // the end of whatever you were typing when you remembered it. KeyCastr breaks the
        // line for the same reason, and for the same chord: the modifiers, not the key.
        if is_command_chord(&cap) || self.quiet_for(now) >= LINE_BREAK {
            self.shown.clear();
        }
        self.shown.push(cap);
        // The oldest go first, so the panel keeps the keys a person is most likely to be
        // pressing *now* — and so a long chord does not push the key you just pressed off
        // the display.
        self.bound();
        self.pressed_at = Some(now);
    }

    /// How long the keys have been quiet, or the longest gap there could be if nothing has
    /// been pressed yet.
    ///
    /// Saturation rather than an option because a plugin that has never seen a key is not
    /// in the middle of a line, so "quiet since the beginning" and "quiet for a very long
    /// time" are the same answer.
    fn quiet_for(&self, now: Duration) -> Duration {
        self.pressed_at
            .map_or(Duration::MAX, |pressed_at| now.saturating_sub(pressed_at))
    }

    /// A key came up.
    ///
    /// It leaves the held set and stays on the panel: what the viewer needs to see is the
    /// key that was pressed, and a cap that vanished the instant a finger lifted is a cap
    /// nobody could read.
    fn release(&mut self, control: &str) {
        self.held.retain(|held| held != control);
    }

    /// A button went down or up.
    ///
    /// The same shape as a key: a press is shown and remembered, a release is only remembered
    /// — so two clicks are two caps on the line, which is what a person clicking twice did.
    fn mouse(&mut self, button: String, pressed: bool, now: Duration) {
        if !self.preferences.include_mouse {
            return;
        }
        if pressed {
            if self.held.contains(&button) {
                return;
            }
            self.held.push(button.clone());
            self.shown.push(control_label(&button).to_owned());
            self.bound();
            self.pressed_at = Some(now);
        } else {
            self.release(&button);
        }
    }

    /// Drop the oldest keycaps until the panel is within the user's bound.
    fn bound(&mut self) {
        let keep = self.preferences.maximum_keys;
        if self.shown.len() > keep {
            let excess = self.shown.len() - keep;
            self.shown.drain(..excess);
        }
    }

    /// The platform forgot what was held.
    ///
    /// The whole set goes, not a guess at which keys: a reconciliation is the platform
    /// saying its own pressed set is not what it told anybody, and the only set this plugin
    /// can be sure of afterwards is the empty one. A tally that keeps a key the platform
    /// has already forgotten is a display that lies until the key is pressed again.
    ///
    /// The line stays. It is a record of what was pressed, and a lock screen does not
    /// un-press it — what it means is that the next chord must not be composed from keys
    /// this plugin believes are down when they are not.
    fn forget_everything(&mut self) {
        self.held.clear();
    }

    /// One event, as this plugin reacts to it.
    fn react(&mut self, event: &InputEvent, now: Duration) {
        match event {
            InputEvent::KeyDown { control, repeat } => {
                if !repeat {
                    self.press(control.clone(), now);
                }
            }
            InputEvent::KeyUp { control } => self.release(control),
            InputEvent::MouseButton { button, pressed } => {
                self.mouse(button.clone(), *pressed, now);
            }
            InputEvent::Reset { .. } => self.forget_everything(),
            // Pointer movement is not this plugin's business. A display of pressed keys that
            // also showed a cursor would be a second cursor, and the model window already
            // has one.
            InputEvent::MouseMove { .. } => {}
        }
    }

    /// The one keycap for `control`, written as it would read on a key: the modifiers down
    /// at the moment it went down, then the key.
    ///
    /// A chord is one cap rather than several because a chord is one thing the person did —
    /// `⌘S` is not four keys, and a viewer reading four separate caps has to reassemble it
    /// themselves.
    fn chord_cap(&self, control: &str) -> String {
        if let Some(glyph) = modifier_glyph(control) {
            // A modifier pressed on its own is its own cap, which is also what a viewer
            // needs to see: they pressed shift, then pressed something else with it, and
            // the `⇧` before the `⇧A` is what tells them the order.
            return glyph.to_string();
        }
        let mut cap = String::new();
        for glyph in MODIFIER_GLYPHS {
            if self
                .held
                .iter()
                .any(|held| modifier_glyph(held) == Some(glyph))
            {
                cap.push(glyph);
            }
        }
        cap.push_str(control_label(control));
        cap
    }

    /// Take the panel down if the line it was showing has run out.
    ///
    /// The one thing a tick is for. Everything else about this display changes when a key
    /// changes, and this plugin is subscribed to the feed, so a redraw on every tick would
    /// rebuild the same tree sixty times a second to produce the one the user already has.
    fn expire(&mut self, now: Duration) -> bool {
        let Some(pressed_at) = self.pressed_at else {
            return false;
        };
        if now.saturating_sub(pressed_at) < LINE_LIFETIME {
            return false;
        }
        self.shown.clear();
        self.pressed_at = None;
        true
    }

    /// Rebuild the panel if what it would show has changed, and put it up or take it down.
    fn draw(&mut self, host: &mut Host) {
        // Nothing to say is nothing drawn. The alternative — a panel saying "no keys held"
        // for as long as the plugin runs — is a box on the user's desktop that costs them
        // the top-left corner of their model window and tells them nothing, so it is not a
        // setting: there is only one behaviour and it is this one.
        if self.shown.is_empty() {
            if self.showing {
                host.hide_panel();
                self.showing = false;
                self.painted = None;
            }
            return;
        }
        if self.painted.as_ref() == Some(&self.shown) && self.showing {
            return;
        }
        // One calculation for both answers, because the number of columns the panel is
        // *sized* for is the number it is *drawn* with. Two would agree until the font
        // changed and then disagree in exactly the way that puts a keycap off the edge.
        let box_ = Self::panel_box(&self.preferences, &self.shown);
        let columns = box_.columns;
        // Resized before the tree is built, because a panel's size is part of what the host
        // lays the tree out in: a tree laid out for a narrower panel than the one it is drawn
        // in is a tree the host clips, and the clip is invisible to this plugin.
        self.panel.resize(box_.width, box_.height);
        let cap = self.keycap();
        let shown = self.shown.clone();
        self.panel.rebuild(|panel| {
            panel.surface(6.0, [PANEL_PADDING, 10.0], |content| {
                for row in shown.chunks(columns) {
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
        self.painted = Some(shown);
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

/// The glyph a modifier key is written with, or `None` when it is not a modifier.
///
/// Both spellings the artwork has, because both arrive on the wire and a person pressing
/// shift has one key.
fn modifier_glyph(control: &str) -> Option<char> {
    Some(match control {
        "LeftControl" | "ControlLeft" | "RightControl" | "ControlRight" => '⌃',
        "LeftAlt" | "AltLeft" | "RightAlt" | "AltRight" => '⌥',
        "LeftShift" | "ShiftLeft" | "RightShift" | "ShiftRight" => '⇧',
        // The artwork spells the platform's own key both ways, and a screencast is watched
        // on whichever machine is recording: the one that has a Windows key is the one
        // whose name is not "command".
        "LeftMeta" | "MetaLeft" | "RightMeta" | "MetaRight" | "LeftGUI" | "GuiLeft"
        | "RightGUI" | "GuiRight" => '⌘',
        _ => return None,
    })
}

/// Whether a chord reads as a command, and so starts its own line.
///
/// KeyCastr's test is control **or** command held, which is the useful one on either
/// platform: both are the modifiers a shortcut is built from, and neither is one you hold by
/// accident mid-sentence. Option and shift are left out — `⇧A` belongs with the word being
/// typed, and `⌥C` on a macOS layout is an ordinary character.
///
/// Asked of the *cap* rather than of the key, because the chord is what makes a command: `S`
/// is a letter and `⌘S` is a shortcut, and the rule cannot tell them apart without the
/// modifiers.
fn is_command_chord(cap: &str) -> bool {
    cap.starts_with('⌃') || cap.starts_with('⌘')
}

/// One key, as it looks on a keycap.
///
/// A keycap rather than a label: the point of the display is that it reads as *the key you
/// pressed*, and a bare word in a row is a list of words. A small surface with the label
/// centred in it is the difference between a caption and a keyboard.
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
        //
        // The panel is **pinned** rather than placed. It has a place — the top left — and the
        // user is not offered a menu to move it to, because a key display a viewer has to
        // find on the screen is not doing the job it is installed for. Pinning is also what
        // keeps the corner reserved, so another panel cannot land on top of it.
        SELF.descriptor()
            .pins_panel(ANCHOR)
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
        // One reading for the whole batch: the batch is one moment as far as this plugin is
        // concerned, and reading the clock per event would let a boundary fall between two
        // events the host had already decided were simultaneous.
        let now = self.now();
        for event in &events {
            self.react(event, now);
        }
        self.draw(host);
    }

    fn on_tick(&mut self, _tick: Tick, host: &mut Host) {
        // A tick draws nothing else. The panel changes when a key does, and this plugin is
        // subscribed to the feed, so a redraw here would rebuild the same tree sixty times a
        // second to produce the one the user already has. What a tick *is* good for is the
        // one thing that happens without an event: the line running out of time.
        if self.expire(self.now()) {
            self.draw(host);
        }
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // The bound may have shrunk below what is shown, and the panel has to obey it now
        // rather than at the next press.
        self.bound();
        // A mouse button that is held while the setting turns it off is no longer shown, so
        // it has to leave the held set rather than come back when it is released.
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
    /// Every field named, because the form always sends the whole thing: a test that built
    /// a partial document would be testing a document the product never sends.
    fn configured(maximum_keys: i64, include_mouse: bool) -> ConfigDocument {
        with_font(
            maximum_keys,
            DEFAULT_FONT_SIZE as f64,
            settings::REGULAR,
            include_mouse,
        )
    }

    /// The same document, with the two font settings the user can change.
    fn with_font(
        maximum_keys: i64,
        font_size: f64,
        font_weight: &str,
        include_mouse: bool,
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
            ]
            .into_iter()
            .collect(),
        )
    }

    fn the_defaults() -> ConfigDocument {
        configured(DEFAULT_MAXIMUM_KEYS, true)
    }

    fn a_key(control: &str) -> InputEvent {
        InputEvent::KeyDown {
            control: control.to_owned(),
            repeat: false,
        }
    }

    fn up(control: &str) -> InputEvent {
        InputEvent::KeyUp {
            control: control.to_owned(),
        }
    }

    fn mouse(button: &str, pressed: bool) -> InputEvent {
        InputEvent::MouseButton {
            button: button.to_owned(),
            pressed,
        }
    }

    /// The keycaps the panel last drew.
    fn drawn(written: &WrittenMessages) -> Vec<String> {
        labels_in(&panels(written).last().expect("a panel").scene)
    }

    /// A display whose clock a test controls.
    ///
    /// The plugin reads its own monotonic clock rather than the host's tick — see
    /// [`KeyDisplay::now`] — so a test about a two-second lifetime cannot drive it through
    /// messages. It drives [`KeyDisplay::press`] and [`KeyDisplay::expire`] directly with
    /// the moment each event happened, which is also how a reader can see the timing
    /// written down rather than waited for.
    fn at(seconds: u64) -> KeyDisplay {
        let mut plugin = KeyDisplay::new(Preferences::default());
        plugin.started = Instant::now() - Duration::from_secs(seconds);
        plugin
    }

    #[test]
    fn a_key_stays_on_the_panel_after_it_comes_up() {
        // The whole difference from a display of held keys: the cap outlives the finger, so
        // a viewer watching a screencast sees what was pressed rather than a panel that is
        // only up while somebody is typing.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(up("KeyA"))
                .into_messages(),
        );
        assert!(plugin.held.is_empty(), "so the key is genuinely up");
        assert_eq!(plugin.shown, ["A"], "and the keycap is still on the panel");
        assert!(plugin.showing, "which means the panel is still up");
    }

    #[test]
    fn the_panel_takes_itself_down_when_the_line_has_run_out() {
        // There is no setting for this any more: a line outlives the key, so "nothing is
        // held" is not the question — "nothing is recent" is, and after two seconds of quiet
        // the honest answer is an empty corner rather than a box on the desktop.
        let mut plugin = at(0);
        plugin.press("KeyA".to_owned(), Duration::ZERO);
        plugin.release("KeyA");
        assert!(
            !plugin.expire(LINE_LIFETIME - Duration::from_millis(1)),
            "and it is still up just before that, because a display that goes early is worse \
             than one that lingers"
        );
        assert!(
            plugin.expire(LINE_LIFETIME),
            "and it is due on the tick at the lifetime"
        );
        assert!(
            plugin.shown.is_empty(),
            "because two seconds after the last key there is nothing left to show"
        );
        assert_eq!(plugin.shown, Vec::<String>::new());
    }

    #[test]
    fn a_burst_of_typing_is_one_line() {
        // The reason there is a line at all rather than one key: a burst is one thing being
        // said, and breaking it between every character would be a display nobody can read.
        let mut plugin = at(0);
        for (index, key) in ["KeyZ", "KeyA", "KeyM"].into_iter().enumerate() {
            let when = Duration::from_millis(index as u64 * 100);
            plugin.press(key.to_owned(), when);
        }
        assert_eq!(
            plugin.shown,
            ["Z", "A", "M"],
            "so the row reads in the order the hands moved, at a typist's speed"
        );
    }

    #[test]
    fn a_key_after_a_pause_starts_a_new_line() {
        let mut plugin = at(0);
        plugin.press("KeyA".to_owned(), Duration::ZERO);
        plugin.press(
            "KeyB".to_owned(),
            Duration::from_millis(LINE_BREAK.as_millis() as u64),
        );
        assert_eq!(
            plugin.shown,
            ["B"],
            "because a key pressed after a pause is a new sentence, and a line holding both \
             would say the two were typed together"
        );
    }

    #[test]
    fn a_command_always_starts_its_own_line() {
        // A shortcut is something done on purpose, so it reads alone rather than at the end
        // of whatever was being typed when the person remembered it.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(a_key("LeftMeta"))
                .input(a_key("KeyS"))
                .input(up("KeyS"))
                .input(up("LeftMeta"))
                .into_messages(),
        );
        assert_eq!(
            plugin.shown,
            ["⌘S"],
            "which is one cap saying one shortcut, not a word with a command stuck to it"
        );
    }

    #[test]
    fn a_modifier_and_the_key_it_was_held_with_are_two_caps_then_one() {
        // KeyCastr's shape, and the reason a chord is one cap: the `⇧` before the `⇧A` is
        // what tells a viewer the shift came first, and the `⇧A` is the shortcut itself.
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
                .into_messages(),
        );
        assert_eq!(plugin.shown, ["⇧", "⇧A"]);
    }

    #[test]
    fn a_chord_reads_as_one_cap_with_every_modifier_on_it() {
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("LeftControl"))
                .input(a_key("LeftAlt"))
                .input(a_key("LeftShift"))
                .input(a_key("LeftMeta"))
                .input(a_key("KeyS"))
                .into_messages(),
        );
        assert_eq!(
            plugin.shown.last().map(String::as_str),
            Some("⌃⌥⇧⌘S"),
            "in the order the glyphs stack on a keycap, whatever order the platform sent \
             them in — and on its own line, because the ⌘ deliberately broke the line when \
             it went down"
        );
    }

    #[test]
    fn a_modifier_that_is_not_down_writes_nothing_on_the_caps_after_it() {
        // The one chord rule that is easy to get wrong: a key released before the next one is
        // pressed is not part of that one's chord, or a whole word would be shown as though
        // shift had been held down through it.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("LeftShift"))
                .input(up("LeftShift"))
                .input(a_key("KeyB"))
                .into_messages(),
        );
        assert_eq!(
            plugin.shown.last().map(String::as_str),
            Some("B"),
            "because the shift was up before the B was pressed"
        );
    }

    #[test]
    fn the_windows_key_spells_the_same_glyph_as_the_command_key() {
        // A screencast is watched on whichever machine is recording, and the key is called
        // something different on each of them. Both spellings are in the artwork, so both
        // arrive here.
        for name in ["LeftMeta", "MetaLeft", "RightMeta", "LeftGUI", "GuiLeft"] {
            assert_eq!(modifier_glyph(name), Some('⌘'), "{name}");
        }
    }

    #[test]
    fn only_a_control_or_a_command_starts_a_new_line() {
        // Option and shift are not commands: on a macOS layout `⌥C` is an ordinary
        // character, and `⇧A` belongs with the word being typed.
        assert!(is_command_chord("⌘S"));
        assert!(is_command_chord("⌃⌥⇧⌘S"));
        assert!(!is_command_chord("⌥C"));
        assert!(!is_command_chord("⇧A"));
        assert!(!is_command_chord("A"), "because a letter is not a shortcut");
    }

    #[test]
    fn a_key_the_keyboard_repeats_is_one_key() {
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
            plugin.shown,
            ["A"],
            "because a display that showed A A A is showing the keyboard's timer rather than \
             the person's hands"
        );
    }

    #[test]
    fn a_second_press_of_a_key_already_held_does_not_add_a_second_keycap() {
        let mut plugin = KeyDisplay::new(Preferences::default());
        plugin.press("KeyA".to_owned(), Duration::ZERO);
        plugin.press("KeyA".to_owned(), Duration::ZERO);
        assert_eq!(plugin.shown, ["A"]);
    }

    #[test]
    fn showing_more_keys_than_the_bound_keeps_the_newest() {
        let mut plugin = KeyDisplay::new(Preferences {
            maximum_keys: 3,
            ..Preferences::default()
        });
        for key in ["KeyA", "KeyB", "KeyC", "KeyD", "KeyE"] {
            plugin.press(key.to_owned(), Duration::ZERO);
        }
        assert_eq!(
            plugin.shown,
            ["C", "D", "E"],
            "so the key you just pressed is never the one that got pushed off the display"
        );
    }

    #[test]
    fn a_release_of_a_key_this_plugin_does_not_hold_changes_nothing() {
        // The host tests a release against the set it believes; a plugin that acted on an
        // unmatched release would be defending against a bug that costs nothing to ignore.
        let mut plugin = KeyDisplay::new(Preferences::default());
        plugin.press("KeyA".to_owned(), Duration::ZERO);
        plugin.release("KeyQ");
        assert_eq!(plugin.held, ["KeyA"]);
        assert_eq!(
            plugin.shown,
            ["A"],
            "and the display is a record of presses, so an unmatched release takes nothing \
             off it"
        );
    }

    #[test]
    fn a_reset_forgets_the_held_keys_and_leaves_the_record_alone() {
        // A lock screen, a sleep, a session switch or an unplugged keyboard all arrive as
        // one event with no detail. The held set has to go, or the next chord is composed
        // from keys that are not down; the line has not, because a lock screen does not
        // un-press what was pressed.
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
            "so a locked screen does not leave a shift key in the set the next chord is \
             composed from"
        );
        assert_eq!(
            plugin.shown,
            ["⇧", "⇧A"],
            "while the record of what was pressed stays: a lock screen does not un-press it, \
             and the shift is in the `⇧A` because it was down when the A was pressed"
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
            configured(8, true),
            Inbox::new().input(mouse("left", true)).into_messages(),
        );
        assert_eq!(plugin.shown, ["LMB"]);
        assert!(
            drawn(&written).iter().any(|label| label == "LMB"),
            "and it reads as a button rather than as the word left"
        );

        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(8, false),
            Inbox::new().input(mouse("left", true)).into_messages(),
        );
        assert!(
            plugin.shown.is_empty(),
            "a keyboard-only display shows no mouse button, and draws nothing at all"
        );
        assert!(plugin.held.is_empty(), "and does not remember one either");
    }

    #[test]
    fn two_clicks_are_two_caps_and_a_release_is_neither() {
        // A release only leaves the held set. Were a release a cap too, clicking once would
        // put `LMB` on the line twice, and a display that shows the same key for every edge
        // of one click is showing the mouse's plumbing.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(8, true),
            Inbox::new()
                .input(mouse("left", true))
                .input(mouse("left", false))
                .input(mouse("left", true))
                .into_messages(),
        );
        assert_eq!(
            plugin.shown,
            ["LMB", "LMB"],
            "because two clicks are two things that happened"
        );
    }

    #[test]
    fn a_pointer_moving_changes_nothing() {
        // The model window already has a cursor. A key display that also showed one would be
        // a second cursor, and one nobody asked for.
        let mut plugin = KeyDisplay::new(Preferences::default());
        let before = plugin.shown.clone();
        plugin.react(
            &InputEvent::MouseMove {
                dx: 1.0,
                dy: 1.0,
                distance: 1.4,
            },
            Duration::ZERO,
        );
        assert_eq!(plugin.shown, before);
    }

    #[test]
    fn nothing_is_drawn_before_the_first_key() {
        // The plugin starts with nothing to say, so it starts with no panel rather than one
        // waiting to be filled in.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        assert!(!plugin.showing);
        assert!(
            panels(&written).is_empty(),
            "because a panel that says nothing is a box on the user's desktop"
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
                .config(configured(1, true))
                .into_messages(),
        );
        assert_eq!(
            plugin.shown,
            ["C"],
            "and the panel has to obey the new bound now, because the user just set it"
        );
    }

    #[test]
    fn turning_the_mouse_off_removes_a_mouse_button_that_is_already_held() {
        // Otherwise the button comes back the moment it is released — it was in the set the
        // whole time, it was just not drawn, and a keycap that appears on release is a bug a
        // user would report as "it shows a key when I let go of the mouse".
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new()
                .input(a_key("KeyA"))
                .input(mouse("left", true))
                .config(configured(8, false))
                .into_messages(),
        );
        assert_eq!(plugin.held, ["KeyA"]);
    }

    #[test]
    fn the_row_breaks_where_the_panel_is_too_narrow_for_another_keycap() {
        // The host's layout has no notion of wrapping, so the plugin counts — and the count
        // has to come from the same arithmetic the panel's width came from, or a larger font
        // would size a panel for three keycaps and then try to draw five.
        for size in [
            settings::MINIMUM_FONT_SIZE,
            DEFAULT_FONT_SIZE,
            MAXIMUM_FONT_SIZE,
        ] {
            let mut plugin = KeyDisplay::new(Preferences {
                font_size: size as f32,
                maximum_keys: MAXIMUM_KEYS as usize,
                ..Preferences::default()
            });
            for key in ["KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF"] {
                plugin.press(key.to_owned(), Duration::ZERO);
            }
            let box_ = KeyDisplay::panel_box(&plugin.preferences, &plugin.shown);
            let cap_width = plugin.keycap().width_of("A");
            let fits = (((box_.width as f32 - PANEL_PADDING * 2.0 + KEY_GAP)
                / (cap_width + KEY_GAP))
                .floor()) as usize;
            assert!(
                box_.columns <= fits,
                "at {size}px the panel is {box_:?}, a cap is {cap_width:.0}px, and the row is \
                 cut at a number of caps the panel cannot hold"
            );
            assert!(
                box_.columns == fits.min(plugin.shown.len()),
                "and the row is as full as the panel allows rather than one cap short: \
                 {box_:?} with six keys"
            );
        }
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
                with_font(MAXIMUM_KEYS, size as f64, settings::REGULAR, false),
                inbox.into_messages(),
            );
            let panel = plugin.panel.size();
            let cap = plugin.keycap();
            assert!(
                panel[0] as f32 >= cap.width_of("A"),
                "at {size}px the panel is {panel:?} and one cap is {:.0}px",
                cap.width_of("A")
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
        let mut regular = KeyDisplay::new(Preferences::default());
        let mut bold = KeyDisplay::new(Preferences::default());
        regular.preferences.bold = false;
        bold.preferences.bold = true;
        assert_eq!(regular.preferences.font_size, bold.preferences.font_size);
        assert!(
            bold.keycap().width_of("A") > regular.keycap().width_of("A"),
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
                cap.width_of("A").is_finite() && cap.width_of("A") > 0.0 && cap.height() > 0.0,
                "{size} became a cap of {}x{}",
                cap.width_of("A"),
                cap.height()
            );
            let panel = KeyDisplay::panel_box(&preferences, &["A".to_string()]);
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
            ["maximum_keys", "font_size", "font_weight", "include_mouse"],
            "in the order the form shows them, because the order is the order somebody sets \
             this plugin up in — and with no row for whether the panel is up, because there \
             is only one answer to that"
        );
    }

    #[test]
    fn the_value_the_form_sends_is_the_value_the_plugin_reads() {
        // The one thing that could otherwise drift: what the settings form writes, and what
        // this plugin answers to.
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        for size in [
            settings::MINIMUM_FONT_SIZE,
            DEFAULT_FONT_SIZE,
            MAXIMUM_FONT_SIZE,
        ] {
            for weight in [settings::REGULAR, settings::BOLD] {
                for include_mouse in [false, true] {
                    let values =
                        values_from(&with_font(5, size as f64, weight, include_mouse), &schema);
                    let preferences = Preferences::read(&values);
                    assert_eq!(preferences.maximum_keys, 5);
                    assert_eq!(preferences.font_size, size as f32);
                    assert_eq!(preferences.bold, weight == settings::BOLD);
                    assert_eq!(preferences.include_mouse, include_mouse);
                }
            }
        }
    }

    #[test]
    fn a_panel_this_plugin_builds_is_one_the_host_accepts() {
        // The SDK's builders and the protocol's checks are two halves of one contract, and
        // a keycap on a row of keycaps is the shape most likely to break it.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        let mut inbox = Inbox::new();
        for key in [
            "LeftControl",
            "LeftAlt",
            "LeftShift",
            "LeftMeta",
            "KeyA",
            "KeyB",
            "KeyC",
            "KeyD",
            "KeyE",
            "KeyF",
            "KeyG",
            "KeyH",
            "KeyI",
            "KeyJ",
            "KeyK",
            "KeyL",
        ] {
            inbox = inbox.input(a_key(key));
        }
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(16, true),
            inbox.into_messages(),
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
    fn a_tick_builds_nothing_when_the_line_has_not_run_out() {
        // A display that rebuilt its panel on every tick would be a host that rasterized
        // sixty identical panels a second, which is the most expensive thing the product
        // could be asked to do for nothing.
        let written = harness();
        let mut plugin = KeyDisplay::new(Preferences::default());
        let mut inbox = Inbox::new().input(a_key("KeyA"));
        for frame in 0..60 {
            inbox = inbox.tick((frame * 16) as u64);
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
            1,
            "one panel, from the key, and nothing from the sixty ticks: the line has not run \
             out, so there is nothing to redraw"
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
            "a display of pressed keys that is not told about pressed keys is a display of \
             nothing"
        );
        assert!(
            !descriptor.subscribes_to(Subscription::HostState),
            "and it has no use for the model's name or the window's visibility"
        );
    }

    #[test]
    fn this_plugin_pins_its_panel_to_the_top_left_and_the_sound_one_does_not() {
        // The pinned panel is the plugin whose value is being in the same place every time.
        // The user is offered no position to move it to, and — the half that is easy to leave
        // out — the corner is reserved so nothing else is allocated there.
        let plugin = KeyDisplay::new(Preferences::default());
        assert!(
            plugin.descriptor().has_panel(),
            "a display of pressed keys is a panel on the model window"
        );
        assert_eq!(
            plugin.descriptor().pinned_anchor(),
            Some(PluginAnchor::TopLeft),
            "pinned to the corner its author put it in, rather than offered a menu of nine"
        );
        plugin
            .descriptor()
            .check()
            .expect("and it is a descriptor the host accepts");
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
