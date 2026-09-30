//! A panel that shows which input method you are typing with.
//!
//! Issue #849 asked to see the current input method on macOS. This is that, and it is the
//! shortest of the plugins because the host already has the answer: every tick carries the
//! input method the system reported, with the system's own name for it in the user's own
//! language.
//!
//! * **All of the logic is here** — when to show it, what to shorten a long name to, and
//!   what counts as "a switch".
//! * **All of the copy is here**, and there is very little of it, because the label on the
//!   panel is not this plugin's string: it is `微信输入法` or `かな` or `ABC`, read from the
//!   system. A plugin that kept its own table of input methods would be wrong about every
//!   one it had not heard of, and there are thousands.
//! * **All of the settings are here**, four switches, and the window renders them.
//!
//! What it asks of the host is one fact and a panel. It cannot ask which application has
//! focus, cannot ask what the model is doing, and cannot ask the system anything at all —
//! which is the point, and the reason the fact is in the protocol rather than something this
//! plugin reaches for: the reading is a framework call that is not thread-safe, and the
//! product is the only side that may make it.

mod copy;

use bongocat_plugin_sdk::prelude::*;

/// The motion played when the method changes, when the user asked for that.
///
/// Empty by default: a plugin that starts by moving the cat is a plugin that moves the cat
/// while the user is reading the settings page. The setting turns it on.
const DEFAULT_REACT_MOTION: &str = "";

/// The shortest a shortened name may be.
///
/// One character, because the point of shortening is to fit a keycap-sized label and a name
/// that is still long after shortening has not been shortened.
const MINIMUM_SHORT_NAME: usize = 1;

/// The most characters a name may occupy on the panel.
///
/// Bounded because the panel has a width, and a method with a long name would push every
/// other node off it. Long enough for a Chinese or Japanese name, short enough for a label.
const MAXIMUM_LABEL_CHARS: usize = 12;

const PANEL_WIDTH: u32 = 190;
const PANEL_HEIGHT: u32 = 72;

/// What the user configured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preferences {
    /// Show the panel only while a source that does not type Latin is selected.
    pub only_when_not_latin: bool,
    /// With the above off, whether the Latin method is shown as well.
    pub show_latin: bool,
    /// Whether a long name is shortened to the part after its last dot.
    pub shorten: bool,
    /// The motion to play on a switch, empty for none.
    pub react_motion: String,
    /// Whether a switch moves the cat at all.
    pub react: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        // Showing it only while a non-Latin method is selected is the arrangement people
        // actually want: nothing on screen while typing English, and the method's name the
        // moment they switch away from it.
        Self {
            only_when_not_latin: true,
            show_latin: false,
            shorten: true,
            react_motion: DEFAULT_REACT_MOTION.to_owned(),
            react: false,
        }
    }
}

impl Preferences {
    fn read(values: &Values) -> Self {
        let motion = values.text("react_motion");
        Self {
            only_when_not_latin: values.flag("only_when_not_latin"),
            show_latin: values.flag("show_latin"),
            shorten: values.flag("shorten"),
            // A blank name is no motion, rather than a motion that does not exist: the
            // panel says the setting is off and the model is left alone.
            react_motion: if motion.trim().is_empty() {
                String::new()
            } else {
                motion.trim().to_owned()
            },
            react: values.flag("react"),
        }
    }

    /// The motion to play, or `None` when the user has not asked for one.
    fn motion(&self) -> Option<&str> {
        if self.react && !self.react_motion.is_empty() {
            Some(self.react_motion.as_str())
        } else {
            None
        }
    }
}

/// The settings this plugin declares, which *are* the settings panel.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Toggle::new("only_when_not_latin", copy::only_when_not_latin_label())
                .described(copy::only_when_not_latin_help())
                .into(),
        )
        .with(
            Toggle::new("show_latin", copy::show_latin_label())
                .described(copy::show_latin_help())
                .into(),
        )
        .with(
            Toggle::new("shorten", copy::shorten_label())
                .described(copy::shorten_help())
                .into(),
        )
        .with(
            Toggle::new("react", copy::react_label())
                .described(copy::react_help())
                .into(),
        )
        .with(
            TextField::new("react_motion", copy::react_label())
                .described("A model's motions are named group then number, like CAT_motion.0.")
                .into(),
        )
}

/// What the panel should show, decided from the fact and the settings.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Visible {
    /// A method's label.
    Method(String),
    /// A line saying the platform has no such concept.
    Unsupported,
    /// A line saying a source is selected but has no name.
    NothingSelected,
    /// Nothing: the panel comes down.
    Nothing,
}

/// The whole plugin.
pub struct InputMethodPanel {
    panel: Panel,
    preferences: Preferences,
    /// The method as the last tick reported it.
    current: Option<InputMethod>,
    /// The label last drawn, so a tick that changed nothing builds nothing.
    painted: Option<String>,
    /// Whether the panel is up.
    showing: bool,
    /// The identifier of the method last observed, whether or not it was shown.
    ///
    /// The identifier rather than the drawn label, because "a switch" is a fact about the
    /// keyboard and not about this plugin's display: a name the plugin shortens, clips or
    /// translates changes for reasons of its own, and reacting to those would twitch the
    /// cat when a person changed a setting rather than when they switched keyboards.
    last_seen: Option<String>,
}

impl InputMethodPanel {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            panel: Panel::new(PANEL_WIDTH, PANEL_HEIGHT)
                .anchored(PluginAnchor::TopRight)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.22)
                .with_opacity(0.92),
            preferences,
            current: None,
            painted: None,
            showing: false,
            last_seen: None,
        }
    }

    /// The label for a method, as this plugin's settings ask for it.
    fn label(&self, method: &InputMethod) -> String {
        let name = if self.preferences.shorten {
            shorten(&method.name, &method.id)
        } else {
            method.name.clone()
        };
        clip(&name)
    }

    /// What the panel should show, given the current method and the settings.
    fn visible(&self) -> Visible {
        let Some(method) = &self.current else {
            // No method and no platform fact: a platform with no input methods is a
            // permanent answer, and saying so once is better than showing nothing and
            // leaving a user wondering whether the plugin works.
            return Visible::Unsupported;
        };
        if method.id.is_empty() && method.name.is_empty() {
            return Visible::NothingSelected;
        }
        if self.preferences.only_when_not_latin
            && method.types_latin()
            && !self.preferences.show_latin
        {
            return Visible::Nothing;
        }
        Visible::Method(self.label(method))
    }

    /// Put the panel up or take it down, and react to a switch.
    ///
    /// One pass, in one order, because the two decisions are not independent: a reaction is
    /// for a switch *into something visible*, and the visibility is decided first. So a
    /// switch from a hidden Latin method into a visible one reacts, and a switch into a
    /// hidden one does not — the new method is still remembered, so the next switch back out
    /// reacts as itself rather than as the first thing ever seen.
    fn update(&mut self, host: &mut Host) {
        let visible = self.visible();
        let switched = self.switched();
        if switched {
            // One line per switch, at debug, because the question a user actually asks this
            // plugin is "why is nothing showing?" — and the answer is either the settings or
            // what the system reported, neither of which is otherwise visible anywhere. One
            // line per switch rather than one per tick, so it is readable.
            if let Some(method) = &self.current {
                host.log(
                    LogLevel::Debug,
                    &format!(
                        "the keyboard is now {} ({}){}",
                        method.name,
                        method.id,
                        match &visible {
                            Visible::Method(_) => "",
                            _ => ", which this plugin's settings do not show",
                        }
                    ),
                );
            }
            if let Visible::Method(_) = &visible
                && let Some(motion) = self.preferences.motion()
            {
                host.play_motion(motion);
            }
        }
        match visible {
            Visible::Method(label) => self.draw_line(host, &label),
            // The two permanent "nothing" answers each get one panel of their own, drawn
            // once. "This platform does not report one" and "the method has no name" are
            // different facts, and a plugin that showed nothing for both would be
            // indistinguishable from one that is not running.
            Visible::Unsupported => self.draw_line(host, &copy::say(host, &copy::unsupported())),
            Visible::NothingSelected => {
                self.draw_line(host, &copy::say(host, &copy::nothing_selected()));
            }
            Visible::Nothing => self.hide(host),
        }
    }

    /// Whether the method changed since the last tick, remembering the new one.
    ///
    /// The very first method of a session is not a switch: it is a state the plugin has
    /// caught up with, and moving the cat for it would be a motion at a moment nobody
    /// chose. The method that goes away is not one either — the system stopped reporting,
    /// which is not something a person did to the keyboard.
    fn switched(&mut self) -> bool {
        let Some(method) = &self.current else {
            return false;
        };
        if method.id.is_empty() {
            return false;
        }
        if self.last_seen.as_deref() == Some(method.id.as_str()) {
            return false;
        }
        let was_known = self.last_seen.is_some();
        self.last_seen = Some(method.id.clone());
        was_known
    }

    /// Take the panel down, once.
    fn hide(&mut self, host: &mut Host) {
        if !self.showing {
            return;
        }
        let _ = host.hide_panel();
        self.showing = false;
        self.painted = None;
    }

    /// Draw one line, if it is not already on screen.
    fn draw_line(&mut self, host: &mut Host, text: &str) {
        if self.showing && self.painted.as_deref() == Some(text) {
            return;
        }
        let size = if text.chars().count() > 6 { 18.0 } else { 24.0 };
        let text = text.to_owned();
        self.panel.rebuild(|panel| {
            panel.surface(4.0, [12.0, 10.0], |content| {
                content.chip(&text, size, [12.0, 7.0], 8.0);
            })
        });
        self.showing = true;
        host.show(&mut self.panel);
        self.painted = Some(text);
    }
}

/// The part of a name a reader recognises.
///
/// The system's own name is the right thing to show and is sometimes long — a Chinese method
/// can be named `微信输入法/微信输入法` and an English one `AppleInternal...`. So the part
/// after the last separator is taken, and the result is used only if it is *shorter* than
/// what it came from: a name that is already short is not made shorter by losing a word.
fn shorten(name: &str, id: &str) -> String {
    let candidate = name.rsplit(['.', '/', ' ']).next().unwrap_or(name).trim();
    if candidate.is_empty() || candidate.chars().count() >= name.chars().count() {
        return name.to_owned();
    }
    // The identifier's last component is the fallback when the name has no separator at
    // all, which is the common case for a keyboard layout: `com.apple.keylayout.ABC` has a
    // name of `ABC` and an identifier worth remembering either way.
    if candidate.chars().count() < MINIMUM_SHORT_NAME {
        return id.rsplit('.').next().unwrap_or(id).to_owned();
    }
    candidate.to_owned()
}

/// A label cut to what a chip of this size can show.
fn clip(text: &str) -> String {
    if text.chars().count() <= MAXIMUM_LABEL_CHARS {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(MAXIMUM_LABEL_CHARS - 1).collect();
    out.push('…');
    out
}

impl Plugin for InputMethodPanel {
    fn descriptor(&self) -> Descriptor {
        Descriptor::new("input-method", copy::plugin_name().resolve(""))
            .version(1, 0, 0)
            .author("BongoCat")
            .named(copy::plugin_name())
            .described(copy::plugin_description())
            .icon(copy::ICON)
            // Host state for the fact, model reactions for the optional twitch. No input
            // feed: which input method is selected has nothing to do with which keys are
            // down, and asking for the feed would cost a batch per keystroke for a fact
            // the system publishes on its own.
            .subscribe(Subscription::HostState)
            .subscribe(Subscription::ModelReaction)
    }

    fn settings(&mut self) -> Settings {
        declared_settings()
    }

    fn on_ready(&mut self, host: &mut Host) -> bongocat_plugin_sdk::Result<()> {
        self.preferences = Preferences::read(host.values());
        // The fact arrives on the first tick, so there is nothing to draw yet and the
        // right answer before then is no panel at all rather than a guess.
        let _ = host.hide_panel();
        Ok(())
    }

    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        self.current = tick.state.input_method.clone();
        self.update(host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // The panel has to obey the new settings now, and the label may be a different
        // string, so the comparison starts again.
        self.painted = None;
        self.update(host);
    }
}

fn main() -> bongocat_plugin_sdk::Result<()> {
    InputMethodPanel::new(Preferences::default()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, model_requests, panels,
        values_from,
    };
    use bongocat_plugin_sdk::{ConfigSchema, Host, PluginMessage, Session, Values};

    fn a_method(id: &str, name: &str, latin: bool) -> InputMethod {
        InputMethod {
            id: id.to_owned(),
            name: name.to_owned(),
            ascii_capable: latin,
        }
    }

    fn the_defaults() -> ConfigDocument {
        document(
            [
                ("only_when_not_latin".to_string(), ConfigValue::Bool(true)),
                ("show_latin".to_string(), ConfigValue::Bool(false)),
                ("shorten".to_string(), ConfigValue::Bool(true)),
                ("react".to_string(), ConfigValue::Bool(false)),
                ("react_motion".to_string(), ConfigValue::Text(String::new())),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// The defaults with one setting changed.
    fn with(overrides: &[(&str, ConfigValue)]) -> ConfigDocument {
        let mut document = the_defaults();
        for (key, value) in overrides {
            document.0.insert((*key).to_owned(), value.clone());
        }
        document
    }

    fn serve(
        plugin: &mut InputMethodPanel,
        written: &WrittenMessages,
        locale: &str,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        let values: Values = values_from(&config, &schema);
        let host = Host::new(
            written.writer(),
            IdentityBuilder::new()
                .id("input-method")
                .locale(locale)
                .build(),
            schema,
            values,
        )
        .expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("announced");
        session.serve(plugin, messages).expect("served");
    }

    /// One tick carrying this input method, the way the host sends it.
    fn with_method(elapsed_ms: u64, method: Option<InputMethod>) -> Inbox {
        Inbox::new().tick_with_input_method(elapsed_ms, method)
    }

    #[test]
    fn the_method_name_the_system_gave_is_what_the_panel_shows() {
        // The point of carrying the system's own name: this plugin has no table of input
        // methods, and could not have a correct one.
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            with_method(
                100,
                Some(a_method("com.tencent.wetype.pinyin", "微信输入法", false)),
            )
            .into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.iter().any(|label| label == "微信输入法"),
            "in the system's own words, in the user's own language: {labels:?}"
        );
    }

    #[test]
    fn nothing_is_shown_while_a_method_that_types_latin_is_selected() {
        // The usual arrangement: nothing on screen while typing English.
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            with_method(100, Some(a_method("com.apple.keylayout.ABC", "ABC", true)))
                .into_messages(),
        );
        assert!(
            panels(&written).is_empty(),
            "so a person typing English does not have a chip in the corner of their screen"
        );
    }

    #[test]
    fn a_user_who_wants_to_see_the_latin_method_too_gets_it() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            with(&[
                ("only_when_not_latin", ConfigValue::Bool(false)),
                ("show_latin", ConfigValue::Bool(true)),
            ]),
            with_method(100, Some(a_method("com.apple.keylayout.ABC", "ABC", true)))
                .into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(labels.iter().any(|label| label == "ABC"), "{labels:?}");
    }

    #[test]
    fn a_switch_says_what_the_keyboard_is_now_and_whether_it_is_shown() {
        // The question a user asks this plugin is "why is nothing showing?", and the answer
        // is the settings or the system — neither of which is visible anywhere else. So the
        // switch says both, once, and says when the plugin's own settings are the reason.
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        inbox = inbox.tick_with_input_method(200, Some(a_method("a.abc", "ABC", true)));
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            inbox.into_messages(),
        );
        let logs: Vec<_> = written
            .messages()
            .into_iter()
            .filter_map(|message| match message {
                PluginMessage::Log { message, .. } => Some(message),
                _ => None,
            })
            .collect();
        assert_eq!(
            logs.len(),
            1,
            "one line per switch, not one per tick: {logs:?}"
        );
        assert!(
            logs[0].contains("ABC") && logs[0].contains("which this plugin's settings do not show"),
            "so a hidden switch says why it is hidden rather than looking like a plugin that              stopped: {logs:?}"
        );
    }

    #[test]
    fn a_switch_is_a_changed_keyboard_and_nothing_else() {
        // The plugin's own display changes for its own reasons — a name is shortened, a
        // label is clipped, a setting is changed — and a cat that twitched for any of those
        // would be twitching at settings rather than at the keyboard.
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let pinyin = a_method("com.tencent.wetype.pinyin", "微信输入法", false);
        plugin.current = Some(pinyin.clone());
        assert!(
            !plugin.switched(),
            "the first method of a session is a state the plugin has caught up with"
        );
        assert!(
            !plugin.switched(),
            "and an unchanged method is not a switch"
        );
        plugin.current = Some(a_method(
            "com.tencent.wetype.pinyin",
            "微信输入法/微信输入法",
            false,
        ));
        assert!(
            !plugin.switched(),
            "because the identifier is the same and only the name this plugin draws changed"
        );
        plugin.current = Some(a_method("com.apple.keylayout.ABC", "ABC", true));
        assert!(
            plugin.switched(),
            "whereas a different identifier is a switch, whatever the name is"
        );
    }

    #[test]
    fn a_switch_into_a_hidden_method_is_remembered_rather_than_reacted_to() {
        // Nothing is on screen while a Latin method is selected, so moving the cat there
        // would be moving it where nobody can see it. The switch is still recorded, so
        // switching back out reacts as itself and not as the plugin's first sight of
        // anything.
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        inbox = inbox.tick_with_input_method(200, Some(a_method("a.abc", "ABC", true)));
        inbox = inbox.tick_with_input_method(300, Some(a_method("a.kana", "かな", false)));
        serve(
            &mut plugin,
            &written,
            "en-US",
            with(&[
                ("react", ConfigValue::Bool(true)),
                ("react_motion", ConfigValue::Text("CAT_motion.0".to_owned())),
            ]),
            inbox.into_messages(),
        );
        let requests = model_requests(&written);
        assert_eq!(
            requests.len(),
            1,
            "one motion, for the switch *into* the visible method: the switch into the \\\\n             hidden one is remembered and the switch back out is the one that is seen"
        );
    }

    #[test]
    fn the_panel_comes_down_when_the_method_stops_being_one_it_shows() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        inbox = inbox.tick_with_input_method(200, Some(a_method("a.abc", "ABC", true)));
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            inbox.into_messages(),
        );
        let messages = written.messages();
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, PluginMessage::HidePanel)),
            "so the chip does not stay in the corner saying a method that is no longer \\\\n             selected: {messages:?}"
        );
    }

    #[test]
    fn a_platform_with_no_input_methods_says_so_once_rather_than_saying_nothing() {
        // "This platform does not report one" and "your method has no name" are different
        // facts, and a plugin that showed nothing for both would be indistinguishable from
        // one that is not running.
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            with_method(100, None).into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels
                .iter()
                .any(|label| label.contains("does not report an input method")),
            "{labels:?}"
        );
    }

    #[test]
    fn a_long_name_is_shortened_to_the_part_a_reader_recognises() {
        let plugin = InputMethodPanel::new(Preferences::default());
        assert_eq!(
            plugin.label(&a_method(
                "com.tencent.inputmethod.wetype.pinyin",
                "微信输入法/微信输入法",
                false
            )),
            "微信输入法",
            "so a name that repeats itself is shown once"
        );
        assert_eq!(
            plugin.label(&a_method("com.apple.keylayout.ABC", "ABC", true)),
            "ABC",
            "and a name that is already short is not made shorter by losing a word"
        );
        assert_eq!(
            plugin.label(&a_method("com.apple.keylayout.ABC", "U.S.", true)),
            "U.S.",
            "because a name with no separator in it is already the short one"
        );
    }

    #[test]
    fn a_label_too_long_for_its_chip_is_cut_with_one_ellipsis() {
        assert_eq!(clip("ABC"), "ABC");
        // Twelve characters is the whole allowance, so twelve is untouched and thirteen is
        // the first thing that is cut: a clip that shortened a name that already fitted
        // would show an ellipsis for no reason at all.
        assert_eq!(clip("ABCDEFGHIJKL"), "ABCDEFGHIJKL");
        let cut = clip("ABCDEFGHIJKLM");
        assert_eq!(cut, "ABCDEFGHIJK…");
        assert_eq!(
            cut.chars().count(),
            MAXIMUM_LABEL_CHARS,
            "and the cut is to a width, not to a guess"
        );
        assert!(
            !clip("微信输入法/微信输入法").contains('\u{2026}')
                || clip("微信输入法/微信输入法").chars().count() <= MAXIMUM_LABEL_CHARS,
            "a cut is one character over the bound however wide the characters are"
        );
    }

    #[test]
    fn switching_methods_moves_the_cat_only_when_the_user_asked_for_it() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        inbox = inbox.tick_with_input_method(200, Some(a_method("a.kana", "かな", false)));
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            inbox.into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "because a plugin that moves the cat the moment it starts is a plugin that moves \\
             the cat while the settings page is open"
        );
    }

    #[test]
    fn a_motion_setting_moves_the_cat_once_per_switch() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        inbox = inbox.tick_with_input_method(200, Some(a_method("a.kana", "かな", false)));
        inbox = inbox.tick_with_input_method(300, Some(a_method("a.kana", "かな", false)));
        serve(
            &mut plugin,
            &written,
            "en-US",
            with(&[
                ("react", ConfigValue::Bool(true)),
                ("react_motion", ConfigValue::Text("CAT_motion.0".to_owned())),
            ]),
            inbox.into_messages(),
        );
        let requests = model_requests(&written);
        assert_eq!(
            requests.len(),
            1,
            "one switch, one motion: the second tick reported the same method and the cat has \\
             already noticed"
        );
        assert!(matches!(
            &requests[0].1,
            ModelRequest::PlayMotion { name, .. } if name == "CAT_motion.0"
        ));
    }

    #[test]
    fn a_blank_motion_name_is_no_motion_rather_than_a_motion_that_does_not_exist() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        inbox = inbox.tick_with_input_method(200, Some(a_method("a.kana", "かな", false)));
        serve(
            &mut plugin,
            &written,
            "en-US",
            with(&[("react", ConfigValue::Bool(true))]),
            inbox.into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "so a user who turns the reaction on and leaves the name blank gets a still cat \\
             rather than a refusal on every switch"
        );
    }

    #[test]
    fn a_tick_that_reports_the_same_method_builds_nothing() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        let mut inbox = with_method(100, Some(a_method("a.pinyin", "拼音", false)));
        for frame in 1..60 {
            inbox =
                inbox.tick_with_input_method(frame * 16, Some(a_method("a.pinyin", "拼音", false)));
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
            "one panel for sixty ticks, because the answer changed once"
        );
    }

    #[test]
    fn nothing_is_drawn_before_the_first_answer_arrives() {
        let written = WrittenMessages::new();
        let mut plugin = InputMethodPanel::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            the_defaults(),
            Inbox::new().into_messages(),
        );
        assert!(
            panels(&written).is_empty(),
            "because the fact arrives on the first tick and a guess before it would be a lie"
        );
    }

    #[test]
    fn the_plugin_asks_for_the_fact_and_nothing_else() {
        let plugin = InputMethodPanel::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "input-method");
        assert_eq!(descriptor.name().resolve("zh-CN"), "输入法");
        assert!(
            descriptor.subscribes_to(Subscription::HostState),
            "for the fact, which arrives with everything else the host knows"
        );
        assert!(
            !descriptor.subscribes_to(Subscription::Input),
            "and not for input: which method is selected has nothing to do with which keys are \\
             down, and asking would cost a batch per keystroke for a fact the system \\
             publishes on its own"
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
                "only_when_not_latin",
                "show_latin",
                "shorten",
                "react",
                "react_motion"
            ]
        );
    }
}
