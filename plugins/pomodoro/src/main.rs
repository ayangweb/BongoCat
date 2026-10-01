//! A focus timer on the model window.
//!
//! Issue #996 asked for "a pomodoro timer or something similar, that can time and
//! remind". This is that, and it is the plugin every other one follows, so it is worth
//! saying what it is made of:
//!
//! * **All of the logic is here.** The timer, the round it is on, what follows a round,
//!   and the arithmetic that turns elapsed milliseconds into `24:31`. Adding a fourth
//!   setting would change nothing outside this crate.
//! * **All of the state is here.** Which round is on screen and when it began, in this
//!   process, in this plugin's own terms. A pomodoro has no state worth keeping between
//!   runs — see [`Pomodoro::on_shutdown`] — and it says so rather than writing a file it
//!   would then have to explain.
//! * **All of the copy is here**, in [`copy`], in the user's own language.
//! * **All of the settings are here.** The declarations in [`declared_settings`] *are*
//!   the settings form: the window renders whatever fields the plugin sends, so a new
//!   setting is a new line in that function and nothing else.
//!
//! What it asks of the host is small and entirely generic: a panel to draw, a clock to
//! read, a press to hear about, and — when a round ends — a motion and a bubble. It
//! cannot ask for anything else, because there is nothing else to ask for.
//!
//! It depends on the SDK and on nothing else. Not even `serde`: the settings are read
//! through the SDK's own typed accessors, so a plugin whose settings are three switches
//! does not pull a serialization library into its dependency graph to get them.

mod copy;
mod timer;

use bongocat_plugin_sdk::prelude::*;
use std::sync::LazyLock;
use timer::{Phase, Round, RoundKind, format_seconds, minutes};

/// This plugin's own manifest, embedded at compile time.
///
/// One document for the identity the card shows and the words the panel draws. See
/// [`bongocat_plugin_sdk::SelfDescription`] for why it is embedded rather than read, and
/// [`copy`] for the words themselves.
static SELF: LazyLock<SelfDescription> = LazyLock::new(|| {
    describe(include_str!("../plugin.json")).expect("this plugin's own manifest is readable")
});

/// How long a round lasts when the user has not chosen.
///
/// Twenty-five minutes, because that is the number the name comes from and because it is
/// a round most people can finish.
const DEFAULT_FOCUS_MINUTES: i64 = 25;

/// The shortest round worth timing.
///
/// One minute. A round is a unit of work, and a unit of work that fits inside a minute
/// is not one — it is a delay someone pressed a button on.
const MINIMUM_ROUND_MINUTES: i64 = 1;

/// The longest round worth timing.
///
/// Two hours, because past that a person is not working in a round any more, and a
/// spinner that can count to a day is a spinner nobody believes.
const MAXIMUM_ROUND_MINUTES: i64 = 120;

/// A short break's length.
const SHORT_BREAK_MINUTES: i64 = 5;

/// A long break's length.
const LONG_BREAK_MINUTES: i64 = 15;

/// How many focus rounds a long break follows.
///
/// Four, which is the shape the name comes from and which a person can hold in their
/// head: four focus rounds, then a longer one.
const ROUNDS_PER_LONG_BREAK: u32 = 4;

/// How long a finished round's notice stays up.
///
/// A countdown the user was watching cannot scroll away before they read why it stopped.
/// Two and a half seconds is one short phrase, and the bubble takes itself down after
/// that whether this plugin remembers to or not.
const NOTICE_MILLIS: u32 = 2_500;

/// The motion asked for when a round ends.
///
/// A name rather than a motion id, because a model chooses its own motions and the only
/// honest thing to ask for is one a model of this sort has. The spelling is the model's own:
/// a group name and an index, `Group.index`, which is the protocol's whole motion
/// vocabulary. The shipped BongoCat models call their first group `CAT_motion`, so this
/// resolves against them; a model without it answers `NotInModel` and the panel and the
/// bubble still say what happened, so the request is a nicety and never a dependency.
const FINISHED_MOTION: &str = "CAT_motion.0";

/// The press id of the one button that starts and stops the round.
const TOGGLE: &str = "toggle";

/// The press id of the button that throws the round away.
const RESET: &str = "reset";

/// Whether a long break is the one that follows, given how many focus rounds are done.
///
/// A count of zero is the start of a session rather than the fourth round, which is why
/// the check is not a remainder: `0 % 4 == 0` is true, and a timer that gave a long break
/// before the first round would be a timer whose first break is the reward for nothing.
fn long_break_follows(completed: u32) -> bool {
    completed >= ROUNDS_PER_LONG_BREAK && completed.is_multiple_of(ROUNDS_PER_LONG_BREAK)
}

/// What a finished round is followed by.
///
/// A choice rather than a number, because the answers are qualitatively different and
/// neither is a special case of the other: a long break is for when you are done and a
/// short one is for when you are not. A "break length in minutes: 0" setting would have
/// to encode that a zero means "no break", and a zero that means "no break" is a value
/// two settings would disagree about.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AfterRound {
    /// Another focus round, immediately. For someone who does not want to stop.
    #[default]
    Focus,
    /// A short pause, then another focus round.
    ShortBreak,
    /// A long pause, then another focus round.
    LongBreak,
}

impl AfterRound {
    /// This choice, from the string the settings form stores.
    ///
    /// A value this build does not recognise is the short break, which is also the
    /// default — so a file written by a newer version of this plugin reads as the pause it
    /// was most likely written as rather than as a broken round.
    fn from_setting(value: &str) -> Self {
        match value {
            "focus" => Self::Focus,
            "long_break" => Self::LongBreak,
            _ => Self::ShortBreak,
        }
    }

    /// The string the settings form stores for this choice.
    ///
    /// The inverse of [`Self::from_setting`], and a plugin has no other use for it: the
    /// form owns the value and this plugin only reads it. It is here because a test that
    /// stands in for the form needs to write one, and a hand-written string in every such
    /// test is a second spelling of the menu waiting to drift.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the settings form owns this value")
    )]
    const fn as_setting(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
        }
    }
}

/// What the user configured.
///
/// Read out of the document the host hands over rather than out of the schema, because
/// the plugin owns both and the only side that can say what a key means is the plugin.
/// The three reads below are the SDK's typed accessors: a value the user has never
/// touched reads as the field's own default, so there is no `unwrap_or` written twice
/// and no way for a new setting to arrive without a reading for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preferences {
    pub focus_minutes: i64,
    pub after_round: AfterRound,
    pub auto_start: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            focus_minutes: DEFAULT_FOCUS_MINUTES,
            after_round: AfterRound::default(),
            auto_start: false,
        }
    }
}

impl Preferences {
    /// The user's settings, as this build understands them.
    fn read(values: &Values) -> Self {
        // Clamped as well as declared. The host fits every document to the schema before
        // it arrives, so this clamp is unreachable through the product; it is here so
        // that a `Values` built any other way still cannot produce a round of zero
        // seconds, which is a timer that finishes before it has been drawn.
        Self {
            focus_minutes: values
                .integer("focus_minutes")
                .clamp(MINIMUM_ROUND_MINUTES, MAXIMUM_ROUND_MINUTES),
            after_round: AfterRound::from_setting(&values.text("after_round")),
            auto_start: values.flag("auto_start"),
        }
    }

    /// The focus round's length in milliseconds.
    fn focus_length_ms(self) -> u64 {
        minutes(self.focus_minutes)
    }
}

/// The settings this plugin declares.
///
/// The declarations are the settings panel: the window renders whatever fields the plugin
/// sends, with the product's own controls, so adding a setting here adds a row in the
/// user's window and changes no code in the application.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Integer::ranged(
                "focus_minutes",
                copy::focus_minutes_label(),
                DEFAULT_FOCUS_MINUTES,
                MINIMUM_ROUND_MINUTES,
                MAXIMUM_ROUND_MINUTES,
            )
            .stepping(1)
            .with_unit(copy::minutes_unit())
            .described(copy::focus_minutes_help())
            .into(),
        )
        .with(
            Choice::new(
                "after_round",
                copy::after_round_label(),
                vec![
                    Option_::new("short_break", copy::then_short_break()),
                    Option_::new("long_break", copy::then_long_break()),
                    Option_::new("focus", copy::then_focus()),
                ],
            )
            .described(copy::after_round_help())
            .into(),
        )
        .with(
            Toggle::new("auto_start", copy::auto_start_label())
                .described(copy::auto_start_help())
                .into(),
        )
}

/// The whole plugin.
///
/// Five fields, and the number is worth a word: a timer is what it is showing, what it
/// will show next, what the user set, and the last thing it drew. Everything else is
/// derived from those, so there is nothing here that can disagree with anything else here.
pub struct Pomodoro {
    /// The panel this plugin draws.
    ///
    /// Held rather than rebuilt per tick, because the comparison against what the host
    /// already has is what makes a second redraw unnecessary.
    panel: Panel,
    /// The round the panel is about.
    round: Round,
    /// What the user configured.
    preferences: Preferences,
    /// The session's elapsed time as of the last tick, which is what the panel draws.
    now_ms: u64,
    /// The last thing the panel showed, so a tick that changes nothing builds nothing.
    painted: Option<Painted>,
}

/// What the panel last showed.
///
/// Two facts and nothing else, because they are the only two that can change without the
/// timer having done anything: the number on screen, and what the button under it says.
/// Held rather than comparing whole scene trees, because a plugin that has to build a
/// tree to find out whether it needs one is a plugin that builds sixty trees a second to
/// decide it does not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Painted {
    seconds: u64,
    button: Button,
}

/// What the one button on the panel does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Button {
    Start,
    Pause,
    Resume,
}

impl Pomodoro {
    /// A timer at rest, with a starting guess at the user's settings.
    ///
    /// A guess rather than the truth, and the handshake replaces it: the document on
    /// disk is the only thing that says what the user chose, and a timer that trusted the
    /// value it was constructed with would show twenty-five minutes to somebody who set
    /// forty-five and then read the file anyway.
    pub fn new(preferences: Preferences) -> Self {
        Self {
            panel: Panel::new(260, 150)
                .anchored(PluginAnchor::BottomLeft)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.62)
                .with_opacity(0.94),
            round: Round::new(RoundKind::Focus, preferences.focus_length_ms(), 0, 0),
            preferences,
            now_ms: 0,
            painted: None,
        }
    }

    /// The round that follows the current one, beginning at `started_ms`.
    ///
    /// `completed` is how many focus rounds have *finished*, not how many rounds have
    /// been shown, because that is the number a long break is really about.
    fn next_round(&self, started_ms: u64, completed: u32) -> Round {
        let (kind, length_ms) = match self.preferences.after_round {
            AfterRound::Focus => (RoundKind::Focus, self.preferences.focus_length_ms()),
            AfterRound::ShortBreak => (RoundKind::ShortBreak, minutes(SHORT_BREAK_MINUTES)),
            AfterRound::LongBreak if long_break_follows(completed) => {
                (RoundKind::LongBreak, minutes(LONG_BREAK_MINUTES))
            }
            // A long break only follows a *fourth* round, which is what the name means.
            // Breaking after every round would leave two lengths that differ only in size,
            // and the setting would be a preference about minutes rather than about when
            // to stop.
            AfterRound::LongBreak => (RoundKind::ShortBreak, minutes(SHORT_BREAK_MINUTES)),
        };
        Round::new(kind, length_ms, started_ms, completed)
    }

    /// The line under the countdown: what this round is for.
    fn round_label(&self, host: &Host) -> String {
        let text = match self.round.phase {
            Phase::Paused => copy::paused(),
            _ => match self.round.kind {
                RoundKind::Focus => copy::focus(),
                RoundKind::ShortBreak => copy::short_break(),
                RoundKind::LongBreak => copy::long_break(),
            },
        };
        copy::say(host, &text)
    }

    /// What the one button would do right now.
    const fn button(&self) -> Button {
        match self.round.phase {
            Phase::Counting => Button::Pause,
            Phase::Paused => Button::Resume,
            Phase::Over => Button::Start,
        }
    }

    fn button_label(&self, host: &Host, button: Button) -> String {
        copy::say(host, &copy::toggle_label(button))
    }

    /// Tell the host what this button would do, so the settings window can draw it.
    ///
    /// The one control a person reaches for often enough that hunting for it inside a
    /// 260-pixel panel on the model window is the wrong way round: a pomodoro is set up
    /// in the settings window and then started from it, and the old arrangement made the
    /// second step a thing you had to go and find on your desktop.
    ///
    /// Re-offered whenever the button changes meaning, which is the whole point of the
    /// control travelling live rather than being declared once: the label is what the
    /// user reads to decide what a press will do, so a card still saying "Start" over a
    /// counting round would say the opposite of the truth. [`Host::offer_action`] skips
    /// the write when nothing changed, so this is safe to call from every tick.
    fn publish_action(&mut self, host: &mut Host) {
        host.offer_action(copy::toggle_action(self.button(), host).to_protocol());
    }

    /// The press on the one button, as one meaning.
    ///
    /// Start and resume are the same press — both make the round run again — and pause is
    /// the only other thing it can mean. One match rather than a test of the phase at
    /// each of the three call sites, so the whole of "what the user did" to "what the
    /// timer does" is one function a reader can see at a glance.
    fn toggle(&mut self) {
        match self.round.phase {
            Phase::Counting => self.round.pause(self.now_ms),
            Phase::Paused => self.round.resume(self.now_ms),
            Phase::Over => {
                let completed = self.finished_rounds();
                self.round = self.next_round(self.now_ms, completed);
            }
        }
    }

    /// How many focus rounds have finished, counting the one on screen if it is over.
    fn finished_rounds(&self) -> u32 {
        if self.round.kind.is_focus() {
            self.round.completed_rounds.saturating_add(1)
        } else {
            self.round.completed_rounds
        }
    }

    /// Notice a round that has reached zero.
    ///
    /// Returns what ended, once, and only the first time. The second half of that is the
    /// interesting one: a tick that arrives after the machine slept for an hour must not
    /// walk through a whole day of rounds firing a notice for each, and the way to
    /// guarantee that is to start the next round *now* rather than at the moment it would
    /// have begun — so this boundary is behind us and the next one is a whole round away.
    fn notice_if_over(&mut self) -> Option<RoundKind> {
        if !self.round.is_over(self.now_ms) {
            return None;
        }
        let finished = self.round.kind;
        let completed = self.finished_rounds();
        self.round.phase = Phase::Over;
        if self.preferences.auto_start {
            self.round = self.next_round(self.now_ms, completed);
        }
        Some(finished)
    }

    /// Rebuild and send the panel, but only if what it would show has changed.
    ///
    /// Returns whether anything was sent, so a caller can tell "the panel is up to date"
    /// from "the panel was just updated" without reading the host's message.
    pub fn draw(&mut self, host: &mut Host) -> bool {
        let seconds = self.round.seconds_left(self.now_ms);
        let toggle = self.button();
        if self.painted
            == Some(Painted {
                seconds,
                button: toggle,
            })
        {
            return false;
        }
        let countdown = format_seconds(seconds);
        let label = self.round_label(host);
        let toggle_label = self.button_label(host, toggle);
        let reset = copy::say(host, &copy::reset());
        // The ring and the number are both derived from the same whole second, so they
        // cannot disagree and neither moves while the other is still.
        let fraction = self.round.fraction_left(self.now_ms);
        let over = self.round.phase == Phase::Over;
        let finished = if over {
            copy::say(host, &copy::round_finished())
        } else {
            String::new()
        };
        self.panel.rebuild(|panel| {
            panel.surface(8.0, [14.0, 12.0], |content| {
                content.row_centered(10.0, |row| {
                    row.push(heading(&countdown, 40.0));
                    row.push(spacer());
                    row.push(ring(fraction, 44.0));
                });
                content.push(bar(fraction, 6.0));
                content.push(muted(&label, 13.0));
                if over {
                    // A finished round is the one moment the panel says something about
                    // time that has passed rather than time that is left, and it says it
                    // here and not only in a bubble: a bubble is gone before a user who
                    // looked away comes back, and this line is still there.
                    content.push(muted(&finished, 12.0));
                }
                content.row_spaced(8.0, |row| {
                    row.push(button(TOGGLE, &toggle_label));
                    row.push(button_secondary(RESET, &reset));
                });
            })
        });
        self.painted = Some(Painted {
            seconds,
            button: toggle,
        });
        host.panel(&mut self.panel)
    }

    /// Read the user's settings.
    fn reload(&mut self, host: &Host) {
        self.preferences = Preferences::read(host.values());
    }
}

impl Plugin for Pomodoro {
    fn descriptor(&self) -> Descriptor {
        // The manifest says who this plugin is, and the descriptor is a projection of it
        // rather than a second place spelling the same six fields out. Adding a plugin
        // that keeps its metadata in its own `plugin.json` is then a change to that one
        // file, and a card that said one thing before the plugin started and another
        // after is not expressible.
        SELF.descriptor().subscribe(Subscription::HostState)
    }

    fn settings(&mut self) -> Settings {
        declared_settings()
    }

    fn on_ready(&mut self, host: &mut Host) -> bongocat_plugin_sdk::Result<()> {
        // The settings are read before the first panel is drawn, so what the user sees on
        // the model window is the round length they chose rather than the default
        // flashing for one frame.
        self.reload(host);
        self.round = Round::new(
            RoundKind::Focus,
            self.preferences.focus_length_ms(),
            self.now_ms,
            0,
        );
        // The control is offered before the first panel, and for the same reason: a
        // button that appears one frame after the timer does is a button the user has to
        // look for, and the settings window cannot draw what it has not been told about.
        self.publish_action(host);
        self.draw(host);
        Ok(())
    }

    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        self.now_ms = tick.elapsed_ms;
        if let Some(kind) = self.notice_if_over() {
            // Two requests, neither of which this plugin has to handle. A model with no
            // motion by that name is answered "not in model", and the panel and the
            // bubble already say what happened — so a model that cannot wave does not
            // stop the countdown from being announced.
            host.play_motion(FINISHED_MOTION);
            let text = if kind.is_focus() {
                copy::round_finished()
            } else {
                copy::break_over()
            };
            host.bubble(&copy::say(host, &text), NOTICE_MILLIS);
        }
        // **After** the round-end check, not before it. `notice_if_over` is what moves a
        // round from counting to over, and over is the state whose button says "Start" —
        // so offering the control before that check would republish the button that was
        // already on the card and leave the card offering "Pause" over a timer that has
        // stopped. The label has to describe the phase this tick produced, and this is
        // the tick that produces it.
        self.publish_action(host);
        self.draw(host);
    }

    fn on_press(&mut self, id: &str, host: &mut Host) {
        if id == TOGGLE {
            self.toggle();
        } else if id == RESET {
            self.round.restart(self.now_ms);
        } else {
            return;
        }
        // Re-offered after every press, because a press is the one thing that changes
        // what the button means — and the press may have come from the settings window's
        // own button, which is drawn from what was last offered. Re-offering here is what
        // makes that button rename itself to what it now does.
        self.publish_action(host);
        self.draw(host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        // A round in progress keeps its length: the setting decides the *next* round, not
        // this one. Shortening the round a person is halfway through is the kind of
        // surprise that loses trust in a timer, and it is the one change here that could
        // take away time somebody has already spent.
        self.reload(host);
        self.draw(host);
    }

    fn on_shutdown(&mut self, _host: &mut Host) {
        // Nothing is written, and that is a decision rather than an omission. A pomodoro
        // is a timer for the stretch of work you are in: which round it is on and when it
        // began both mean something only relative to the session that is running, and the
        // host's monotonic clock is gone the moment the process is. Restoring "24:31
        // remaining" from a wall-clock reading would need the clock, and a timer that
        // guesses how long you were away is a timer that lies. So a restart begins a
        // fresh round, with the choices the user made still in force — which is the part
        // they actually set, and which lives in their own settings file.
    }
}

fn main() -> bongocat_plugin_sdk::Result<()> {
    Pomodoro::new(Preferences::default()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, model_requests, panels,
        values_from,
    };
    use bongocat_plugin_sdk::{ActionGlyph, Host, PluginAction, PluginMessage, Session};

    /// The record of what a session wrote.
    ///
    /// One per test rather than one per session: a test that served a plugin two
    /// sessions at once would be asserting on a conversation that cannot happen.
    fn harness() -> WrittenMessages {
        WrittenMessages::new()
    }

    /// Run a whole session over this document: announce, then serve these messages.
    ///
    /// The document goes in rather than being left at the schema's defaults, because that
    /// is the only way a value reaches a plugin in the product too: the host reads the
    /// plugin's own file and sends the whole thing at the handshake. A test that set a
    /// round length by constructing the plugin would be testing a path nothing takes.
    fn serve(
        plugin: &mut Pomodoro,
        written: &WrittenMessages,
        locale: &str,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        let schema = declared_settings().to_schema().expect("a valid schema");
        let host = Host::new(
            written.writer(),
            IdentityBuilder::new().id("pomodoro").locale(locale).build(),
            schema.clone(),
            values_from(&config, &schema),
        )
        .expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("the plugin announces itself and draws");
        session.serve(plugin, messages).expect("served");
    }

    /// Serve a session with the settings a user who changed nothing would have.
    fn serve_default(plugin: &mut Pomodoro, written: &WrittenMessages, messages: Vec<HostMessage>) {
        serve(
            plugin,
            written,
            "en-US",
            configured(DEFAULT_FOCUS_MINUTES, AfterRound::default(), false),
            messages,
        );
    }

    /// The document the settings form would send for these settings.
    fn configured(focus_minutes: i64, after_round: AfterRound, auto_start: bool) -> ConfigDocument {
        document(
            [
                (
                    "focus_minutes".to_string(),
                    ConfigValue::Integer(focus_minutes),
                ),
                (
                    "after_round".to_string(),
                    ConfigValue::Text(after_round.as_setting().to_string()),
                ),
                ("auto_start".to_string(), ConfigValue::Bool(auto_start)),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// A one-minute round, which is the shortest this plugin will run.
    fn a_minute() -> ConfigDocument {
        configured(MINIMUM_ROUND_MINUTES, AfterRound::ShortBreak, false)
    }

    /// A one-minute round that rolls on by itself.
    fn a_minute_on_auto() -> ConfigDocument {
        configured(MINIMUM_ROUND_MINUTES, AfterRound::ShortBreak, true)
    }

    #[test]
    fn a_ready_draws_its_first_panel_without_waiting_for_a_tick() {
        // A panel that appears only on the first tick is a panel that is missing when the
        // model window opens and the user is looking for it.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            configured(45, AfterRound::default(), false),
            Inbox::new().into_messages(),
        );
        let drawn = panels(&written);
        assert_eq!(
            drawn.len(),
            1,
            "one panel, from `on_ready` and with no tick"
        );
        let labels = labels_in(&drawn[0].scene);
        assert!(
            labels.contains(&"45:00".to_string()),
            "the round the user chose is on screen from the first frame rather than the default \
             flashing for one: {labels:?}"
        );
        assert!(
            labels.contains(&"Focus".to_string()),
            "and the panel says what the round is for"
        );
        drawn[0]
            .validate()
            .expect("a panel this plugin builds is one the host accepts");
    }

    #[test]
    fn the_panel_changes_once_a_second_rather_than_once_a_frame() {
        // The host rasterizes a panel whenever its contents differ, so a panel that moved
        // on every tick would mean a texture upload per frame to show a number that changes
        // once a second.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        let mut inbox = Inbox::new();
        for frame in 0..120 {
            inbox = inbox.tick(frame * 8);
        }
        serve_default(&mut plugin, &written, inbox.into_messages());
        assert_eq!(
            panels(&written).len(),
            1,
            "a second of ticks at 125 Hz, and the seconds on screen never changed"
        );
    }

    #[test]
    fn a_round_that_ends_asks_the_model_to_react_and_says_why() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            a_minute(),
            Inbox::new().tick(minutes(1)).into_messages(),
        );
        let requests = model_requests(&written);
        assert_eq!(
            requests.len(),
            2,
            "a motion and a bubble, and both are things the host may refuse"
        );
        assert!(matches!(requests[0].1, ModelRequest::PlayMotion { .. }));
        assert!(matches!(requests[1].1, ModelRequest::ShowBubble { .. }));
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.contains(&"00:00".to_string()) && labels.contains(&"Round finished".to_string()),
            "and the panel says it too, because a bubble is gone before a user who looked away \
             comes back: {labels:?}"
        );
    }

    #[test]
    fn a_round_that_ends_waits_for_the_user_unless_they_asked_for_it_to_continue() {
        // Automatic start off: the timer stops at zero and the button becomes "Start".
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            a_minute(),
            Inbox::new().tick(minutes(1)).into_messages(),
        );
        assert_eq!(
            plugin.button(),
            Button::Start,
            "a finished round with nothing queued is a round the user has to acknowledge"
        );

        // On: the next round is already running, and the panel says so.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            a_minute_on_auto(),
            Inbox::new().tick(minutes(1)).into_messages(),
        );
        assert_eq!(plugin.round.phase, Phase::Counting);
        assert_eq!(plugin.round.kind, RoundKind::ShortBreak);
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.contains(&"05:00".to_string()),
            "so the panel moved straight on to the pause: {labels:?}"
        );
    }

    #[test]
    fn a_tick_that_arrives_after_the_machine_slept_announces_one_boundary_not_a_days_worth() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            a_minute_on_auto(),
            // Six one-minute rounds went by without a tick.
            Inbox::new().tick(minutes(6)).into_messages(),
        );
        assert_eq!(
            model_requests(&written).len(),
            2,
            "one motion and one bubble, because the next round starts now rather than at the \
             moment it would have — so the boundary behind us cannot fire again"
        );
        assert_eq!(plugin.round.kind, RoundKind::ShortBreak);
        assert_eq!(
            plugin.round.remaining_ms(minutes(6)),
            minutes(5),
            "and the pause it moved to is a whole one, not whatever was left of the day"
        );
    }

    #[test]
    fn the_button_stops_the_round_and_starts_it_again_without_losing_the_time_served() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(
            &mut plugin,
            &written,
            Inbox::new()
                .tick(minutes(10))
                .press(TOGGLE)
                .tick(minutes(20))
                .press(TOGGLE)
                .into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert_eq!(
            plugin.button(),
            Button::Pause,
            "a resumed round is a running one"
        );
        assert_eq!(
            plugin.round.remaining_ms(minutes(20)),
            minutes(15),
            "a timer that forgot how long you had been working when you stopped for a coffee is \
             measuring the wrong thing"
        );
        assert!(
            !labels.contains(&"Paused".to_string()),
            "and the paused state is gone from the panel: {labels:?}"
        );
    }

    #[test]
    fn a_paused_round_does_not_lose_a_second_to_the_time_it_spent_stopped() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(
            &mut plugin,
            &written,
            Inbox::new()
                .tick(minutes(10))
                .press(TOGGLE)
                .tick(minutes(400))
                .into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert_eq!(
            plugin.button(),
            Button::Resume,
            "an hour of not pressing the button did not stop the round from being paused"
        );
        assert!(
            labels.contains(&"15:00".to_string()),
            "and the countdown kept the time already served, rather than serving the hour too: \
             {labels:?}"
        );
    }

    #[test]
    fn a_press_on_something_this_panel_does_not_have_changes_nothing() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(
            &mut plugin,
            &written,
            Inbox::new()
                .tick(minutes(10))
                .press("somebody-elses-button")
                .into_messages(),
        );
        assert_eq!(
            plugin.button(),
            Button::Pause,
            "the host only ever sends a press the panel declared, and a plugin that guessed at \
             others would act on one it never drew"
        );
    }

    #[test]
    fn the_panel_answers_in_the_language_the_user_reads() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "zh-CN",
            configured(DEFAULT_FOCUS_MINUTES, AfterRound::default(), false),
            Inbox::new().into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.contains(&"专注".to_string()) && labels.contains(&"暂停".to_string()),
            "the plugin's own copy, in the user's own language, with the application knowing \
             none of these words: {labels:?}"
        );
    }

    #[test]
    fn a_language_the_plugin_has_no_copy_for_reads_as_its_default_rather_than_as_a_key() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "ja-JP",
            configured(DEFAULT_FOCUS_MINUTES, AfterRound::default(), false),
            Inbox::new().into_messages(),
        );
        let labels = labels_in(&panels(&written).last().expect("a panel").scene);
        assert!(
            labels.contains(&"Focus".to_string()),
            "a panel full of keys is a panel nobody can use: {labels:?}"
        );
    }

    #[test]
    fn a_setting_the_user_changed_takes_effect_from_the_next_round() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(
            &mut plugin,
            &written,
            Inbox::new()
                .tick(minutes(10))
                .config(configured(5, AfterRound::Focus, true))
                .into_messages(),
        );
        assert_eq!(
            plugin.round.remaining_ms(minutes(10)),
            minutes(15),
            "the round in progress keeps the length the user started it with, because shortening \
             a round somebody is halfway through is the kind of surprise that loses trust in a \
             timer"
        );
        assert_eq!(
            plugin.next_round(0, 0).length_ms,
            minutes(5),
            "and the setting is not ignored: the next round is the new length"
        );
        assert!(plugin.preferences.auto_start);
    }

    #[test]
    fn the_value_the_form_sends_is_the_value_the_plugin_reads() {
        // The one thing that could otherwise drift: what the settings form writes, and what
        // this plugin answers to.
        let schema = declared_settings().to_schema().expect("a valid schema");
        for choice in [
            AfterRound::Focus,
            AfterRound::ShortBreak,
            AfterRound::LongBreak,
        ] {
            assert_eq!(AfterRound::from_setting(choice.as_setting()), choice);
            let values = values_from(&configured(30, choice, true), &schema);
            let preferences = Preferences::read(&values);
            assert_eq!(preferences.focus_minutes, 30);
            assert_eq!(preferences.after_round, choice);
            assert!(preferences.auto_start);
        }
    }

    #[test]
    fn a_value_this_build_does_not_know_reads_as_the_pause_it_was_written_as() {
        let schema = declared_settings().to_schema().expect("a valid schema");
        let values = values_from(
            &document(
                [(
                    "after_round".to_string(),
                    ConfigValue::Text("something_newer".to_string()),
                )]
                .into_iter()
                .collect(),
            ),
            &schema,
        );
        assert_eq!(
            Preferences::read(&values).after_round,
            AfterRound::ShortBreak,
            "a value a newer version wrote is not this build's to interpret, and the short break \
             is the reading it was most likely written as"
        );
    }

    #[test]
    fn a_document_with_a_value_of_the_wrong_kind_reads_as_the_default() {
        let schema = declared_settings().to_schema().expect("a valid schema");
        let values = values_from(
            &document(
                [
                    ("focus_minutes".to_string(), ConfigValue::Bool(true)),
                    ("auto_start".to_string(), ConfigValue::Integer(1)),
                ]
                .into_iter()
                .collect(),
            ),
            &schema,
        );
        let preferences = Preferences::read(&values);
        assert_eq!(
            preferences.focus_minutes, DEFAULT_FOCUS_MINUTES,
            "a switch where a number was declared is a document the host would not send; reading \
             the default is the honest answer"
        );
        assert!(!preferences.auto_start);
    }

    #[test]
    fn a_hand_written_file_cannot_ask_for_a_round_that_finishes_before_it_is_drawn() {
        let schema = declared_settings().to_schema().expect("a valid schema");
        for written in [i64::MIN, 0, -10, 100_000, i64::MAX] {
            let values = values_from(
                &document(
                    [("focus_minutes".to_string(), ConfigValue::Integer(written))]
                        .into_iter()
                        .collect(),
                ),
                &schema,
            );
            let preferences = Preferences::read(&values);
            assert!(
                (MINIMUM_ROUND_MINUTES..=MAXIMUM_ROUND_MINUTES)
                    .contains(&preferences.focus_minutes),
                "{written} became {}, which is inside the range this plugin declares",
                preferences.focus_minutes
            );
        }
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
            ["focus_minutes", "after_round", "auto_start"],
            "in the order the form shows them, because the headline is first"
        );
        assert_eq!(
            declared_settings().len(),
            3,
            "and the count is the count, so a field added here is a row a user will see"
        );
    }

    #[test]
    fn a_finished_focus_round_is_followed_by_the_pause_the_user_asked_for() {
        let plugin = Pomodoro::new(Preferences {
            after_round: AfterRound::ShortBreak,
            ..Preferences::default()
        });
        let next = plugin.next_round(1_000, 1);
        assert_eq!(next.kind, RoundKind::ShortBreak);
        assert_eq!(next.length_ms, minutes(SHORT_BREAK_MINUTES));
    }

    #[test]
    fn a_long_break_follows_the_fourth_round_because_that_is_what_the_name_means() {
        let plugin = Pomodoro::new(Preferences {
            after_round: AfterRound::LongBreak,
            ..Preferences::default()
        });
        assert_eq!(
            plugin.next_round(0, 0).kind,
            RoundKind::ShortBreak,
            "a session that has finished nothing has earned no long break, and `0 % 4 == 0` is \
             true, which is exactly why this cannot be a remainder"
        );
        for completed in 1..ROUNDS_PER_LONG_BREAK {
            assert_eq!(
                plugin.next_round(0, completed).kind,
                RoundKind::ShortBreak,
                "after round {completed} the break is short"
            );
        }
        assert_eq!(
            plugin.next_round(0, ROUNDS_PER_LONG_BREAK).kind,
            RoundKind::LongBreak,
            "and after the fourth it is long, which is the whole difference between two lengths \
             and a choice about when to stop"
        );
        assert_eq!(
            plugin.next_round(0, ROUNDS_PER_LONG_BREAK * 2).kind,
            RoundKind::LongBreak,
            "and again after the eighth"
        );
    }

    #[test]
    fn asking_for_another_round_straight_away_is_a_choice_and_not_a_zero_length_break() {
        let plugin = Pomodoro::new(Preferences {
            after_round: AfterRound::Focus,
            ..Preferences::default()
        });
        let next = plugin.next_round(0, 1);
        assert_eq!(next.kind, RoundKind::Focus);
        assert_eq!(
            next.length_ms,
            minutes(DEFAULT_FOCUS_MINUTES),
            "a zero that means 'no break' is a value two settings would disagree about"
        );
    }

    /// Every label this plugin offered, in the order it offered them.
    ///
    /// Resolved against the default language rather than a host, because the label is
    /// what these tests are about and the language they run in is the default one. The
    /// test that is about *language* reads the same field through
    /// [`LocalizedText::resolve`] for the tag it cares about.
    fn offered_labels(written: &WrittenMessages) -> Vec<String> {
        offered_controls(written)
            .into_iter()
            .map(|action| action.label.resolve("en-US").to_string())
            .collect()
    }

    /// Every control this plugin offered, across a whole session, newest last.
    fn offered_controls(written: &WrittenMessages) -> Vec<PluginAction> {
        written
            .messages()
            .into_iter()
            .filter_map(|message| match message {
                PluginMessage::Actions { actions } => Some(actions),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn the_settings_window_gets_the_button_the_round_is_actually_in() {
        // The reason this plugin offers a control at all: a pomodoro is configured in
        // the settings window, so that is where it has to be controllable from. Before
        // this, the only way to start or stop one was to find a small button inside a
        // panel on the model window.
        //
        // "Pause" rather than "Start", and that is the honest answer rather than a
        // surprise: a round begins counting the moment the plugin starts, so the first
        // thing that button can do is stop it. `Round::new` is in `Counting`, and a
        // control that said "Start" here would be describing a round that is already
        // running.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(&mut plugin, &written, Inbox::new().into_messages());
        assert_eq!(plugin.round.phase, Phase::Counting);
        let offered = offered_controls(&written);
        assert_eq!(offered.len(), 1, "one control, offered once");
        assert_eq!(offered[0].id, TOGGLE);
        assert_eq!(offered_labels(&written), ["Pause"]);
        assert_eq!(
            offered[0].glyph,
            ActionGlyph::Pause,
            "and the icon agrees with the label"
        );
    }

    #[test]
    fn that_button_says_what_pressing_it_now_does() {
        // The whole reason an action is a message rather than a declaration made once. A
        // card still reading "Pause" over a stopped round says the opposite of what the
        // press will do, and a user who trusts it is looking at a timer that is not
        // running.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(
            &mut plugin,
            &written,
            Inbox::new().tick(minutes(1)).press(TOGGLE).into_messages(),
        );
        let offered = offered_controls(&written);
        assert_eq!(
            offered.len(),
            2,
            "the button's meaning changed once — counting, then paused"
        );
        assert_eq!(
            offered_labels(&written),
            ["Pause", "Resume"],
            "so what a press will do is what the label says at the moment it is read"
        );
        assert_eq!(
            offered[1].glyph,
            ActionGlyph::Play,
            "and Resume gets the play glyph, not the pause one it is named after — the press sets \
             a stopped round running again, it does not suspend anything"
        );
    }

    #[test]
    fn a_press_from_the_settings_window_stops_the_round_exactly_as_one_on_the_panel_does() {
        // One press vocabulary for both: an action's id is a panel button's id, so the
        // plugin's handler is the handler for both and neither can drift from the other.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve_default(
            &mut plugin,
            &written,
            Inbox::new().tick(minutes(5)).press(TOGGLE).into_messages(),
        );
        assert_eq!(plugin.round.phase, Phase::Paused);
        assert_eq!(
            plugin.round.remaining_ms(minutes(5)),
            minutes(20),
            "so the round stopped with the five minutes already served deducted — the same \
             arithmetic a press on the panel's own button goes through"
        );
    }

    #[test]
    fn a_round_that_ends_on_its_own_offers_start_again_rather_than_pause() {
        // Auto start off: the timer stops at zero, and the card's button has to say so in
        // the tick that did it. This is the tick ordering — the control is re-offered
        // before the round-end check — so the label cannot lag the panel by a frame.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "en-US",
            a_minute(),
            Inbox::new().tick(minutes(1)).into_messages(),
        );
        assert_eq!(
            offered_labels(&written)
                .last()
                .expect("a control was offered"),
            "Start",
            "so the button offers to start the next round rather than to pause one that has \
             already stopped"
        );
    }

    #[test]
    fn the_control_answers_in_the_language_the_user_reads() {
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            "zh-CN",
            configured(DEFAULT_FOCUS_MINUTES, AfterRound::default(), false),
            Inbox::new().into_messages(),
        );
        assert_eq!(
            offered_controls(&written)[0].label.resolve("zh-CN"),
            "暂停",
            "the plugin's own copy, in the user's language, with the application knowing none \
             of these words"
        );
    }

    #[test]
    fn an_unchanged_control_is_not_re_offered_every_tick() {
        // Otherwise a timer would write a protocol line sixty times a second to say the
        // same word, and those lines are what the host's reader thread wakes up for.
        let written = harness();
        let mut plugin = Pomodoro::new(Preferences::default());
        let mut inbox = Inbox::new();
        for frame in 0..120 {
            inbox = inbox.tick(frame * 8);
        }
        serve_default(&mut plugin, &written, inbox.into_messages());
        assert_eq!(
            offered_controls(&written).len(),
            1,
            "a second of ticks at 125 Hz, and the button's meaning never changed"
        );
    }

    #[test]
    fn the_descriptor_is_valid_before_the_plugin_talks_to_anybody() {
        // A plugin that cannot name itself is one the host will not start, and the failure
        // arrives as a refusal with no detail. This is the check that says which part is
        // wrong instead.
        let plugin = Pomodoro::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "pomodoro");
        assert_eq!(
            descriptor.name().resolve("zh-CN"),
            "番茄钟",
            "the card's name is the plugin's own copy, in the user's own language"
        );
        assert_eq!(
            descriptor.name().resolve("en-US"),
            "Pomodoro",
            "and the name the archive carries is still an ordinary string for the store to check"
        );
        descriptor
            .check()
            .expect("a pomodoro's own descriptor is one the host accepts");
    }

    #[test]
    fn the_plugin_does_not_subscribe_to_input_because_a_timer_has_no_use_for_a_keystroke() {
        let plugin = Pomodoro::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert!(
            !descriptor.subscribes_to(Subscription::Input),
            "a timer that counted input would be a different plugin"
        );
        assert!(
            descriptor.subscribes_to(Subscription::HostState),
            "and it does want to know whether the window is there to draw on"
        );
    }
}
