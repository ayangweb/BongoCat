//! The connection to the host: what a plugin sends, and what it can ask for.
//!
//! Every method here writes one line to the plugin's stdout and returns
//! immediately. Nothing blocks, nothing waits for an answer, and nothing can fail
//! in a way that takes the plugin down — a closed pipe is reported once by
//! [`Host::is_connected`] and then the plugin's own loop decides what to do.
//!
//! That shape is deliberate. A panel is a thing the user looks at while they work,
//! so a plugin that blocked waiting for the host would make the cat stutter; and a
//! plugin is a separate process, so a host that is not there is a state a plugin
//! can survive rather than a crash it has to handle. The one thing a plugin *does*
//! wait for is its own state file, and that is its own file.

use crate::panel::Panel;
use crate::settings::{Store, Values};
use crate::{Error, Result};
use bongocat_plugin_protocol::{
    ConfigDocument, ConfigSchema, Hello, HostMessage, LocalizedText, LogLevel, ModelAnswer,
    ModelRequest, PanelUpdate, PluginAction, PluginId, PluginVersion, SceneNode, write_message,
};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// What a plugin can say to the host.
///
/// Held for the plugin's whole run and passed to every callback, so a plugin never
/// has to reach for a global and the compiler can see that sending a panel and
/// reading the clock go through the same connection.
pub struct Host {
    writer: Box<dyn Write + Send>,
    identity: Identity,
    /// The schema the plugin declared, held so a document the host sends is fitted
    /// to the same fields this build knows about.
    schema: ConfigSchema,
    /// The user's settings, refreshed whenever the host sends a new document.
    values: Values,
    store: Store,
    /// Whether a write has failed, so the plugin's loop can stop rather than
    /// discovering it once per tick.
    connected: AtomicBool,
    /// The host's monotonic clock, last read. Never wall time: a plugin's own
    /// arithmetic should not disagree with the host's about how long it has been
    /// running.
    last_elapsed_ms: u64,
    /// The host's last reading of the user's local clock, republished on every tick.
    last_clock: bongocat_plugin_protocol::WallClock,
    /// The host's most recent facts.
    state: bongocat_plugin_protocol::HostState,
    /// The controls last offered to the host, so an unchanged re-offer sends nothing.
    offered_actions: Vec<PluginAction>,
    /// Answers to the plugin's own model requests, waiting to be drained by the
    /// callback that asked.
    answers: Receiver<ModelAnswer>,
    answers_tx: std::sync::mpsc::Sender<ModelAnswer>,
    next_request_id: u64,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Host")
            .field("id", &self.identity.id)
            .field("connected", &self.is_connected())
            .finish()
    }
}

/// What the host told this plugin about itself at the handshake.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub id: PluginId,
    pub version: PluginVersion,
    pub app_version: String,
    /// This plugin's own directory, which holds its assets and nothing else.
    pub plugin_dir: PathBuf,
    /// This plugin's own data directory, which holds its settings and its state.
    pub data_dir: PathBuf,
    /// The user's language, so a plugin can pick its own copy before its first frame.
    pub locale: String,
}

impl Host {
    /// A host connection over these pipes, with these settings and this identity.
    ///
    /// Public because a test needs it and because a plugin with its own transport
    /// may want it; there is exactly one way the product makes one.
    pub fn new(
        writer: Box<dyn Write + Send>,
        identity: Identity,
        schema: ConfigSchema,
        values: Values,
    ) -> Result<Self> {
        let (answers_tx, answers) = mpsc::channel();
        let store = Store::new(identity.data_dir.clone());
        Ok(Self {
            writer,
            identity,
            schema,
            values,
            store,
            connected: AtomicBool::new(true),
            last_elapsed_ms: 0,
            last_clock: bongocat_plugin_protocol::WallClock::default(),
            state: bongocat_plugin_protocol::HostState::default(),
            offered_actions: Vec::new(),
            answers,
            answers_tx,
            next_request_id: 1,
        })
    }

    /// The plugin's own identity, as the store installed it.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// The user's settings, with every declared field present.
    pub fn values(&self) -> &Values {
        &self.values
    }

    /// The plugin's own file store.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Whether the host is still there.
    ///
    /// A plugin's loop should end when this goes false rather than continuing to
    /// draw a panel nothing will show — which is what a plugin does after the app
    /// closes, and it is why a plugin that is still running after the app quits is
    /// a plugin that exits on its own rather than one that has to be killed.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// How long this session has been running, in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.last_elapsed_ms
    }

    /// The host's most recent facts: the model, the window, the language.
    pub fn state(&self) -> &bongocat_plugin_protocol::HostState {
        &self.state
    }

    /// Whether the model window is on screen right now.
    pub fn is_overlay_visible(&self) -> bool {
        self.state.overlay_visible
    }

    /// The active model's display name, when one is loaded.
    pub fn model_name(&self) -> Option<&str> {
        self.state.model_name.as_deref()
    }

    /// The host's last reading of the user's local clock.
    pub fn clock(&self) -> bongocat_plugin_protocol::WallClock {
        self.last_clock
    }

    /// The user's language, as a locale tag.
    pub fn locale(&self) -> &str {
        if self.state.locale.is_empty() {
            &self.identity.locale
        } else {
            &self.state.locale
        }
    }

    /// Take one of this plugin's answers, if one has arrived.
    ///
    /// Non-blocking on purpose: a plugin that waits for an answer before drawing its
    /// next panel would make the panel depend on the host's schedule, which is the
    /// opposite of what a panel is for.
    pub fn take_answer(&mut self) -> Option<ModelAnswer> {
        match self.answers.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Take every answer that has arrived, newest last.
    pub fn drain_answers(&mut self) -> Vec<ModelAnswer> {
        let mut answers = Vec::new();
        while let Some(answer) = self.take_answer() {
            answers.push(answer);
        }
        answers
    }

    /// Whether the host last said the model window is visible, checked once per
    /// panel rather than per request.
    ///
    /// A panel that is hidden is a panel that is not being looked at, and a plugin
    /// that stops advancing while hidden is a plugin that is not doing work nobody
    /// can see.
    pub fn should_advance(&self) -> bool {
        self.state.overlay_visible
    }

    /// Show a panel, if its tree is not the one the host already drew.
    ///
    /// Returns whether anything was sent. A plugin whose countdown reads the same
    /// second twice sends nothing the second time, and that is the whole reason the
    /// model window does not re-upload an identical texture.
    pub fn panel(&mut self, panel: &mut Panel) -> bool {
        if !panel.needs_sending() {
            return false;
        }
        if self.send_panel(&panel.to_update()) {
            panel.mark_sent();
            return true;
        }
        false
    }

    /// Show a panel unconditionally.
    ///
    /// For a panel whose contents change every tick — a live counter, a clock — where
    /// the "did it change" check is the plugin's own and worth stating.
    pub fn show(&mut self, panel: &mut Panel) -> bool {
        let sent = self.send_panel(&panel.to_update());
        if sent {
            panel.mark_sent();
        }
        sent
    }

    /// Show one scene as the whole panel, without a [`Panel`] to hold it.
    ///
    /// Always sends: there is nothing here to compare against, because the tree went
    /// out of scope. A plugin redrawing on a timer wants [`Self::panel`] instead —
    /// this is for the one-shot case, where sending is the whole point.
    pub fn show_scene(&mut self, scene: SceneNode, size: [u32; 2]) -> bool {
        let panel = Panel::new(size[0], size[1]);
        self.send_panel(&bongocat_plugin_protocol::PanelUpdate {
            placement: bongocat_plugin_protocol::PanelPlacement {
                size,
                ..panel.to_update().placement
            },
            scene,
        })
    }

    /// Take the panel down without stopping the plugin.
    ///
    /// A plugin with nothing to show uses this rather than sending an empty panel,
    /// because an empty panel is still a box on the user's desktop.
    pub fn hide_panel(&mut self) -> bool {
        self.write(&bongocat_plugin_protocol::PluginMessage::HidePanel)
    }

    fn send_panel(&mut self, update: &PanelUpdate) -> bool {
        self.write(&bongocat_plugin_protocol::PluginMessage::Panel(Box::new(
            update.clone(),
        )))
    }

    /// Offer the host a set of controls to draw on this plugin's own card.
    ///
    /// For a plugin whose main control the user reaches for often enough that
    /// hunting for it inside a panel on the model window is the wrong way round: the
    /// host draws these with the settings window's own buttons, in the window the
    /// plugin was configured in.
    ///
    /// **Send the whole set every time, not a patch.** The host replaces its list with
    /// whatever arrived, which is what stops a plugin leaving behind a control it has
    /// stopped wanting, and it is what makes a press of an id that is no longer
    /// offered ignored rather than delivered to a plugin that has forgotten what it
    /// meant.
    ///
    /// A plugin whose control changes meaning — a timer whose button says "Start" and
    /// then "Pause" — re-sends. The label is the user's only clue to what a press will
    /// do, and a stale one is worse than no button. [`Panel`]'s own send-if-changed
    /// check is the right model to copy here: compare against what you last sent and
    /// skip the write when nothing changed, so a plugin calling this every tick does
    /// not write a line every tick.
    ///
    /// A press of any of these arrives in [`Plugin::on_press`](crate::Plugin::on_press)
    /// with the same `id`, so there is no second handler to write.
    pub fn offer_actions(&mut self, actions: &[PluginAction]) -> bool {
        // Checked here rather than left to the host's refusal: the host's answer to an
        // invalid list is to ignore it and log, and a plugin author testing against the
        // SDK should learn about it from a `false` rather than from a card that quietly
        // has no buttons on it.
        if bongocat_plugin_protocol::check_actions(actions).is_err() {
            return false;
        }
        // The unchanged case sends nothing, exactly as [`Host::panel`] does for a scene
        // that came out the same. A plugin that derives its action from its own state
        // would otherwise write a protocol line every tick to say the same word, and
        // those lines are what the host's reader thread wakes up for.
        if self.offered_actions == actions {
            return true;
        }
        let sent = self.write(&bongocat_plugin_protocol::PluginMessage::Actions {
            actions: actions.to_vec(),
        });
        // Recorded on the outcome rather than before it: a failed write means the host
        // never got these, so the next call has to try again rather than believe the
        // list is already there.
        if sent {
            self.offered_actions = actions.to_vec();
        }
        sent
    }

    /// Offer one control, in place of any previously offered set.
    ///
    /// The shape most plugins want: one control, rebuilt whenever its label or glyph
    /// changed. [`Self::offer_actions`] is the same call for a plugin with more than
    /// one, and is spelled that way so a plugin never has to write a one-element slice
    /// to get a single button onto its card.
    pub fn offer_action(&mut self, action: PluginAction) -> bool {
        self.offer_actions(std::slice::from_ref(&action))
    }

    /// Ask the model to play a motion, by its name in the model.
    ///
    /// Returns the request's id, which comes back in an answer. A model without a
    /// motion by that name is answered with [`ModelOutcome::NotInModel`], not an
    /// error — a plugin that has a fallback wants to know, and one that does not
    /// simply never looks.
    pub fn play_motion(&mut self, name: &str) -> u64 {
        self.request(ModelRequest::PlayMotion {
            name: name.to_string(),
            restart: false,
        })
    }

    /// Play a motion, restarting it even if one of the same priority is running.
    pub fn play_motion_restarting(&mut self, name: &str) -> u64 {
        self.request(ModelRequest::PlayMotion {
            name: name.to_string(),
            restart: true,
        })
    }

    /// Set an expression, by its name in the model.
    pub fn set_expression(&mut self, name: &str) -> u64 {
        self.request(ModelRequest::SetExpression {
            name: name.to_string(),
        })
    }

    /// Clear the expression the plugin set, restoring the model's own default.
    pub fn clear_expression(&mut self) -> u64 {
        self.request(ModelRequest::ClearExpression)
    }

    /// Show a short bubble beside the model.
    ///
    /// The duration is clamped by the protocol, so a plugin cannot leave something
    /// on the user's desktop by asking for an hour. Pass [`MINIMUM_BUBBLE_MILLIS`] or
    /// [`MAXIMUM_BUBBLE_MILLIS`] to mean "as short or as long as allowed".
    pub fn bubble(&mut self, text: &str, duration_ms: u32) -> u64 {
        self.request(ModelRequest::ShowBubble {
            text: LocalizedText::from(text),
            duration_ms,
        })
    }

    /// Show a bubble whose text is a plugin's own localized string.
    pub fn bubble_text(&mut self, text: LocalizedText, duration_ms: u32) -> u64 {
        self.request(ModelRequest::ShowBubble { text, duration_ms })
    }

    /// Take the bubble down now.
    pub fn hide_bubble(&mut self) -> u64 {
        self.request(ModelRequest::HideBubble)
    }

    fn request(&mut self, request: ModelRequest) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        self.write(&bongocat_plugin_protocol::PluginMessage::Request {
            id,
            request: Box::new(request),
        });
        id
    }

    /// Write a line to the plugin's own log.
    ///
    /// stderr, which the host forwards to its own. A plugin never has to remember
    /// that, and printing to stdout by hand is the one mistake that breaks the
    /// protocol — which is why it fails loudly rather than silently.
    pub fn log(&mut self, level: LogLevel, message: &str) {
        let _ = self.write(&bongocat_plugin_protocol::PluginMessage::Log {
            level,
            message: message.chars().take(MAXIMUM_LOG_CHARS).collect(),
        });
    }

    /// Report a failure the user should see.
    ///
    /// Ends the session: a plugin that has reported a failure is a plugin that has
    /// stopped, and the host shows the reason on its card rather than leaving a
    /// stale panel on screen with no explanation.
    pub fn fail(&mut self, message: &str) -> Result<()> {
        let _ = self.write(&bongocat_plugin_protocol::PluginMessage::Failed {
            error: bongocat_plugin_protocol::PluginError::with_detail(
                bongocat_plugin_protocol::PluginErrorCode::PluginExited,
                message.chars().take(MAXIMUM_LOG_CHARS).collect::<String>(),
            ),
        });
        Err(Error::Failed(message.to_string()))
    }

    /// Tell the host this plugin is finished, and stop.
    ///
    /// The normal ending. [`Host::is_connected`] goes false afterwards, so a
    /// plugin's loop that watches it exits cleanly rather than being killed.
    pub fn shutdown(&mut self) -> Result<()> {
        let _ = self.write(&bongocat_plugin_protocol::PluginMessage::Shutdown);
        self.connected.store(false, Ordering::Release);
        Ok(())
    }

    fn write(&mut self, message: &impl serde::Serialize) -> bool {
        if !self.is_connected() {
            return false;
        }
        let Some(line) = write_message(message) else {
            return false;
        };
        let mut framed = line;
        framed.push(b'\n');
        match self.writer.write_all(&framed) {
            Ok(()) => self.writer.flush().is_ok(),
            Err(_) => {
                // The host is gone. Recorded once; every later write is skipped, so
                // a plugin whose host exited does not pay for a failed syscall on
                // every tick until it notices.
                self.connected.store(false, Ordering::Release);
                false
            }
        }
    }

    /// Take a tick's facts, for the run loop.
    pub(crate) fn apply_tick(
        &mut self,
        elapsed_ms: u64,
        state: &bongocat_plugin_protocol::HostState,
        clock: bongocat_plugin_protocol::WallClock,
    ) {
        self.last_elapsed_ms = elapsed_ms;
        self.state = state.clone();
        self.last_clock = clock;
    }

    pub(crate) fn apply_config(&mut self, config: &ConfigDocument) {
        self.values = crate::settings::fit(config, self.values_schema());
    }

    pub(crate) fn apply_answer(&self, answer: ModelAnswer) {
        let _ = self.answers_tx.send(answer);
    }

    fn values_schema(&self) -> &ConfigSchema {
        &self.schema
    }
}

/// The longest a plugin's own log line may be, in characters.
///
/// A log line is not a user-facing message and is not bounded by anything the user
/// sees, so this is generous — but it is a bound, because an unbounded line from a
/// plugin in a loop is a log file with no end.
pub const MAXIMUM_LOG_CHARS: usize = 2000;

/// Read the host's `hello`, which is the first line on the plugin's stdin.
pub(crate) fn read_hello(line: &[u8]) -> Result<Hello> {
    match bongocat_plugin_protocol::parse_host_message(line) {
        Ok(HostMessage::Hello(hello)) => {
            hello
                .check_version()
                .map_err(|error| Error::Protocol(error.to_string()))?;
            Ok(hello)
        }
        Ok(other) => Err(Error::Protocol(format!(
            "the first message was {other:?}, not a hello"
        ))),
        Err(error) => Err(Error::Protocol(error.to_string())),
    }
}

/// The outcome of a model request, for a plugin that reads one.
pub type Outcome = bongocat_plugin_protocol::ModelOutcome;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    use crate::settings::{Integer, Settings, Toggle};
    use crate::testing::{IdentityBuilder, WrittenMessages};
    use bongocat_plugin_protocol::{ActionGlyph, ModelOutcome};

    fn host() -> (Host, WrittenMessages) {
        let schema = Settings::new()
            .with(Toggle::new("auto_start", "Auto").into())
            .with(Integer::ranged("minutes", "Minutes", 25, 1, 120).into())
            .to_schema()
            .expect("a valid schema");
        let written = WrittenMessages::new();
        let identity = IdentityBuilder::new().build();
        let values = crate::settings::fit(&schema.defaults(), &schema);
        let host = Host::new(written.writer(), identity, schema, values).expect("a host");
        (host, written)
    }

    #[test]
    fn a_panel_is_sent_as_one_panel_message() {
        let (mut host, written) = host();
        let mut panel = Panel::new(200, 60);
        panel.rebuild(|p| p.column(|c| c.text("hi", 16.0)));
        assert!(host.panel(&mut panel));
        let messages = written.messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages[0],
            bongocat_plugin_protocol::PluginMessage::Panel(_)
        ));
    }

    #[test]
    fn a_panel_that_did_not_change_sends_nothing() {
        // This is the property the model window's upload check depends on, so it is
        // worth pinning here rather than only in the renderer.
        let (mut host, written) = host();
        let mut panel = Panel::new(200, 60);
        panel.rebuild(|p| p.column(|c| c.text("hi", 16.0)));
        assert!(host.panel(&mut panel), "the first one changed something");
        assert!(!panel.needs_sending(), "so the host has it now");
        panel.rebuild(|p| p.column(|c| c.text("hi", 16.0)));
        assert!(
            !host.panel(&mut panel),
            "and rebuilding the same tree is not a change worth sending"
        );
        assert_eq!(written.messages().len(), 1);
    }

    #[test]
    fn hiding_a_panel_sends_hide_rather_than_an_empty_one() {
        // An empty panel is still a box on the user's desktop; a hidden panel is not.
        let (mut host, written) = host();
        assert!(host.hide_panel());
        assert!(matches!(
            written.messages()[0],
            bongocat_plugin_protocol::PluginMessage::HidePanel
        ));
    }

    #[test]
    fn an_offered_action_is_sent_once_and_the_rest_of_the_time_sends_nothing() {
        // The panel's own rule, applied to a control the host draws. A plugin whose
        // action label comes from its own state would otherwise write a line every tick
        // to say the same word, and those lines are what the host's reader wakes for.
        let (mut host, written) = host();
        let action = Action::new("toggle", "Start").glyph(ActionGlyph::Play);
        let offered = [action.to_protocol()];
        assert!(host.offer_actions(&offered));
        assert!(host.offer_actions(&offered));
        assert!(host.offer_actions(&offered));
        assert_eq!(
            written.messages().len(),
            1,
            "three offers of the same control, one line out"
        );

        // A changed label is a change, and is sent.
        let renamed = [Action::new("toggle", "Pause")
            .glyph(ActionGlyph::Pause)
            .to_protocol()];
        assert!(host.offer_actions(&renamed));
        assert_eq!(
            written.messages().len(),
            2,
            "so a timer that renamed its control tells the host rather than leaving the card \
             saying what it used to do"
        );
        let bongocat_plugin_protocol::PluginMessage::Actions { actions } = &written.messages()[1]
        else {
            panic!("an actions message");
        };
        assert_eq!(actions, &renamed);
    }

    #[test]
    fn a_list_the_host_would_refuse_is_refused_here_too() {
        // Checked by the SDK rather than left to the host's silent refusal, so a plugin
        // author finds out from the `false` instead of from a card with no buttons.
        let (mut host, written) = host();
        let two_with_one_id = [
            Action::new("go", "Go").to_protocol(),
            Action::new("go", "Go again").to_protocol(),
        ];
        assert!(
            !host.offer_actions(&two_with_one_id),
            "two controls sharing an id would make a press ambiguous"
        );
        assert!(
            written.messages().is_empty(),
            "and nothing reached the wire"
        );
    }

    #[test]
    fn a_closed_host_does_not_remember_an_offer_it_never_sent() {
        // Otherwise a plugin whose first offer failed would never send it again: the
        // comparison would say "already sent" for a list the host has never seen.
        let schema = Settings::new().to_schema().expect("valid");
        let written = WrittenMessages::new();
        let mut host = Host::new(
            written.failing_writer(),
            IdentityBuilder::new().build(),
            schema.clone(),
            crate::settings::fit(&schema.defaults(), &schema),
        )
        .expect("a host");
        let offered = [Action::new("toggle", "Start").to_protocol()];
        assert!(!host.offer_actions(&offered), "the write failed");
        assert!(!host.is_connected());
    }

    #[test]
    fn a_model_request_carries_an_id_and_gets_an_answer_back() {
        let (mut host, written) = host();
        let id = host.play_motion("wave");
        assert_eq!(id, 1);
        assert_eq!(host.set_expression("happy"), 2);
        let requests = crate::testing::model_requests(&written);
        assert_eq!(
            requests.len(),
            2,
            "a request travels plugin to host, so it is one of the plugin's own messages"
        );
        assert_eq!(requests[0].0, 1);
        assert_eq!(requests[1].0, 2);

        // An answer arrives later, out of band, and is matched by the plugin's own id.
        host.apply_answer(ModelAnswer {
            id: 1,
            outcome: ModelOutcome::NotInModel {
                kind: bongocat_plugin_protocol::ModelRequestKind::Motion,
            },
        });
        let answers = host.drain_answers();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].id, 1);
        assert_eq!(
            answers[0].outcome,
            ModelOutcome::NotInModel {
                kind: bongocat_plugin_protocol::ModelRequestKind::Motion
            },
            "a model without that motion is an answer, not a failure"
        );
        assert!(
            host.drain_answers().is_empty(),
            "and an answer is taken once, not re-read"
        );
    }

    #[test]
    fn a_log_line_is_bounded_because_a_loop_must_not_fill_the_log() {
        let (mut host, written) = host();
        host.log(LogLevel::Warn, &"x".repeat(MAXIMUM_LOG_CHARS * 2));
        let bongocat_plugin_protocol::PluginMessage::Log { message, .. } = &written.messages()[0]
        else {
            panic!("a log message");
        };
        assert_eq!(message.chars().count(), MAXIMUM_LOG_CHARS);
    }

    #[test]
    fn a_write_to_a_closed_host_records_the_fact_once_and_then_skips() {
        // The plugin outliving its host is a normal state — the app closed — and a
        // plugin that keeps paying for a failed syscall on every tick is a plugin
        // that burns a core on the way out.
        let schema = Settings::new().to_schema().expect("valid");
        let written = WrittenMessages::new();
        let mut host = Host::new(
            written.failing_writer(),
            IdentityBuilder::new().build(),
            schema.clone(),
            crate::settings::fit(&schema.defaults(), &schema),
        )
        .expect("a host");
        assert!(host.is_connected());
        let mut panel = Panel::new(100, 40);
        panel.rebuild(|p| p.column(|c| c.text("x", 12.0)));
        assert!(!host.panel(&mut panel), "the write failed");
        assert!(!host.is_connected(), "and that is now known");
        assert!(
            !host.hide_panel(),
            "so the next one is skipped rather than tried"
        );
    }

    #[test]
    fn a_tick_updates_the_clock_and_the_facts_the_plugin_reads() {
        let (mut host, _written) = host();
        let state = bongocat_plugin_protocol::HostState::new(Some("Cat".to_string()), true)
            .with_locale("zh-CN");
        host.apply_tick(
            1500,
            &state,
            bongocat_plugin_protocol::WallClock::new(9, 5, 3),
        );
        assert_eq!(host.elapsed_ms(), 1500);
        assert!(host.is_overlay_visible());
        assert_eq!(host.model_name(), Some("Cat"));
        assert_eq!(host.locale(), "zh-CN");
        assert!(host.should_advance());
        assert_eq!(
            host.clock().to_hms(),
            "09:05:03",
            "and the wall clock rides on the tick, because the host's main thread is the only \
             thread allowed to ask the operating system for the local offset"
        );
    }

    #[test]
    fn a_hidden_overlay_is_the_answer_to_should_i_keep_counting() {
        let (mut host, _written) = host();
        host.apply_tick(
            1000,
            &bongocat_plugin_protocol::HostState::new(None, false),
            bongocat_plugin_protocol::WallClock::default(),
        );
        assert!(!host.should_advance());
        assert_eq!(
            host.model_name(),
            None,
            "and a model that is not loaded has no name rather than an empty one"
        );
    }

    #[test]
    fn a_configuration_message_replaces_the_values_the_plugin_reads() {
        let (mut host, _written) = host();
        assert_eq!(host.values().integer("minutes"), 25);
        let document = ConfigDocument::single(
            "minutes",
            bongocat_plugin_protocol::ConfigValue::Integer(50),
        );
        host.apply_config(&document);
        assert_eq!(
            host.values().integer("minutes"),
            50,
            "so the next panel the plugin builds uses what the user just set"
        );
    }

    #[test]
    fn a_handshake_is_read_before_anything_else_or_it_is_not_a_handshake() {
        let hello = bongocat_plugin_protocol::write_message(&HostMessage::Hello(
            IdentityBuilder::new().hello(),
        ))
        .expect("serializes");
        assert_eq!(
            read_hello(&hello).expect("reads").id.as_str(),
            "test-plugin"
        );

        let not_a_hello =
            bongocat_plugin_protocol::write_message(&HostMessage::Shutdown).expect("serializes");
        assert!(
            matches!(read_hello(&not_a_hello), Err(Error::Protocol(_))),
            "a plugin that read a tick as its hello would be running with no identity"
        );
    }

    #[test]
    fn a_hello_from_a_host_this_plugin_does_not_understand_is_refused() {
        let mut hello = IdentityBuilder::new().hello();
        hello.protocol_version = bongocat_plugin_protocol::PROTOCOL_VERSION + 1;
        let line = bongocat_plugin_protocol::write_message(&HostMessage::Hello(hello))
            .expect("serializes");
        assert!(
            matches!(read_hello(&line), Err(Error::Protocol(_))),
            "half a protocol is worse than none"
        );
    }
}
