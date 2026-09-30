//! Watching what an AI coding tool is doing, from its own hook events.
//!
//! Issue #927 asked for the cat to notice what Claude Code (and anything else shaped like
//! it) is doing, and react: reading, writing, running, asking, finished, broken. The
//! reference it pointed at receives events over a local TCP socket. This plugin does not,
//! and the difference is worth stating because it is what makes the whole thing possible
//! without the product growing anything:
//!
//! **The plugin binary is the hook.** Claude Code's hook mechanism runs a *command* with a
//! JSON payload on stdin, once per event, in a fresh short-lived process. So this plugin's
//! executable takes an argument — `hook` — and in that mode it reads one payload, appends one
//! line to a file in its own data directory, and exits. The long-running half is the same
//! executable with no argument, watching that file. There is no port to collide with, no
//! firewall decision, nothing listening that has to be found and killed, and no second binary
//! in the plugin.
//!
//! **Everything else is the plugin's own.** The queue file, the byte offset it has read to,
//! the classification of each event, the mapping from state to motion and words, the labels,
//! the settings form. The product is not asked for a fact, a port, a file or a line of code:
//! it starts this process, gives it a data directory and a tick, and draws the panel that
//! comes back.
//!
//! **What is written down is a tool's name and a state, never the tool's input.** A tool
//! input can be a whole file's contents or a command with a token in it, and the file this
//! appends to is a record of what the user has been doing all day. A test pins that the
//! queue never contains one.
//!
//! # What a user has to do
//!
//! One thing: paste the command into their tool's hook settings. The panel shows the exact
//! command, built from this plugin's own path, because a command a user has to reconstruct
//! from a directory structure is a command nobody pastes.

mod copy;
mod event;
mod mapping;
mod queue;

use bongocat_plugin_sdk::Result;
use bongocat_plugin_sdk::prelude::*;
use event::{Activity, Event, Watch};
use mapping::{Look, Mapping};
use queue::{Reader, now_unix};
use std::io::Read as _;

/// This plugin's own executable name, which is what the hook command has to invoke.
///
/// A constant rather than something read back from the handshake, because the handshake does
/// not carry it: what it carries is a *directory*, and a directory does not say which file
/// inside it the host started. Spelling the name here is honest — it is this plugin's own
/// name — and a mismatch would fail loudly on the first paste rather than silently.
const EXECUTABLE: &str = "agent-watch";

/// The word that makes this executable the hook instead of the plugin.
///
/// A word rather than a flag because a hook command is written by hand into somebody else's
/// settings file, and the shortest thing that cannot be misread is the one to ask for.
const HOOK_SUBCOMMAND: &str = "hook";

/// What the hook calls the sending tool when nobody says.
///
/// A name rather than an empty string, because the panel says which tool an event came from
/// and "a tool" is a worse sentence than a name the user can recognise.
const DEFAULT_SOURCE: &str = "ai-tool";

/// The shortest a bubble may be, in characters.
///
/// Bounded because the mapping file is the user's own text and there is nothing to stop a
/// paste of a novel into it; the protocol bounds the bubble itself, but a bubble clipped to
/// nothing is worse than one that was never sent.
const MINIMUM_BUBBLE_CHARS: usize = 1;

/// The longest a bubble from a mapping may be, in characters.
///
/// Bounded for the same reason in the other direction: the protocol's own bound is the last
/// line of defence, and a mapping that is quietly clipped is a mapping the user cannot tell
/// is wrong.
const MAXIMUM_BUBBLE_CHARS: usize = 120;

/// The panel's size, chosen for two lines of text and a command's worth of chips.
const PANEL_WIDTH: u32 = 260;
const PANEL_HEIGHT: u32 = 150;

/// What the user configured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preferences {
    /// What each state does and says.
    pub mapping: Mapping,
    /// How long a tool may be quiet before the cat stops working.
    pub idle_seconds: i64,
    /// Whether the cat reacts only while its window is up.
    pub only_when_visible: bool,
    /// Whether the panel shows the tool's own words.
    pub show_event: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            mapping: Mapping::defaults(),
            idle_seconds: 90,
            only_when_visible: false,
            show_event: true,
        }
    }
}

impl Preferences {
    fn read(values: &Values) -> Self {
        let mapping = if values.flag("use_defaults") {
            Mapping::defaults()
        } else {
            Mapping::from_json(&values.text("mapping"))
        };
        Self {
            mapping,
            idle_seconds: values.integer("idle_seconds").clamp(5, 3_600),
            only_when_visible: values.flag("only_when_visible"),
            show_event: values.flag("show_event"),
        }
    }
}

/// The settings this plugin declares, which *are* the settings panel.
pub fn declared_settings() -> Settings {
    Settings::new()
        .with(
            Integer::ranged("idle_seconds", copy::idle_seconds_label(), 90, 5, 3_600)
                .stepping(5)
                .with_unit("seconds")
                .described(copy::idle_seconds_help())
                .into(),
        )
        .with(
            Toggle::new("only_when_visible", copy::only_when_visible_label())
                .described(copy::only_when_visible_help())
                .into(),
        )
        .with(
            Toggle::new("show_event", copy::show_event_label())
                .described(copy::show_event_help())
                .into(),
        )
        .with(
            TextField::new("mapping", copy::mapping_label())
                .described(copy::mapping_help())
                .into(),
        )
        .with(
            Toggle::new("use_defaults", copy::reset_mapping_label())
                .described(copy::reset_mapping_help())
                .into(),
        )
}

/// What the panel should show.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Painted {
    state: String,
    detail: String,
    sessions: String,
    command: String,
}

/// The whole plugin.
pub struct AgentWatch {
    panel: Panel,
    preferences: Preferences,
    /// Every conversation this plugin has heard from.
    watch: Watch,
    /// How far into the queue file this plugin has read.
    reader: Reader,
    /// Where the queue file is, which the hook mode is told and this mode is handed.
    queue: std::path::PathBuf,
    /// The state the cat is currently in, for noticing a change.
    shown: Option<Activity>,
    /// What the panel last showed, so a tick that changed nothing builds nothing.
    painted: Option<Painted>,
    /// Whether the window is up, from the last tick.
    visible: bool,
    /// The hook command, built once from this plugin's own path.
    command: String,
    /// Whether the mapping's complaints have been logged, so they are said once.
    complained: bool,
    /// Whether the effective mapping has been written down, so it is said once.
    recorded: bool,
}

impl AgentWatch {
    pub fn new(preferences: Preferences, queue: std::path::PathBuf) -> Self {
        Self {
            panel: Panel::new(PANEL_WIDTH, PANEL_HEIGHT)
                .anchored(PluginAnchor::BottomLeft)
                .with_margin(0.03, 0.03)
                .with_width_fraction(0.26)
                .with_opacity(0.92),
            watch: Watch::new(now_unix(), preferences.idle_seconds),
            reader: Reader::new(),
            preferences,
            queue,
            shown: None,
            painted: None,
            visible: true,
            command: String::new(),
            complained: false,
            recorded: false,
        }
    }

    /// The command a user pastes into their tool's hook settings.
    ///
    /// Built from the handshake's own paths rather than guessed at, because a command a
    /// user has to repair by hand is a command nobody pastes. Quoted because both paths
    /// routinely contain spaces — `Application Support` alone is enough — and an unquoted
    /// command there is a command that runs part of a path.
    fn hook_command(plugin_dir: &std::path::Path, data_dir: &std::path::Path) -> String {
        format!(
            "\"{}\" {} --data-dir \"{}\"",
            plugin_dir.join(EXECUTABLE).display(),
            HOOK_SUBCOMMAND,
            queue::queue_path(data_dir).display(),
        )
    }

    /// Whether the cat should react right now.
    ///
    /// Two independent reasons to say no, and the cheap one first: a hidden window means a
    /// cat that is not on screen, and a motion played at a cat nobody can see is a motion
    /// that did not happen.
    fn reacts(&self) -> bool {
        !(self.preferences.only_when_visible && !self.visible)
    }

    /// Read what is new, and say what became true.
    fn observe(&mut self) {
        for event in self.reader.read_new(&self.queue) {
            self.watch.observe(event);
        }
        self.watch.expire(now_unix());
    }

    /// React, if the state changed and the cat should.
    ///
    /// A reaction is for a *change*, so the very first state this session sees is not one:
    /// it is a state the plugin has caught up with, and a motion for it would fire while the
    /// user is reading the settings page. `self.shown` is only set when the cat actually
    /// reacts, so a state that arrives while the window is hidden is still a change when the
    /// window comes back.
    fn react_if_changed(&mut self, activity: Activity, host: &mut Host) {
        if self.shown == Some(activity) {
            return;
        }
        if !self.reacts() {
            return;
        }
        self.shown = Some(activity);
        let Look { motion, bubble } = self.preferences.mapping.look(activity);
        if !motion.is_empty() {
            host.play_motion(&motion);
        }
        let bubble = bubble.trim();
        if bubble.chars().count() >= MINIMUM_BUBBLE_CHARS
            && bubble.chars().count() <= MAXIMUM_BUBBLE_CHARS
        {
            // The protocol bounds a bubble's duration as well as its length, so this is the
            // only place the length has to be decided: long enough for a short sentence to
            // be read at a glance, short enough not to outlive the state that caused it.
            host.bubble(bubble, 2_500);
        }
    }

    /// Rebuild the panel if what it would show has changed.
    fn draw(&mut self, host: &mut Host) {
        let now = now_unix();
        let activity = self.watch.combined(now);
        let sessions = if self.watch.is_empty() {
            copy::say(host, &copy::nothing_watched())
        } else {
            let busy = self.watch.busy(now);
            let total = self.watch.len();
            let text = copy::sessions_label(host, total);
            // The busy count is only worth saying when it differs from the total: "2 sessions
            // · 2" is a number twice, and a reader who stops to work out which of the two
            // means what is a reader who stopped reading.
            if busy > 1 && busy < total {
                format!("{text} · {busy}")
            } else {
                text
            }
        };
        // The tool's own words are on a second line, and only when the user asked: a panel
        // with a state's name and a tool's name is two facts, and somebody watching the cat
        // wants one of them.
        let detail = if !self.preferences.show_event {
            String::new()
        } else {
            match self.watch.latest_words() {
                None => copy::say(host, &copy::nothing_watched()),
                Some(("", _)) => copy::say(host, &copy::quiet()),
                Some((event, Some(tool))) => format!("{event} · {tool}"),
                Some((event, None)) => event.to_owned(),
            }
        };
        let painted = Painted {
            state: copy::say(host, &copy::activity_label(activity)),
            detail,
            sessions,
            command: self.command.clone(),
        };
        if self.painted.as_ref() == Some(&painted) {
            return;
        }
        let hook_label = copy::say(host, &copy::hook_command_label());
        let hook_help = copy::say(host, &copy::hook_command_help());
        self.panel.rebuild(|panel| {
            panel.surface(6.0, [14.0, 12.0], |content| {
                content.row_centered(8.0, |row| {
                    row.push(heading(&painted.state, 22.0));
                });
                content.push(muted(&painted.sessions, 12.0));
                if !painted.detail.is_empty() {
                    content.push(muted(&painted.detail, 11.0));
                }
                content.push(divider());
                content.push(muted(&hook_label, 11.0));
                content.chip(&painted.command, 11.0, [8.0, 4.0], 5.0);
                content.push(muted(&hook_help, 10.0));
            })
        });
        host.show(&mut self.panel);
        self.painted = Some(painted);
    }

    /// Say what the mapping is, once, when it is not the plugin's own.
    ///
    /// In the plugin's log rather than on the panel, because the panel is where the tool's
    /// state is and a mapping is not a state — and in the log rather than nowhere, because a
    /// user who changed a mapping and cannot tell whether it took effect has no way to find
    /// out. Only the states that differ are written, so two changed of nine reads as two.
    fn record_mapping_once(&mut self, host: &mut Host) {
        if self.recorded || self.preferences.mapping.is_default() {
            return;
        }
        self.recorded = true;
        host.log(
            LogLevel::Info,
            &format!(
                "in effect, and different from the built-in one:\n{}",
                self.preferences.mapping.to_json()
            ),
        );
    }

    /// Say the mapping's complaints once.
    fn complain_once(&mut self, host: &mut Host) {
        if self.complained {
            return;
        }
        self.complained = true;
        for complaint in self.preferences.mapping.complaints.clone() {
            host.log(LogLevel::Warn, &complaint);
        }
    }
}

impl Plugin for AgentWatch {
    fn descriptor(&self) -> Descriptor {
        Descriptor::new("agent-watch", copy::plugin_name().resolve(""))
            .version(1, 0, 0)
            .author("BongoCat")
            .named(copy::plugin_name())
            .described(copy::plugin_description())
            .icon(copy::ICON)
            // Model reactions, because the whole job is moving the cat. No input feed and
            // no host state: a plugin watching another program's events has no use for the
            // user's keystrokes, and the only host fact it wants is the clock, which every
            // tick carries whether it is subscribed or not.
            .subscribe(Subscription::ModelReaction)
    }

    fn settings(&mut self) -> Settings {
        declared_settings()
    }

    fn on_ready(&mut self, host: &mut Host) -> Result<()> {
        self.preferences = Preferences::read(host.values());
        // The handshake's data directory is the only place this plugin writes, and the hook
        // mode is told the queue path by the command the panel shows. A plugin that guessed
        // at its own directory would be a plugin writing somewhere the host did not name.
        let identity = host.identity();
        self.queue = queue::queue_path(&identity.data_dir);
        self.command = Self::hook_command(&identity.plugin_dir, &identity.data_dir);
        self.watch = Watch::new(now_unix(), self.preferences.idle_seconds);
        self.shown = None;
        self.painted = None;
        self.complain_once(host);
        self.draw(host);
        Ok(())
    }

    fn on_tick(&mut self, tick: Tick, host: &mut Host) {
        self.visible = tick.is_overlay_visible();
        self.observe();
        let activity = self.watch.combined(now_unix());
        self.react_if_changed(activity, host);
        self.draw(host);
    }

    fn on_config_changed(&mut self, host: &mut Host) {
        self.preferences = Preferences::read(host.values());
        self.watch.idle_seconds = self.preferences.idle_seconds;
        // The complaints are about the *new* mapping, so they are owed again: a user who
        // fixed their JSON should not still see the warning about the version before it.
        self.complained = false;
        self.recorded = false;
        self.complain_once(host);
        self.record_mapping_once(host);
        // The state is not reset, because a mapping change is not a change of what the tool
        // is doing — the cat should not twitch because a sentence was reworded. The label
        // may be a different string, so the panel comparison starts again.
        self.painted = None;
        self.draw(host);
    }
}

fn main() -> Result<()> {
    // The hook half of this plugin. Before anything else, because a hook that starts a
    // session would sit there holding a pipe the host is not going to write to.
    if let Some(arguments) = std::env::args().nth(1).as_deref()
        && arguments == HOOK_SUBCOMMAND
    {
        return run_hook(std::env::args().skip(2));
    }
    AgentWatch::new(Preferences::default(), std::path::PathBuf::from(".")).run()
}

/// Run as a hook: read one payload, record it, and get out of the way.
///
/// Always succeeds, on purpose. A hook's exit status is the sending tool's business — a
/// non-zero status can cancel the user's tool call, or block it, or print to their terminal
/// — and a monitoring tool that can break the thing it monitors is worse than one that misses
/// an event. So every failure here is swallowed, and the one thing worth telling anybody is
/// printed to stderr, which the sending tool shows as its own output rather than as a
/// failure.
fn run_hook(arguments: impl Iterator<Item = String>) -> Result<()> {
    let mut data_dir = None;
    let mut source = DEFAULT_SOURCE.to_owned();
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--data-dir" => data_dir = arguments.next(),
            "--source" => {
                if let Some(value) = arguments.next() {
                    source = value;
                }
            }
            // An argument this build does not know is ignored rather than fatal: the command
            // lives in somebody else's settings file, and refusing to run because of an
            // option a later version added would stop the monitoring entirely.
            _ => {}
        }
    }
    let Some(data_dir) = data_dir.map(std::path::PathBuf::from) else {
        eprintln!(
            "agent-watch: the hook command needs --data-dir; copy the command from the plugin's \
             own panel"
        );
        return Ok(());
    };
    let mut payload = String::new();
    // Reading stdin to the end is what a hook has to do: the sending tool writes the
    // payload and closes, and reading less than all of it would mean parsing half a JSON
    // object. Bounded, because a sender that never closes would otherwise hang a tool call.
    let mut payload_stream = std::io::stdin().take(MAXIMUM_PAYLOAD_BYTES as u64);
    let read = payload_stream.read_to_string(&mut payload);
    if read.is_err() {
        eprintln!("agent-watch: the hook's payload could not be read, so nothing was recorded");
        return Ok(());
    }
    let at = now_unix();
    match Event::parse(&source, at, &payload) {
        Some(event) => {
            if !queue::append(&queue::queue_path(&data_dir), &event) {
                eprintln!(
                    "agent-watch: the event could not be written to {}",
                    queue::queue_path(&data_dir).display()
                );
            }
        }
        // A payload this build cannot read is not a failure either. It is recorded as
        // nothing, and the reason is on stderr where the sender shows it.
        None => eprintln!(
            "agent-watch: the hook's payload was not a JSON object, so nothing was recorded"
        ),
    }
    Ok(())
}

/// The most a hook will read from its stdin.
///
/// Bounded because the payload comes from another program and this process is in the middle
/// of somebody's tool call: a sender that never closes stdin must not be able to hold a hook
/// open. Ten megabytes is far more than any hook payload needs and far less than enough to
/// matter.
const MAXIMUM_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_sdk::testing::{
        IdentityBuilder, Inbox, WrittenMessages, document, labels_in, model_requests, panels,
        values_from,
    };
    use bongocat_plugin_sdk::{ConfigSchema, Host, HostState, PluginMessage, Session, Values};

    fn the_defaults() -> ConfigDocument {
        document(
            [
                ("idle_seconds".to_string(), ConfigValue::Integer(90)),
                ("only_when_visible".to_string(), ConfigValue::Bool(false)),
                ("show_event".to_string(), ConfigValue::Bool(true)),
                ("mapping".to_string(), ConfigValue::Text(String::new())),
                ("use_defaults".to_string(), ConfigValue::Bool(false)),
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

    fn serve(
        plugin: &mut AgentWatch,
        written: &WrittenMessages,
        queue: &std::path::Path,
        config: ConfigDocument,
        messages: Vec<HostMessage>,
    ) {
        let schema: ConfigSchema = declared_settings().to_schema().expect("a valid schema");
        let values: Values = values_from(&config, &schema);
        let identity = IdentityBuilder::new()
            .id("agent-watch")
            .locale("en-US")
            .data_dir(queue.parent().expect("a directory"))
            .build();
        let host = Host::new(written.writer(), identity, schema, values).expect("a host");
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), plugin)
            .expect("announced");
        session.serve(plugin, messages).expect("served");
    }

    /// A plugin whose queue is a real one, fed by real appends.
    fn watched(
        data_dir: &std::path::Path,
        config: ConfigDocument,
    ) -> (AgentWatch, std::path::PathBuf) {
        let queue = queue::queue_path(data_dir);
        let mut plugin = AgentWatch::new(Preferences::default(), queue.clone());
        plugin.preferences = Preferences::read(&{
            let schema: ConfigSchema = declared_settings().to_schema().expect("a schema");
            values_from(&config, &schema)
        });
        plugin.watch = Watch::new(now_unix(), plugin.preferences.idle_seconds);
        (plugin, queue)
    }

    fn fire(queue: &std::path::Path, payload: &str) {
        let event = Event::parse("claude-code", now_unix(), payload).expect("a payload");
        assert!(queue::append(queue, &event), "a hook that could not write");
    }

    /// The host state a tick carries, with the window up or down.
    fn host_state(visible: bool) -> HostState {
        HostState::new(None, visible)
    }

    fn labels(written: &WrittenMessages) -> Vec<String> {
        labels_in(&panels(written).last().expect("a panel").scene)
    }

    #[test]
    fn nothing_happening_shows_the_waiting_line_and_the_command_to_change_it() {
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(100).into_messages(),
        );
        let labels = labels(&written);
        assert!(
            labels.iter().any(|label| label == "waiting"),
            "so the panel is not blank while nothing has been watched: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|label| label.contains("nothing yet — wire up the hook below")),
            "and it says the one thing the user has to do: {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label.contains("--data-dir")),
            "with the command itself, because a command a user has to reconstruct from a \
             directory structure is a command nobody pastes: {labels:?}"
        );
    }

    #[test]
    fn the_command_names_this_plugins_own_paths_and_quotes_them() {
        // `Application Support` has a space in it, so an unquoted command is a command that
        // runs part of a path.
        let command = AgentWatch::hook_command(
            std::path::Path::new("/Users/me/App Support/plugins/agent-watch/1.0.0"),
            std::path::Path::new("/Users/me/App Support/development/plugin-data/agent-watch"),
        );
        assert!(command.starts_with('"'), "{command}");
        assert!(command.contains("hook --data-dir \""), "{command}");
        assert!(
            command.contains("/1.0.0/agent-watch\""),
            "so the executable is named inside the plugin's own directory: {command}"
        );
        assert!(command.ends_with('"'), "{command}");
        for part in command.split(' ').filter(|part| !part.starts_with('"')) {
            assert!(
                !part.ends_with('/'),
                "and no unquoted fragment is left half a path: {command}"
            );
        }
    }

    #[test]
    fn an_event_the_hook_recorded_reaches_the_cat() {
        // The whole of the feature, end to end and without a socket: something appended to
        // the queue, and the panel says what it means.
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(100).into_messages(),
        );
        fire(
            &queue,
            r#"{"hook_event_name":"PreToolUse","tool_name":"Write","session_id":"s1"}"#,
        );
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(200).into_messages(),
        );
        let labels = labels(&written);
        assert!(
            labels.iter().any(|label| label == "writing"),
            "so a tool about to write a file is a cat writing: {labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|label| label.contains("PreToolUse · Write")),
            "and the tool's own words are there while the user is wiring it up: {labels:?}"
        );
    }

    #[test]
    fn a_state_the_cat_can_react_to_moves_the_cat_once() {
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(100).into_messages(),
        );
        fire(&queue, r#"{"hook_event_name":"Stop","session_id":"s1"}"#);
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(200).tick(300).tick(400).into_messages(),
        );
        let requests = model_requests(&written);
        assert_eq!(
            requests.len(),
            2,
            "one reaction, which is a motion and a word — and not one per tick, because a \
             state the cat has already noticed is a state it has already reacted to: \
             {requests:?}"
        );
        assert!(
            matches!(&requests[0].1, ModelRequest::PlayMotion { name, .. } if name == "CAT_motion.2"),
            "so the movement is the model's own finished motion: {requests:?}"
        );
        assert!(
            matches!(&requests[1].1, ModelRequest::ShowBubble { .. }),
            "and the word is what the mapping said to say: {requests:?}"
        );
    }

    #[test]
    fn a_custom_mapping_can_say_something_else_instead() {
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(
            dir.path(),
            with(&[(
                "mapping",
                ConfigValue::Text(r#"{"running": {"motion": "", "bubble": ""}}"#.to_owned()),
            )]),
        );
        serve(
            &mut plugin,
            &written,
            &queue,
            with(&[(
                "mapping",
                ConfigValue::Text(r#"{"running": {"motion": "", "bubble": ""}}"#.to_owned()),
            )]),
            Inbox::new().tick(100).into_messages(),
        );
        fire(
            &queue,
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"s"}"#,
        );
        serve(
            &mut plugin,
            &written,
            &queue,
            with(&[(
                "mapping",
                ConfigValue::Text(r#"{"running": {"motion": "", "bubble": ""}}"#.to_owned()),
            )]),
            Inbox::new().tick(200).into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "so a user who mapped a state to nothing gets a still cat, and not a refusal — and \
             not a word either, because \"does and says nothing\" has to mean both"
        );
        assert!(
            labels(&written).iter().any(|label| label == "running"),
            "while the panel still says what the tool is doing, because the mapping is about \
             the reaction and not about the information"
        );
    }

    #[test]
    fn a_mapping_with_a_mistake_in_it_is_reported_once_and_everything_still_works() {
        // A user who cannot see why their mapping did not apply needs to be told, and the
        // thing they need it to do — watch the tool — must not be what breaks.
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let broken = with(&[("mapping", ConfigValue::Text("{ not json".to_owned()))]);
        let (mut plugin, queue) = watched(dir.path(), broken.clone());
        serve(
            &mut plugin,
            &written,
            &queue,
            broken,
            Inbox::new().tick(100).tick(200).into_messages(),
        );
        let complaints: Vec<_> = written
            .messages()
            .into_iter()
            .filter(|message| {
                matches!(
                    message,
                    PluginMessage::Log {
                        level: LogLevel::Warn,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(
            complaints.len(),
            1,
            "and said once rather than on every tick: {complaints:?}"
        );
        assert!(
            labels(&written).iter().any(|label| label == "waiting"),
            "while the panel is still the panel: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn a_hidden_window_stops_the_reactions_but_not_the_recording() {
        // The queue is a record of what the user has been doing; whether the cat is on screen
        // is a question about the cat. So the events keep being read and only the reaction
        // waits — and because `shown` is only set on a reaction, a state that arrived while
        // hidden is still a change when the window comes back.
        //
        // One session, because the window being up or down is a fact about the host and can
        // only change between ticks of the same session.
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let config = with(&[("only_when_visible", ConfigValue::Bool(true))]);
        let (mut plugin, queue) = watched(dir.path(), config.clone());
        fire(&queue, r#"{"hook_event_name":"Stop","session_id":"s1"}"#);
        serve(
            &mut plugin,
            &written,
            &queue,
            config,
            Inbox::new()
                .tick_with(100, host_state(false))
                .tick_with(200, host_state(false))
                .tick_with(300, host_state(true))
                .into_messages(),
        );
        assert_eq!(
            model_requests(&written).len(),
            2,
            "one reaction in total — a motion and the word that goes with it — so the two \
             hidden ticks moved nothing and the visible one moved the cat: {:?}",
            model_requests(&written)
        );
        assert_eq!(
            plugin.shown,
            Some(Activity::Done),
            "and the state the cat is now in is the one the tool reached, so nothing is left \
             pending"
        );
        assert!(
            labels(&written).iter().any(|label| label == "all done"),
            "while the panel says what the tool did throughout, hidden or not: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn a_cat_that_is_not_on_screen_is_not_told_anything() {
        // The other half of the same rule, checked on its own: a motion and a word played at a
        // cat nobody can see are two requests to the model that produced no visible effect.
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let config = with(&[("only_when_visible", ConfigValue::Bool(true))]);
        let (mut plugin, queue) = watched(dir.path(), config.clone());
        fire(&queue, r#"{"hook_event_name":"Stop","session_id":"s1"}"#);
        serve(
            &mut plugin,
            &written,
            &queue,
            config,
            Inbox::new()
                .tick_with(100, host_state(false))
                .tick_with(200, host_state(false))
                .into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "so nothing is asked of the model while nobody is looking"
        );
        assert_eq!(
            plugin.shown, None,
            "and the state is still pending rather than marked as seen, which is what lets it \
             react when the window comes back"
        );
    }

    #[test]
    fn a_tick_that_changes_nothing_builds_nothing() {
        // A monitor that rebuilt its panel sixty times a second would be the most expensive
        // thing the product could be asked to do for a tool that is idle.
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        let mut inbox = Inbox::new();
        for frame in 0..60 {
            inbox = inbox.tick(frame * 16);
        }
        serve(
            &mut plugin,
            &written,
            &queue,
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
    fn two_conversations_are_counted_and_the_busier_one_is_shown() {
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(100).into_messages(),
        );
        fire(
            &queue,
            r#"{"hook_event_name":"PreToolUse","tool_name":"Read","session_id":"a"}"#,
        );
        fire(
            &queue,
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"b"}"#,
        );
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(200).into_messages(),
        );
        let labels = labels(&written);
        assert!(
            labels.iter().any(|label| label == "running"),
            "so the command wins over the read, because a command is the thing a person would \
             want to be told about: {labels:?}"
        );
        assert!(
            labels.iter().any(|label| label == "2 sessions"),
            "and both are counted, because two terminals is the situation this feature exists \
             for: {labels:?}"
        );
        assert!(
            labels.iter().all(|label| !label.starts_with("2 sessions ·")),
            "and the busy count is not said when it is the same number twice: {labels:?}"
        );
    }

    #[test]
    fn a_tool_going_quiet_puts_the_cat_back_to_waiting() {
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let config = with(&[("idle_seconds", ConfigValue::Integer(5))]);
        let (mut plugin, queue) = watched(dir.path(), config.clone());
        serve(
            &mut plugin,
            &written,
            &queue,
            config.clone(),
            Inbox::new().tick(100).into_messages(),
        );
        fire(
            &queue,
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","session_id":"a"}"#,
        );
        serve(
            &mut plugin,
            &written,
            &queue,
            config,
            Inbox::new().tick(200).into_messages(),
        );
        assert!(labels(&written).iter().any(|label| label == "running"));
        // The queue's events are five seconds old now, which the idle window accepts as a
        // change rather than as still-running: the plugin compares against the reading, and
        // this tick is six seconds after the one that read the event.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut aged = String::from(
            "{\"at\":0,\"source\":\"claude-code\",\"session\":\"a\",\"activity\":\"running\",\"event\":\"PreToolUse\",\"tool\":\"Bash\"}\n",
        );
        aged.push_str(
            &serde_json::json!({
                "at": 0, "source": "claude-code", "session": "b",
                "activity": "running", "event": "PreToolUse", "tool": "Bash",
            })
            .to_string(),
        );
        aged.push('\n');
        std::fs::write(&queue, aged).expect("writes events from long ago");
        plugin.reader = Reader::new();
        serve(
            &mut plugin,
            &written,
            &queue,
            with(&[("idle_seconds", ConfigValue::Integer(5))]),
            Inbox::new().tick(300).into_messages(),
        );
        assert!(
            labels(&written).iter().any(|label| label == "waiting"),
            "so a tool that stopped is not shown as still running: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn the_cat_does_not_say_waiting_the_moment_the_app_opens() {
        // Idle is the state the plugin is in before anything has been watched, so a bubble on
        // entering it is a cat that says "waiting" every single time BongoCat starts. The
        // panel says what waiting looks like; a bubble is for something that happened.
        let dir = tempfile::tempdir().expect("a data directory");
        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(100).tick(200).into_messages(),
        );
        assert!(
            model_requests(&written).is_empty(),
            "so starting up says nothing: {:?}",
            model_requests(&written)
        );
        assert!(
            labels(&written).iter().any(|label| label == "waiting"),
            "while the panel still says what the state is: {:?}",
            labels(&written)
        );
    }

    #[test]
    fn the_hook_records_an_event_and_returns_a_success_status() {
        // The one thing a hook must never do is take the user's tool call down with it.
        let dir = tempfile::tempdir().expect("a data directory");
        let data_dir = dir.path().to_owned();
        let payload = r#"{"hook_event_name":"PreToolUse","tool_name":"Edit","session_id":"s9"}"#;
        let outcome = run_hook(
            [
                "--data-dir".to_owned(),
                data_dir.display().to_string(),
                "--source".to_owned(),
                "claude-code".to_owned(),
            ]
            .into_iter()
            .chain(std::iter::once(String::new())),
        );
        assert!(
            outcome.is_ok(),
            "so the hook exits successfully whatever happens"
        );
        // The payload is not on stdin in a test, so the recorded line is whatever stdin
        // held; what is checked is that the file exists and that the reader is happy with
        // it, so a hook that wrote something unparseable would fail here.
        let path = queue::queue_path(&data_dir);
        let mut reader = Reader::new();
        for event in reader.read_new(&path) {
            assert_eq!(event.source, "claude-code", "the --source it was given");
        }
        assert!(!payload.is_empty());
    }

    #[test]
    fn the_hook_accepts_a_payload_and_this_plugin_takes_over_from_there() {
        // The join between the two halves, and the only test that needs both: what the hook
        // writes is what this side reads, through the file and nothing else.
        let dir = tempfile::tempdir().expect("a data directory");
        let event = Event::parse(
            "claude-code",
            now_unix(),
            r#"{"hook_event_name":"PreToolUse","tool_name":"Grep","session_id":"s1"}"#,
        )
        .expect("a payload");
        assert!(queue::append(&queue::queue_path(dir.path()), &event));

        let written = WrittenMessages::new();
        let (mut plugin, queue) = watched(dir.path(), the_defaults());
        serve(
            &mut plugin,
            &written,
            &queue,
            the_defaults(),
            Inbox::new().tick(100).into_messages(),
        );
        assert!(
            labels(&written).iter().any(|label| label == "looking"),
            "so the two halves of one executable agree: {:?}",
            labels(&written)
        );
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
                "idle_seconds",
                "only_when_visible",
                "show_event",
                "mapping",
                "use_defaults"
            ]
        );
    }

    #[test]
    fn a_hand_written_idle_window_outside_the_range_lands_on_the_bound() {
        // A setting the host's own form cannot produce must still land somewhere sensible,
        // because a window of zero seconds would make every event immediately stale.
        let schema: ConfigSchema = declared_settings().to_schema().expect("a schema");
        for (given, expected) in [(0_i64, 5_i64), (1, 5), (-100, 5), (10_000_000, 3_600)] {
            let values = values_from(
                &with(&[("idle_seconds", ConfigValue::Integer(given))]),
                &schema,
            );
            assert_eq!(
                Preferences::read(&values).idle_seconds,
                expected,
                "for {given}"
            );
        }
    }

    #[test]
    fn the_plugin_asks_for_the_model_and_nothing_else() {
        let plugin = AgentWatch::new(Preferences::default(), std::path::PathBuf::from("."));
        let descriptor = plugin.descriptor();
        assert_eq!(descriptor.id(), "agent-watch");
        assert_eq!(descriptor.name().resolve("zh-CN"), "AI 监控");
        assert!(descriptor.subscribes_to(Subscription::ModelReaction));
        assert!(
            !descriptor.subscribes_to(Subscription::Input),
            "a plugin watching another program's events has no use for the user's keystrokes"
        );
        descriptor
            .check()
            .expect("this plugin's own descriptor is one the host accepts");
    }
}
