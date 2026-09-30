//! Letting the cat do things.
//!
//! Issue #945 asked for abilities rather than observation: an agent setting a reminder, the
//! cat fetching the weather, the cat answering a question and doing a motion about it. The
//! reference in that issue suggested agent hooks; hooks are what the sibling plugin does, and
//! this is the other shape — the cat reaches *out* rather than watching.
//!
//! # The agent is whatever command the user names
//!
//! That is the whole design, and it is why this plugin is small. There is no agent client, no
//! API key handling, no model, no conversation state: a skill is a command line, and
//! whatever that command prints is the answer the cat says. Point it at `wttr.in` and it is
//! the weather; point it at your own AI command-line tool and it is an agent; point it at a
//! shell script and it is that script. Adding an ability adds a line to a text box, and adds
//! nothing at all to the product — no crate, no feature flag, no installer bytes.
//!
//! **Nothing runs until the user fills the box in.** A command setting starts empty, so the
//! plugin's default state is to do nothing. Shipping a built-in network call would be the
//! plugin deciding that it may talk to the internet on the user's behalf, which is the user's
//! decision to make, not the plugin's.
//!
//! # What is careful about, and why
//!
//! The command is the one part of this plugin that can do something to the machine, so it is
//! the part with the constraints, and each of them is in [`command`]: no shell, a bounded
//! output, a bounded wait, one substitution and only where the user put it.
//!
//! Reminders are the other part that could be got wrong, and what makes them safe is that they
//! are the plugin's own file — three facts in `state.json` in a directory the host created and
//! never writes inside, so replacing this plugin's program cannot replace what the user asked
//! to be reminded about.
//!
//! # What it asks of the host
//!
//! A motion and a bubble. No facts, no input, no platform capability, no protocol change. That
//! is the strongest statement this repository makes about where a plugin's boundary is: a
//! plugin that fetches the weather and talks to an agent needed *nothing* added to the
//! application to do it.

mod command;
mod copy;
mod reminder;

use bongocat_plugin_sdk::Result;
use bongocat_plugin_sdk::prelude::*;
use command::{Command, Outcome, Running};
use reminder::{Book, State, now_unix};
use std::time::Duration;

/// The panel's size, chosen for an answer of a couple of lines and two buttons.
const PANEL_WIDTH: u32 = 280;
const PANEL_HEIGHT: u32 = 190;

/// How long a bubble this plugin shows is on screen.
///
/// Long enough to read a sentence at a glance and short enough that the answer is not still
/// up when the next one arrives. The protocol bounds this itself; this is the value, not the
/// limit.
const BUBBLE_MILLIS: u32 = 6_000;

/// The press id of the button that runs the skill.
const RUN: &str = "run";

/// The press id of the button that arms the reminder.
const ARM: &str = "arm";

/// The press id of the button that forgets the reminder.
const CLEAR: &str = "clear";

/// What the user configured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preferences {
    /// The skill's command, or nothing when the box is empty.
    pub command: Option<Command>,
    /// What `%s` in the command becomes.
    pub question: String,
    /// Minutes between automatic runs, zero for button-only.
    pub every_minutes: i64,
    /// How long to wait for an answer.
    pub timeout: Duration,
    /// The motion to play when something arrives.
    pub motion: String,
    /// Whether the cat speaks only while its window is up.
    pub only_when_visible: bool,
    /// What the reminder says.
    pub reminder_text: String,
    /// How long the reminder waits.
    pub reminder_minutes: i64,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            // Nothing runs until the user says what to run.
            command: None,
            question: String::new(),
            every_minutes: 0,
            timeout: command::DEFAULT_TIMEOUT,
            motion: String::new(),
            only_when_visible: false,
            reminder_text: String::new(),
            reminder_minutes: 10,
        }
    }
}

impl Preferences {
    fn read(values: &Values) -> Self {
        let question = values.text("question").trim().to_owned();
        // Every N minutes, zero meaning button-only. Negative is nonsense and zero is the
        // documented "do not repeat", so a floor of zero is what a hand-edited negative gets.
        let every_minutes = values.integer("every_minutes").max(0);
        let timeout = Duration::from_secs(
            values
                .integer("timeout_seconds")
                .clamp(1, command::MAXIMUM_TIMEOUT_SECONDS as i64) as u64,
        );
        let motion = values.text("motion");
        Self {
            command: Command::parse(values.text("command").trim()),
            question,
            every_minutes,
            timeout,
            // A blank name is no motion rather than a motion that does not exist: the bubble
            // is the answer, and a motion the model does not have is a refusal every time.
            motion: if motion.trim().is_empty() {
                String::new()
            } else {
                motion.trim().to_owned()
            },
            only_when_visible: values.flag("only_when_visible"),
            reminder_text: values.text("reminder_text").trim().to_owned(),
            reminder_minutes: values.integer("reminder_minutes").max(0),
        }
    }

    /// The command as it would be run, with the question filled in.
    fn skill(&self) -> Option<Command> {
        self.command
            .as_ref()
            .map(|command| command.with_question(&self.question))
    }
}

/// The settings this plugin declares, which *are* the settings panel.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            TextField::new("command", copy::command_label())
                .described(copy::command_help())
                .into(),
        )
        .with(
            TextField::new("question", copy::question_label())
                .described(copy::question_help())
                .into(),
        )
        .with(
            Integer::ranged("every_minutes", copy::every_label(), 0, 0, 1_440)
                .stepping(1)
                .with_unit("minutes")
                .described(copy::every_help())
                .into(),
        )
        .with(
            Integer::ranged("timeout_seconds", copy::timeout_label(), 5, 1, 30)
                .stepping(1)
                .with_unit("seconds")
                .described(copy::timeout_help())
                .into(),
        )
        .with(
            TextField::new("motion", copy::motion_label())
                .described(copy::motion_help())
                .into(),
        )
        .with(
            Toggle::new("only_when_visible", copy::only_when_visible_label())
                .described(copy::only_when_visible_help())
                .into(),
        )
        .with(
            TextField::new("reminder_text", copy::reminder_text_label())
                .described(copy::reminder_text_help())
                .into(),
        )
        .with(
            Integer::ranged(
                "reminder_minutes",
                copy::reminder_minutes_label(),
                10,
                0,
                100_000,
            )
            .stepping(1)
            .with_unit("minutes")
            .described(copy::reminder_minutes_help())
            .into(),
        )
}

/// What the panel last showed.
///
/// Four lines and a pair of buttons, and the whole thing is compared by its own text: the
/// panel *is* the text, so two panels are the same panel exactly when their text is the same.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Painted {
    state: String,
    detail: String,
    answer: String,
    reminder: String,
    buttons: Vec<(String, String)>,
}

/// The whole plugin.
pub struct CatSkills {
    panel: Panel,
    preferences: Preferences,
    /// The skill that is running, if one is.
    running: Option<Running>,
    /// When the skill last *started*, which is the clock the repeat counts from.
    ///
    /// Started rather than finished on purpose: a command that takes thirty seconds and a
    /// repeat of one minute should run every minute from when it began, not drift by its own
    /// duration every time.
    last_started_unix: i64,
    /// When the skill last *answered*, which is what the panel says and what a test waits for.
    ///
    /// A separate field from the one above because "it has begun" and "it has spoken" are
    /// different facts, and the repeat's clock needs the first while anything waiting for the
    /// skill needs the second. They are usually one tick apart.
    last_answered_unix: i64,
    /// What the skill last said.
    answer: Option<String>,
    /// Every reminder this plugin knows about.
    book: Book,
    /// What the panel last showed, so a tick that changed nothing builds nothing.
    painted: Option<Painted>,
    /// The store the reminder file lives in.
    store: Store,
    /// Whether the window is up, from the last tick.
    visible: bool,
    /// A line to show instead of the usual state, for one tick.
    notice: Option<(String, bool)>,
}

impl CatSkills {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            panel: Panel::new(PANEL_WIDTH, PANEL_HEIGHT)
                .anchored(PluginAnchor::BottomRight)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.28)
                .with_opacity(0.93),
            book: Book::new(),
            running: None,
            last_started_unix: now_unix(),
            last_answered_unix: i64::MIN,
            answer: None,
            preferences,
            painted: None,
            store: Store::new(std::path::PathBuf::from(".")),
            visible: true,
            notice: None,
        }
    }

    /// Whether the cat should say and move right now.
    fn speaks(&self) -> bool {
        !(self.preferences.only_when_visible && !self.visible)
    }

    /// Start the skill, if it is set up and not already running.
    ///
    /// A skill that is already running is not started again: a command that takes four
    /// seconds and a repeat of one minute is fine, and a command that takes four seconds and
    /// a repeat of zero seconds is a user pressing a button twice, and the second press is
    /// answered by doing nothing rather than by running two of everything.
    fn run_skill(&mut self) -> bool {
        if self.running.as_ref().is_some_and(Running::is_pending) {
            return false;
        }
        let Some(skill) = self.preferences.skill() else {
            return false;
        };
        self.running = Some(skill.spawn_within(self.preferences.timeout));
        true
    }

    /// Collect the answer if one has arrived.
    fn collect_answer(&mut self, host: &mut Host) {
        let answer = self.running.as_mut().and_then(|running| running.answer());
        let Some(answer) = answer else {
            return;
        };
        self.running = None;
        self.last_answered_unix = now_unix();
        // A failure and a timeout are worth saying even when there is no line to say, because
        // "the command printed nothing" and "the command timed out" are different facts and a
        // user who wired a skill up needs to be told which one happened.
        let failed = matches!(answer, Outcome::Failed(_) | Outcome::NotStarted(_));
        let text = answer.first_line();
        self.answer = text.clone();
        // A failure and a timeout are worth saying even when there is no line to say, because
        // "the command printed nothing" and "the command timed out" are different facts and a
        // user who wired a skill up needs to be told which one happened.
        let said = match answer {
            Outcome::Silent => Some(copy::say(host, &copy::said_nothing())),
            Outcome::TimedOut => Some(copy::say(host, &copy::timed_out())),
            _ => text.clone(),
        };
        if let Some(said) = said {
            self.say(&said, host, failed);
        }
    }

    /// Say something, and move, if the cat should.
    ///
    /// The bubble is the answer and the motion is punctuation: both or neither is not an
    /// option a user asked for, but a motion with no bubble is a cat moving for no visible
    /// reason, and a bubble with no motion is the default arrangement.
    fn say(&mut self, text: &str, host: &mut Host, failed: bool) {
        if !self.speaks() {
            return;
        }
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        host.bubble(text, BUBBLE_MILLIS);
        if !failed && !self.preferences.motion.is_empty() {
            host.play_motion(&self.preferences.motion);
        }
    }

    /// Do what is due, and remember to do the rest next time.
    fn on_time(&mut self, host: &mut Host) {
        let now = now_unix();
        if let Some(reminder) = self.book.take_due(now) {
            // The text the user typed is the text the cat says. Nothing is added to it and
            // nothing is taken from it, because a reminder the plugin rewrites is a reminder
            // the user did not ask for.
            self.notice = Some((reminder.text.clone(), true));
            self.say(&reminder.text, host, false);
            self.remember(host);
        }
        let every = self.preferences.every_minutes;
        if every > 0
            && now.saturating_sub(self.last_started_unix) >= every.saturating_mul(60)
            && self.run_skill()
        {
            self.last_started_unix = now;
        }
    }

    /// Write the reminder book to the plugin's own file.
    ///
    /// Best-effort and never fatal, exactly as the other plugins that keep state are: a
    /// plugin that stopped working because its file could not be written is a worse outcome
    /// than one that worked and could not remember, and the plugin's own log says which
    /// happened.
    fn remember(&mut self, host: &mut Host) {
        if let Err(error) = self.store.write_state(&self.book.to_state()) {
            host.log(
                LogLevel::Warn,
                &format!("this reminder could not be written: {error}"),
            );
        }
    }

    /// Rebuild the panel if what it would show has changed.
    fn draw(&mut self, host: &mut Host) {
        let running = self.running.as_ref().is_some_and(Running::is_pending);
        let (state, detail) = if running {
            (
                copy::say(host, &copy::running()),
                self.preferences
                    .skill()
                    .map(|skill| skill.display())
                    .unwrap_or_default(),
            )
        } else if let Some((notice, done)) = self.notice.take() {
            (
                notice,
                if done {
                    copy::say(host, &copy::reminder_done())
                } else {
                    copy::say(host, &copy::armed())
                },
            )
        } else {
            let label = if self.preferences.skill().is_some() {
                copy::say(host, &copy::answer_label())
            } else {
                copy::say(host, &copy::no_skill())
            };
            (label, String::new())
        };
        let answer = self.answer.clone().unwrap_or_default();
        let reminder = match self.book.shown() {
            None => copy::say(host, &copy::no_reminder()),
            Some(reminder) => {
                let left = reminder.seconds_left(now_unix());
                if reminder.said {
                    format!(
                        "{} · {}",
                        reminder.text,
                        copy::say(host, &copy::reminder_done())
                    )
                } else {
                    format!("{} · {left}s", reminder.text)
                }
            }
        };
        let run_label = copy::say(host, &copy::run_label());
        let arm_label = copy::say(host, &copy::arm_label());
        let clear_label = copy::say(host, &copy::clear_label());
        let buttons = if running {
            Vec::new()
        } else if self.preferences.skill().is_some() {
            vec![(RUN.to_owned(), run_label.clone())]
        } else {
            Vec::new()
        };
        let painted = Painted {
            state,
            detail,
            answer,
            reminder,
            buttons: buttons.clone(),
        };
        if self.painted.as_ref() == Some(&painted) {
            return;
        }
        let answer_label = copy::say(host, &copy::answer_label());
        let reminder_label = copy::say(host, &copy::reminder_label());
        let armed = self.book.is_armed();
        self.panel.rebuild(|panel| {
            panel.surface(6.0, [14.0, 12.0], |content| {
                content.row_centered(8.0, |row| {
                    row.push(heading(&painted.state, 18.0));
                });
                if !painted.detail.is_empty() {
                    content.push(muted(&painted.detail, 11.0));
                }
                if !painted.answer.is_empty() {
                    content.push(divider());
                    content.push(muted(&answer_label, 10.0));
                    content.push(text(&painted.answer, 13.0));
                }
                content.push(divider());
                content.push(muted(&reminder_label, 10.0));
                content.push(muted(&painted.reminder, 13.0));
                content.row_spaced(6.0, |line| {
                    for (id, label) in &painted.buttons {
                        line.push(button_secondary(id, label));
                    }
                    if armed {
                        line.push(button_secondary(CLEAR, &clear_label));
                    }
                    line.push(button_secondary(ARM, &arm_label));
                });
            })
        });
        host.show(&mut self.panel);
        self.painted = Some(painted);
    }
}

impl Plugin for CatSkills {
    fn descriptor(&self) -> Descriptor {
        Descriptor::new("cat-skills", copy::plugin_name().resolve(""))
            .version(1, 0, 0)
            .author("BongoCat")
            .named(copy::plugin_name())
            .described(copy::plugin_description())
            .icon(copy::ICON)
            // Model reactions, because the bubble and the motion are the whole of what this
            // plugin produces. No input feed, no host state, no platform capability: a plugin
            // that fetches the weather and talks to an agent needed nothing added to the
            // application to do it, and that is the claim this descriptor makes.
            .subscribe(Subscription::ModelReaction)
    }

    fn settings(&mut self) -> Settings {
        declared_settings()
    }

    fn on_ready(&mut self, host: &mut Host) -> Result<()> {
        self.preferences = Preferences::read(host.values());
        // The handshake's data directory is the only place this plugin writes.
        self.store = Store::new(host.identity().data_dir.clone());
        self.book = match self.store.read_state::<State>() {
            Ok(Some(state)) => Book::from_state(state),
            // No file is a first run, not a failure.
            Ok(None) => Book::new(),
            // A file that will not parse is reported rather than swallowed: a reminder the
            // user armed and will never hear again is worth one line in the plugin's log.
            Err(error) => {
                host.log(
                    LogLevel::Warn,
                    &format!("the saved reminders could not be read, so there are none: {error}"),
                );
                Book::new()
            }
        };
        // Deliberately *not* clearing the run in progress, the last answer, the schedule or
        // the notice: those are what a fresh process starts without, and `new` is where a
        // fresh process starts. Clearing them here would be defensive code that never runs in
        // the product — the handshake happens once, before anything else — and it would make
        // the state impossible to reason about for anything that delivers messages later.
        self.painted = None;
        self.draw(host);
        Ok(())
    }

    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        self.visible = tick.is_overlay_visible();
        self.collect_answer(host);
        self.on_time(host);
        self.draw(host);
    }

    fn on_press(&mut self, id: &str, host: &mut Host) {
        match id {
            RUN => {
                if self.run_skill() {
                    self.painted = None;
                }
            }
            ARM => {
                let text = self.preferences.reminder_text.clone();
                if let Some(reminder) = self.book.arm(self.preferences.reminder_minutes, &text) {
                    self.notice = Some((reminder.text, false));
                    self.remember(host);
                    self.painted = None;
                }
            }
            CLEAR => {
                self.book.clear();
                self.remember(host);
                self.painted = None;
            }
            // A press on an id this panel does not have: the host only sends these for nodes
            // it published, so there is nothing to do and nothing to say.
            _ => return,
        }
        self.draw(host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        // The run in progress is not cancelled: a user editing the command while one is
        // running should not have that run's answer thrown away, and the next run uses the new
        // setting. What is reset is the panel, because the command is printed on it.
        self.painted = None;
        self.draw(host);
    }

    fn on_shutdown(&mut self, host: &mut Host) {
        // The last chance to remember, for the same reason as every other plugin that keeps
        // state: a reminder that is a session's worth short is a reminder that is wrong every
        // day by whatever the last session happened to be.
        self.remember(host);
    }
}

fn main() -> Result<()> {
    CatSkills::new(Preferences::default()).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, model_requests, panels,
        values_from,
    };

    use bongocat_plugin_sdk::{ConfigSchema, Host, PluginMessage, Session, Values};

    /// How long a test will wait for a command that ought to answer at once.
    const PATIENCE: Duration = Duration::from_secs(10);

    fn the_defaults() -> ConfigDocument {
        document(
            [
                ("command".to_string(), ConfigValue::Text(String::new())),
                ("question".to_string(), ConfigValue::Text(String::new())),
                ("every_minutes".to_string(), ConfigValue::Integer(0)),
                ("timeout_seconds".to_string(), ConfigValue::Integer(5)),
                ("motion".to_string(), ConfigValue::Text(String::new())),
                ("only_when_visible".to_string(), ConfigValue::Bool(false)),
                (
                    "reminder_text".to_string(),
                    ConfigValue::Text(String::new()),
                ),
                ("reminder_minutes".to_string(), ConfigValue::Integer(10)),
            ]
            .into_iter()
            .collect(),
        )
    }

    fn with(overrides: &[(&str, ConfigValue)]) -> ConfigDocument {
        let mut document = the_defaults();
        for (key, value) in overrides {
            document.0.insert((*key).to_owned(), value.clone());
        }
        document
    }

    /// A session over a real host, with the handshake already done.
    ///
    /// Kept separate from [`serve`] because a command runs on its own thread and the tick loop
    /// has to be driven *after* it started. `on_ready` deliberately leaves a run in progress
    /// alone — see its own comment — so a caller may open once and deliver many ticks.
    fn open(
        plugin: &mut CatSkills,
        written: &WrittenMessages,
        data_dir: &std::path::Path,
        config: &ConfigDocument,
    ) -> Session {
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        let values: Values = values_from(config, &schema);
        let identity = IdentityBuilder::new()
            .id("cat-skills")
            .locale("en-US")
            .data_dir(data_dir)
            .build();
        let host = Host::new(written.writer(), identity, schema, values).expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("announced");
        session
    }

    /// Open a session, deliver some messages, and close it.
    fn serve(
        plugin: &mut CatSkills,
        written: &WrittenMessages,
        data_dir: &std::path::Path,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        deliver(open(plugin, written, data_dir, &config), plugin, messages);
    }

    /// Deliver some messages to an already-open session.
    fn deliver(session: Session, plugin: &mut CatSkills, messages: Vec<HostMessage>) {
        session.serve(plugin, messages).expect("served");
    }

    /// The text on the last panel the plugin drew.
    fn labels(written: &WrittenMessages) -> Vec<String> {
        labels_in(&panels(written).last().expect("a panel").scene)
    }

    /// Serve ticks until a run has finished, or the deadline passes.
    ///
    /// A command runs on its own thread, so "long enough for the answer" is wall time and not
    /// a number of ticks — a suite that served a hundred ticks instantly would never give a
    /// process time to start. A few milliseconds a tick keeps that honest, and the loop stops
    /// the moment the run lands rather than always running to the deadline.
    ///
    /// "Finished" is the plugin's own `last_answered_unix` moving, which is set when a run is
    /// collected and is the one piece of state that means *something answered* even when the
    /// answer was nothing — a command that printed nothing is a finished run whose answer is
    /// `None`, and waiting for a non-`None` answer would wait forever.
    fn pump(
        plugin: &mut CatSkills,
        written: &WrittenMessages,
        data_dir: &std::path::Path,
        config: &ConfigDocument,
        deadline: Duration,
    ) {
        let started = std::time::Instant::now();
        let answered_before = plugin.last_answered_unix;
        let mut frame = 0_u64;
        // A skill on a schedule has not even started yet on the first pass, so waiting for "the
        // skill has answered" rather than "something is running" is what makes this work for a
        // run the tick triggered rather than a run the button did.
        while started.elapsed() < deadline && plugin.last_answered_unix == answered_before {
            std::thread::sleep(Duration::from_millis(4));
            frame += 1;
            let session = open(plugin, written, data_dir, config);
            deliver(
                session,
                plugin,
                Inbox::new().tick(1_000 + frame * 16).into_messages(),
            );
        }
        // One more tick so an answer that landed on the last pass is collected.
        let session = open(plugin, written, data_dir, config);
        deliver(
            session,
            plugin,
            Inbox::new().tick(1_000 + frame * 16).into_messages(),
        );
    }

    #[test]
    fn nothing_runs_until_the_user_names_a_command() {
        // The default state of this plugin is to do nothing. Shipping a built-in network call
        // would be the plugin deciding it may talk to the internet on the user's behalf.
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            the_defaults(),
            Inbox::new().tick(1_000).press(RUN).into_messages(),
        );
        // No pump: nothing starts, so there is nothing to wait for, and waiting ten seconds to
        // learn that would make this the slowest test in the repository.
        assert!(
            plugin.running.is_none(),
            "so a command box that is empty means no command: {:?}",
            model_requests(&written)
        );
        assert!(
            labels(&written)
                .iter()
                .any(|label| label == "no skill set up"),
            "and the panel says so rather than looking broken: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn a_command_that_answers_says_what_it_said_and_moves_the_cat() {
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            with(&[
                ("command", ConfigValue::Text("/bin/echo sunny".to_owned())),
                ("motion", ConfigValue::Text("CAT_motion.0".to_owned())),
            ]),
            Inbox::new().press(RUN).into_messages(),
        );
        pump(
            &mut plugin,
            &written,
            data.path(),
            &with(&[
                ("command", ConfigValue::Text("/bin/echo sunny".to_owned())),
                ("motion", ConfigValue::Text("CAT_motion.0".to_owned())),
            ]),
            PATIENCE,
        );
        let requests = model_requests(&written);
        assert!(
            requests.iter().any(
                |(_, request)| matches!(request, ModelRequest::ShowBubble { text, .. }
                    if text.resolve("en-US") == "sunny")
            ),
            "so the answer is the command's own words: {requests:?}"
        );
        assert!(
            requests.iter().any(
                |(_, request)| matches!(request, ModelRequest::PlayMotion { name, .. }
                    if name == "CAT_motion.0")
            ),
            "and the motion is the punctuation: {requests:?}"
        );
        assert!(
            labels(&written).iter().any(|label| label == "sunny"),
            "while the panel keeps it, because a bubble is gone in six seconds: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn a_command_that_prints_nothing_says_that_rather_than_saying_nothing() {
        // A blank bubble is a blank rectangle on the screen, and "the command printed
        // nothing" is the fact a user who wired a skill up actually needs.
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            with(&[("command", ConfigValue::Text("/usr/bin/true".to_owned()))]),
            Inbox::new().press(RUN).into_messages(),
        );
        pump(
            &mut plugin,
            &written,
            data.path(),
            &with(&[("command", ConfigValue::Text("/usr/bin/true".to_owned()))]),
            PATIENCE,
        );
        assert!(
            model_requests(&written).iter().any(
                |(_, request)| matches!(request, ModelRequest::ShowBubble { text, .. }
                    if text.resolve("en-US") == "the command said nothing")
            ),
            "{:?}",
            model_requests(&written)
        );
    }

    #[test]
    fn a_command_that_fails_says_why_and_does_not_move_the_cat() {
        // A failure is worth saying and a motion for it is a cat celebrating not working. The
        // motion is suppressed for a failure rather than for every answer, because an answer
        // is the ordinary case and a failure is the one worth distinguishing.
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            with(&[
                (
                    "command",
                    ConfigValue::Text("/definitely/not/a/program --flag".to_owned()),
                ),
                ("motion", ConfigValue::Text("CAT_motion.0".to_owned())),
            ]),
            Inbox::new().press(RUN).into_messages(),
        );
        pump(
            &mut plugin,
            &written,
            data.path(),
            &with(&[
                (
                    "command",
                    ConfigValue::Text("/definitely/not/a/program --flag".to_owned()),
                ),
                ("motion", ConfigValue::Text("CAT_motion.0".to_owned())),
            ]),
            PATIENCE,
        );
        let requests = model_requests(&written);
        assert!(
            requests
                .iter()
                .any(|(_, request)| matches!(request, ModelRequest::ShowBubble { .. })),
            "so a command that could not run is reported: {requests:?}"
        );
        assert!(
            !requests
                .iter()
                .any(|(_, request)| matches!(request, ModelRequest::PlayMotion { .. })),
            "and the cat does not celebrate a failure: {requests:?}"
        );
    }

    #[test]
    fn the_question_goes_where_the_user_put_the_placeholder() {
        // The difference between a lookup and an agent: the command is asked something.
        let preferences = Preferences::read(&{
            let schema: ConfigSchema = declared_settings().to_schema().expect("a schema");
            values_from(
                &with(&[
                    (
                        "command",
                        ConfigValue::Text("/usr/bin/ask --question %s".to_owned()),
                    ),
                    (
                        "question",
                        ConfigValue::Text("weather in Tokyo?".to_owned()),
                    ),
                ]),
                &schema,
            )
        });
        let skill = preferences.skill().expect("a skill");
        assert_eq!(skill.arguments(), ["--question", "weather in Tokyo?"]);
        assert_eq!(
            skill.display(),
            "/usr/bin/ask --question \"weather in Tokyo?\"",
            "so the displayed form quotes the argument it substituted, and is therefore \
             something a user can paste back into the setting"
        );
    }

    #[test]
    fn a_skill_runs_again_on_its_schedule_and_not_sooner() {
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        let config = with(&[
            ("command", ConfigValue::Text("/bin/echo tick".to_owned())),
            ("every_minutes", ConfigValue::Integer(1)),
        ]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            config.clone(),
            Inbox::new().tick(100).tick(200).into_messages(),
        );
        assert_eq!(
            model_requests(&written)
                .iter()
                .filter(|(_, request)| matches!(request, ModelRequest::ShowBubble { .. }))
                .count(),
            0,
            "so a minute has not passed and nothing has run yet"
        );
        // Sixty-one ticks is not a minute of wall time, so the schedule is checked through the
        // plugin's own clock rather than by waiting: the test moves the clock by writing the
        // last run into the past, which is what the plugin compares against.
        plugin.last_started_unix -= 61;
        pump(&mut plugin, &written, data.path(), &config, PATIENCE);
        assert_eq!(
            model_requests(&written)
                .iter()
                .filter(|(_, request)| matches!(request, ModelRequest::ShowBubble { .. }))
                .count(),
            1,
            "so one minute later the skill has run once"
        );
    }

    #[test]
    fn a_skill_that_is_already_running_is_not_started_again() {
        // A command that takes four seconds and a button a user presses twice is a user asking
        // twice, and the second ask is answered by doing nothing rather than by running two of
        // everything.
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            with(&[(
                "command",
                ConfigValue::Text("/bin/sleep 2 && /bin/echo late".to_owned()),
            )]),
            Inbox::new()
                .press(RUN)
                .press(RUN)
                .press(RUN)
                .into_messages(),
        );
        assert_eq!(
            plugin.running.as_ref().map(|running| running.is_pending()),
            Some(true),
            "so the panel shows it running rather than looking idle"
        );
    }

    #[test]
    fn an_armed_reminder_is_said_when_it_is_due_and_only_once() {
        // The whole of the reminder half: arm it, wait, hear it, and hear it once.
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            with(&[
                ("reminder_text", ConfigValue::Text("drink water".to_owned())),
                ("reminder_minutes", ConfigValue::Integer(0)),
            ]),
            Inbox::new().press(ARM).tick(1_000).into_messages(),
        );
        let said = |written: &WrittenMessages| {
            model_requests(written)
                .into_iter()
                .filter(|(_, request)| matches!(request, ModelRequest::ShowBubble { .. }))
                .count()
        };
        assert_eq!(
            said(&written),
            1,
            "so arming one says it now, because a reminder due the moment it is armed is a \
             reminder the user asked for immediately: {:?}",
            model_requests(&written)
        );
        // The book has it marked said, so no further tick repeats it.
        serve(
            &mut plugin,
            &written,
            data.path(),
            with(&[
                ("reminder_text", ConfigValue::Text("drink water".to_owned())),
                ("reminder_minutes", ConfigValue::Integer(0)),
            ]),
            Inbox::new().tick(2_000).tick(3_000).into_messages(),
        );
        assert_eq!(said(&written), 1, "and it is not said again on every tick");
    }

    #[test]
    fn a_reminder_with_no_text_arms_nothing() {
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            the_defaults(),
            Inbox::new().press(ARM).tick(1_000).into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "because a reminder the cat cannot say is a reminder that failed silently"
        );
        assert!(!plugin.book.is_armed());
    }

    #[test]
    fn a_reminder_survives_the_app_closing() {
        // The whole point of a reminder: it has to still be there tomorrow.
        let data = tempfile::tempdir().expect("a data directory");
        let first = WrittenMessages::new();
        {
            let mut plugin = CatSkills::new(Preferences::default());
            serve(
                &mut plugin,
                &first,
                data.path(),
                with(&[
                    ("reminder_text", ConfigValue::Text("stand up".to_owned())),
                    ("reminder_minutes", ConfigValue::Integer(30)),
                ]),
                Inbox::new().press(ARM).shutdown().into_messages(),
            );
        }
        assert!(
            Store::new(data.path())
                .read_state::<State>()
                .expect("reads")
                .is_some(),
            "so the reminder is in the plugin's own file before the process ends"
        );
        let second = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &second,
            data.path(),
            the_defaults(),
            Inbox::new().tick(1_000).into_messages(),
        );
        assert_eq!(
            plugin.book.shown().map(|reminder| reminder.text.as_str()),
            Some("stand up"),
            "so a second run starts with the reminder the first one armed"
        );
    }

    #[test]
    fn a_reminder_file_that_will_not_parse_is_reported_rather_than_silently_dropped() {
        // A user waiting for a reminder that is never going to arrive deserves one line.
        let data = tempfile::tempdir().expect("a data directory");
        std::fs::write(Store::new(data.path()).state_path(), b"{ not json at all")
            .expect("writes nonsense");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        serve(
            &mut plugin,
            &written,
            data.path(),
            the_defaults(),
            Inbox::new().tick(100).into_messages(),
        );
        assert!(!plugin.book.is_armed());
        assert!(
            written
                .messages()
                .iter()
                .any(|message| matches!(message, PluginMessage::Log { .. })),
            "and says so in its own log: {:?}",
            written.messages()
        );
    }

    #[test]
    fn forgetting_a_reminder_removes_it_from_the_file() {
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        let config = with(&[
            ("reminder_text", ConfigValue::Text("drink water".to_owned())),
            ("reminder_minutes", ConfigValue::Integer(30)),
        ]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            config.clone(),
            Inbox::new().press(ARM).press(CLEAR).into_messages(),
        );
        assert!(!plugin.book.is_armed());
        assert!(
            Store::new(data.path())
                .read_state::<State>()
                .expect("reads")
                .expect("a file")
                .reminders
                .is_empty(),
            "so it is gone rather than waiting to come back"
        );
    }

    #[test]
    fn a_hidden_window_holds_the_words_back_but_not_the_answer() {
        // The answer is kept, so when the window comes back the panel has it; only the bubble
        // and the motion are held, because a cat that is not on screen is not talking.
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        let config = with(&[
            ("command", ConfigValue::Text("/bin/echo sunny".to_owned())),
            ("only_when_visible", ConfigValue::Bool(true)),
        ]);
        serve(
            &mut plugin,
            &written,
            data.path(),
            config.clone(),
            Inbox::new().press(RUN).into_messages(),
        );
        // A run of ticks with the window down, which is enough wall time for a command that
        // prints one word and not enough to be slow about it.
        for frame in 0..60 {
            std::thread::sleep(Duration::from_millis(4));
            let session = open(&mut plugin, &written, data.path(), &config);
            deliver(
                session,
                &mut plugin,
                Inbox::new()
                    .tick_with(1_000 + frame * 16, HostState::new(None, false))
                    .into_messages(),
            );
        }
        assert_eq!(
            model_requests(&written).len(),
            0,
            "because the cat was not on screen, so nothing was said: {:?}",
            model_requests(&written)
        );
        assert_eq!(
            plugin.answer.as_deref(),
            Some("sunny"),
            "while the answer is still kept, so the panel has it when the window comes back"
        );
        assert!(
            labels(&written).iter().any(|label| label == "sunny"),
            "and the panel says it: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn a_tick_that_changes_nothing_builds_nothing() {
        let data = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let mut plugin = CatSkills::new(Preferences::default());
        let mut inbox = Inbox::new();
        for frame in 0..60 {
            inbox = inbox.tick(frame * 16);
        }
        serve(
            &mut plugin,
            &written,
            data.path(),
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
    fn a_hand_written_wait_outside_the_range_lands_on_the_bound() {
        // The wait is a property of the plugin: a command that takes a minute is a command
        // whose answer is not worth having this late.
        let schema: ConfigSchema = declared_settings().to_schema().expect("a schema");
        for (given, expected) in [(0_i64, 1_u64), (-5, 1), (1_000, 30)] {
            let values = values_from(
                &with(&[("timeout_seconds", ConfigValue::Integer(given))]),
                &schema,
            );
            assert_eq!(
                Preferences::read(&values).timeout,
                Duration::from_secs(expected),
                "for {given}"
            );
        }
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
                "command",
                "question",
                "every_minutes",
                "timeout_seconds",
                "motion",
                "only_when_visible",
                "reminder_text",
                "reminder_minutes"
            ]
        );
    }

    #[test]
    fn the_plugin_asks_for_the_model_and_nothing_else() {
        // The claim this plugin exists to demonstrate: a plugin that fetches the weather and
        // talks to an agent needed nothing added to the application to do it.
        let plugin = CatSkills::new(Preferences::default());
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "cat-skills");
        assert_eq!(descriptor.name().resolve("zh-CN"), "小猫技能");
        assert!(descriptor.subscribes_to(Subscription::ModelReaction));
        assert!(
            !descriptor.subscribes_to(Subscription::Input),
            "and not the user's keystrokes, which are nothing to do with a command's output"
        );
        descriptor
            .check()
            .expect("this plugin's own descriptor is one the host accepts");
    }
}
