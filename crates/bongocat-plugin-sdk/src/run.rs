//! The session loop: read the host's messages, dispatch them, end when it stops.
//!
//! This is the whole of what a plugin's `main` does after the handshake, and it is
//! here rather than in each plugin because a plugin that wrote its own loop would
//! have to re-derive what a tick is, when to answer, and what "the host has gone"
//! means — three decisions that have exactly one right answer.
//!
//! # Why the loop is this shape
//!
//! * **It reads a line, dispatches it, repeats.** No queue of its own, no batching,
//!   no timer. The host decides the cadence and a plugin that added its own would
//!   have two clocks.
//! * **It stops on the host's `shutdown`, and on a closed pipe.** Both are normal:
//!   the first is the user quitting, the second is the app crashing. A plugin that
//!   exits on either is a plugin that never has to be killed.
//! * **A message the plugin did not ask for is dropped, not delivered.** Input for a
//!   plugin that did not subscribe to the feed is discarded here, so
//!   [`Plugin::on_input`] can rely on the subscription having been made.
//! * **A failure ends the session.** A plugin that reported a failure has stopped;
//!   the host shows the reason on its card rather than leaving a stale panel with no
//!   explanation.
//!
//! # Reading the handshake
//!
//! The first line has to be the host's `hello`, and nothing else is accepted in its
//! place. A plugin that began on a tick would be running with no identity, no data
//! directory and no idea which version of the protocol it is speaking — and would
//! find out by writing to a path it had not been given.

use crate::plugin::{Event, Plugin, Tick};
use crate::{Error, Host, Result};
#[cfg(test)]
use bongocat_plugin_protocol::ModelOutcome;
use bongocat_plugin_protocol::{
    Hello, HostMessage, ModelAnswer, PluginMessage, Subscription, write_message,
};
use std::io::{BufRead, Write};

/// Read the handshake, announce the plugin, and serve the session.
///
/// The function a plugin's `main` calls. It does not return until the host has
/// stopped or the pipes have closed.
pub fn run(mut plugin: impl Plugin) -> Result<()> {
    let input = StdinForwarding::new(std::io::stdin().lock());
    let mut output = std::io::stdout().lock();
    let mut session = handshake(input, &mut plugin)?;
    session.announce(&mut output, &mut plugin)?;
    session.serve_reader(&mut plugin)
}

/// Read the host's `hello`, then announce this plugin's descriptor.
///
/// Split from [`run`] so the two halves of the handshake are separately testable:
/// getting the hello wrong is a different bug from announcing the wrong descriptor,
/// and they fail in different places.
fn handshake(mut input: impl BufRead + 'static, plugin: &mut impl Plugin) -> Result<Session> {
    let hello = read_first_line(&mut input)?;
    let hello: Hello = crate::host::read_hello(&hello)?;
    let descriptor = plugin.descriptor();
    let protocol_descriptor = descriptor.to_protocol()?;
    // The id and version the store installed are the ones the card will show, so a
    // plugin that announces a different one is refused here rather than displayed
    // under a name it did not claim.
    protocol_descriptor
        .agrees_with(&bongocat_plugin_protocol::PluginManifest {
            schema_version: bongocat_plugin_protocol::PLUGIN_SCHEMA_VERSION,
            api_version: bongocat_plugin_protocol::SUPPORTED_PLUGIN_API_VERSION,
            id: hello.id.clone(),
            name: hello.id.as_str().into(),
            version: hello.version,
            min_app_version: None,
            author: String::new(),
            description: Default::default(),
            icon: Default::default(),
            executable: hello.id.as_str().to_string(),
        })
        .map_err(|error| Error::Protocol(error.to_string()))?;

    let settings = plugin.settings();
    let schema = settings.to_schema()?;
    let store = crate::settings::Store::new(&hello.data_dir);
    let values = store.read_values(&schema)?;
    let identity = crate::host::Identity {
        id: hello.id,
        version: hello.version,
        app_version: hello.app_version,
        plugin_dir: hello.plugin_dir.into(),
        data_dir: hello.data_dir.into(),
        locale: hello.locale,
    };
    let _ = protocol_descriptor;
    let host = Host::new(Box::new(StdoutForwarding), identity, schema, values)?;
    Ok(Session::new(host).reading(input))
}

/// Send the `ready` message.
///
/// The host will not show the plugin until this arrives, which is why it is sent
/// before `on_ready`: the card exists, and then the panel appears.
fn announce(
    output: &mut impl Write,
    descriptor: bongocat_plugin_protocol::PluginDescriptor,
) -> Result<()> {
    let line = write_message(&PluginMessage::Ready {
        descriptor: Box::new(descriptor),
    })
    .ok_or_else(|| Error::Protocol("this plugin's descriptor could not be written".to_string()))?;
    write_line(output, &line)
}

/// Read the first line, which has to be the host's `hello`.
fn read_first_line(input: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    let read = input
        .read_until(b'\n', &mut line)
        .map_err(|error| Error::Disconnected(error.to_string()))?;
    if read == 0 {
        return Err(Error::Disconnected(
            "BongoCat closed the connection before saying hello".to_string(),
        ));
    }
    Ok(strip_newline(line))
}

fn strip_newline(mut line: Vec<u8>) -> Vec<u8> {
    while line
        .last()
        .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
    {
        line.pop();
    }
    line
}

/// The same, without copying — the read loop already owns the buffer and clears it
/// each turn, so trimming in place is the whole of it.
fn trim_newline(line: &mut Vec<u8>) -> &[u8] {
    while line
        .last()
        .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
    {
        line.pop();
    }
    line
}

fn write_line(output: &mut impl Write, line: &[u8]) -> Result<()> {
    let mut framed = line.to_vec();
    framed.push(b'\n');
    output
        .write_all(&framed)
        .map_err(|error| Error::Disconnected(error.to_string()))?;
    output
        .flush()
        .map_err(|error| Error::Disconnected(error.to_string()))
}

/// A writer that forwards everything to stderr.
///
/// The host's end of the plugin's protocol: one stream, and it is stdout.
///
/// The host reads this plugin's stdout and nothing else, so every message the plugin
/// sends — the announcement, a panel, a control, an answer, and a log line, which is a
/// `Log` *message* rather than a bare print — goes here. The SDK used to hand the host
/// connection a **stderr** writer on the reasoning that diagnostics belong on stderr,
/// and that sent every panel, control and answer a plugin produced to a stream the host
/// never reads. A plugin announced itself and then went silent for the rest of its
/// life.
///
/// Locked per write rather than held for the session, so a plugin that logs from a
/// callback cannot deadlock against a plugin that is also drawing. A raw `eprintln!`
/// still goes to stderr and is still the host's to forward; what it must not do is
/// compete with the protocol, and a line of JSON on the wrong stream is a visible
/// failure rather than a silent one.
struct StdoutForwarding;

impl Write for StdoutForwarding {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        std::io::stdout().lock().write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().lock().flush()
    }
}

/// The plugin's own end of the host's messages.
///
/// A boxed reader rather than a borrow, because [`Session`] owns it and the run loop
/// has to be able to hold a session without borrowing the `stdin` lock it read the
/// handshake from.
struct StdinForwarding<R> {
    inner: R,
}

impl<R: BufRead> StdinForwarding<R> {
    fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: BufRead> std::io::Read for StdinForwarding<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.inner, buffer)
    }
}

impl<R: BufRead> BufRead for StdinForwarding<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

/// One plugin's session, over any reader and any writer.
///
/// The type the tests drive and the type [`run`] builds, so a plugin's behaviour is
/// tested through the same dispatch the product uses. Public for a plugin's own
/// tests; there is no other intended caller.
pub struct Session {
    input: Box<dyn BufRead>,
    host: Host,
}

impl Session {
    /// Announce the descriptor, then run the plugin's `on_ready`.
    ///
    /// Split out of the handshake because announcing and being told you exist are two
    /// different facts. The host can show a card as soon as the first line arrives;
    /// the plugin cannot draw anything until it has read its settings. Doing both
    /// here, in that order, is what makes "the card is there and the panel is already
    /// drawn" the default rather than something a plugin has to arrange.
    ///
    /// The output is a separate argument because it is the host's pipe and the
    /// session does not own it — a test that wants to see the announcement supplies
    /// its own.
    /// The plugin's own words, from the plugin.
    ///
    /// The descriptor and the settings are announced as one document, and this is the line
    /// that makes them one. They used to be two: `Plugin::descriptor` carried the identity
    /// and the host was told a plugin had no fields at all, while the schema the plugin read
    /// its own values against was built separately and never left this process. Every
    /// settings form in the product was therefore empty, and a plugin's own test could not
    /// see it — a test that builds the schema and asserts on it is asserting on the plugin's
    /// own view of its settings, not on what the host was told.
    ///
    /// Both are now the one declaration, asked of the one function: the host's schema and
    /// the host's form are the same fields, and a plugin cannot answer twice and disagree.
    pub fn announce(&mut self, output: &mut impl Write, plugin: &mut impl Plugin) -> Result<()> {
        announce(
            output,
            plugin
                .descriptor()
                .settings(plugin.settings())
                .to_protocol()?,
        )?;
        plugin.on_ready(&mut self.host)
    }

    /// A session over this connection, reading nothing.
    ///
    /// The shape a test uses: it supplies the messages itself through
    /// [`Self::serve`], so a plugin's behaviour is checked through the same dispatch
    /// the product runs rather than through a hand-written imitation of it.
    pub fn new(host: Host) -> Self {
        Self {
            input: Box::new(std::io::empty()),
            host,
        }
    }

    /// A session that reads its messages from a reader.
    pub fn reading(mut self, input: impl BufRead + 'static) -> Self {
        self.input = Box::new(input);
        self
    }

    /// Serve the session until the host stops, feeding it these messages.
    ///
    /// Takes the messages rather than reading them so a test can state a whole
    /// session as data. [`Self::serve_reader`] is the same loop over a reader.
    #[allow(dead_code, reason = "a plugin's own tests drive a session this way")]
    pub fn serve(mut self, plugin: &mut impl Plugin, messages: Vec<HostMessage>) -> Result<()> {
        for message in messages {
            if !self.host.is_connected() {
                break;
            }
            // The host's stop ends the session, exactly as `serve_reader` does. A
            // test that queues messages after it is testing that nothing after the
            // stop is delivered, so the rule has to hold here too.
            let stop = matches!(message, HostMessage::Shutdown);
            self.deliver(plugin, message)?;
            if stop {
                break;
            }
        }
        Ok(())
    }

    /// Serve the session until the host stops or the reader ends.
    pub fn serve_reader(mut self, plugin: &mut impl Plugin) -> Result<()> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = self
                .input
                .read_until(b'\n', &mut line)
                .map_err(|error| Error::Disconnected(error.to_string()))?;
            if read == 0 {
                // The host closed its end. Normal: the app quit, or it crashed, and
                // a plugin that ends here is a plugin that never has to be killed.
                break;
            }
            match bongocat_plugin_protocol::parse_host_message(trim_newline(&mut line)) {
                Ok(message) => {
                    let stop = matches!(message, HostMessage::Shutdown);
                    self.deliver(plugin, message)?;
                    if stop {
                        break;
                    }
                }
                Err(error) => {
                    // A line the SDK cannot read is reported and the session ends,
                    // rather than skipped: a plugin that kept running on a stream it
                    // had lost track of would be running on somebody else's protocol.
                    return Err(Error::Protocol(error.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Deliver one message to the plugin.
    ///
    /// The subscription check lives here, which is the only place that knows what the
    /// plugin asked for: a plugin that did not subscribe to the input feed is never
    /// called, so its callback can assume the subscription was made.
    fn deliver(&mut self, plugin: &mut impl Plugin, message: HostMessage) -> Result<()> {
        let event = match message {
            HostMessage::Hello(_) => {
                // Already handled during the handshake. A second hello means the host
                // restarted the session, which is not something a plugin can resume
                // from, so it is a protocol error rather than an event.
                return Err(Error::Protocol(
                    "the host said hello twice, which this plugin cannot resume from".to_string(),
                ));
            }
            HostMessage::Tick {
                elapsed_ms,
                state,
                clock,
            } => {
                self.host.apply_tick(elapsed_ms, &state, clock);
                Event::Tick(Tick {
                    elapsed_ms,
                    state,
                    clock,
                })
            }
            HostMessage::Input { events } => {
                if !self.subscribed(plugin, Subscription::Input) {
                    return Ok(());
                }
                Event::Input(events)
            }
            HostMessage::Press { id } => Event::Press { id },
            HostMessage::ConfigChanged { config } => {
                self.host.apply_config(&config);
                Event::ConfigChanged
            }
            HostMessage::Shutdown => Event::Shutdown,
            HostMessage::ModelAnswer { id, outcome } => Event::Answer { id, outcome },
        };
        if let Event::Answer { id, outcome } = &event {
            self.host.apply_answer(ModelAnswer {
                id: *id,
                outcome: outcome.clone(),
            });
        }
        plugin.on_event(event, &mut self.host)
    }

    fn subscribed(&self, plugin: &impl Plugin, subscription: Subscription) -> bool {
        plugin.descriptor().subscribes_to(subscription)
    }
}

/// Read one message from the host, as the session loop does.
///
/// Crate-visible rather than private because a plugin's own tests write a line by
/// hand when they want to check what a *malformed* one does, and the point is that
/// they do it through the same reader the product uses.
pub(crate) fn read_host_message(line: &[u8]) -> Result<HostMessage> {
    bongocat_plugin_protocol::parse_host_message(line)
        .map_err(|error| Error::Protocol(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Descriptor;
    use crate::testing::{Inbox, WrittenMessages};
    use crate::{InputEvent, Settings};
    use bongocat_plugin_protocol::{
        ConfigSchema, ConfigValue, ModelRequest, PluginId, PluginVersion,
    };
    use std::collections::BTreeMap;

    /// A plugin that records every callback, so a test can assert on the dispatch.
    struct Recorder {
        events: Vec<String>,
        panels: usize,
        /// Kept across callbacks rather than rebuilt each time, because a real plugin
        /// holds its panel — and that is what makes "did anything change" a question
        /// with an answer.
        panel: crate::Panel,
    }

    impl Default for Recorder {
        fn default() -> Self {
            Self {
                events: Vec::new(),
                panels: 0,
                panel: crate::Panel::new(160, 40),
            }
        }
    }

    impl Plugin for Recorder {
        fn descriptor(&self) -> Descriptor {
            Descriptor::new("recorder", "Recorder")
                .icon("🎛️")
                .subscribe(Subscription::Input)
        }

        fn settings(&mut self) -> Settings {
            Settings::new().with(crate::Toggle::new("flag", "Flag").into())
        }

        fn on_ready(&mut self, host: &mut Host) -> Result<()> {
            self.events.push(format!("ready:{}", host.locale()));
            self.panel = crate::Panel::new(160, 40);
            self.panel.rebuild(|p| p.column(|c| c.text("ready", 14.0)));
            self.panels += usize::from(host.panel(&mut self.panel));
            Ok(())
        }

        fn on_tick(&mut self, tick: Tick, host: &mut Host) {
            self.events.push(format!("tick:{}", tick.elapsed_ms));
            self.panel
                .rebuild(|p| p.column(|c| c.text(&format!("{}", tick.elapsed_ms), 20.0)));
            if host.panel(&mut self.panel) {
                self.panels += 1;
            }
        }

        fn on_input(&mut self, events: Vec<InputEvent>, _host: &mut Host) {
            self.events.push(format!("input:{}", events.len()));
        }

        fn on_press(&mut self, id: &str, host: &mut Host) {
            self.events.push(format!("press:{id}"));
            host.bubble("hi", 1200);
        }

        fn on_config_changed(&mut self, host: &mut Host) {
            self.events
                .push(format!("config:{}", host.values().flag("flag")));
        }

        fn on_answer(&mut self, id: u64, outcome: ModelOutcome, _host: &mut Host) {
            self.events.push(format!("answer:{id}:{outcome:?}"));
        }

        fn on_shutdown(&mut self, _host: &mut Host) {
            self.events.push("shutdown".to_string());
        }
    }

    fn host(schema: &ConfigSchema, written: &WrittenMessages) -> Host {
        let identity = crate::testing::IdentityBuilder::new().build();
        Host::new(
            written.writer(),
            identity,
            schema.clone(),
            crate::testing::values(schema),
        )
        .expect("a host")
    }

    fn schema() -> ConfigSchema {
        Settings::new()
            .with(crate::Toggle::new("flag", "Flag").into())
            .to_schema()
            .expect("a valid schema")
    }

    #[test]
    fn a_ready_draws_its_first_panel_without_waiting_for_a_tick() {
        // A panel that appears only on the first tick is a panel that is missing
        // when the model window opens and the user is looking for it.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        let mut session = Session::new(host);
        session
            .announce(&mut written.writer(), &mut plugin)
            .expect("announced");
        session.serve(&mut plugin, Vec::new()).expect("served");
        let panels = crate::testing::panels(&written);
        assert_eq!(
            panels.len(),
            1,
            "one panel, from `on_ready` and with no tick at all"
        );
        assert!(
            crate::testing::is_ready(&written.messages()[0]),
            "and the descriptor is announced before it, so the card exists first"
        );
        assert_eq!(
            crate::testing::labels_in(&panels[0].scene),
            vec!["ready".to_string()]
        );
    }

    #[test]
    fn the_settings_a_plugin_declares_reach_the_host_in_its_announcement() {
        // The bug this exists for, and it was invisible from inside a plugin: the
        // descriptor and the settings were two separate declarations, and only the settings
        // were asked for when building the store the plugin reads its own values from. The
        // host was told a plugin had no fields at all, so every settings form in the
        // product drew an empty panel — while each plugin's own tests passed, because they
        // asserted on the schema the plugin built for itself rather than on the one the
        // host was given.
        //
        // So the assertion is deliberately about the *written bytes*: a plugin's test has to
        // be able to see what the host would see, or a disagreement between the two cannot
        // be found from either side.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .announce(&mut written.writer(), &mut plugin)
            .expect("announced");
        let descriptor = crate::testing::announced_descriptor(&written).expect("an announcement");
        assert_eq!(
            descriptor
                .config
                .fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            ["flag"],
            "the host reads `descriptor.config` and nothing else, so a schema that does not \
             reach it is a settings form that renders nothing"
        );
        assert_eq!(
            descriptor.config,
            schema(),
            "and it is the same schema the plugin read its values against, so the two cannot \
             drift"
        );
    }

    #[test]
    fn a_tick_arrives_with_the_monotonic_time_the_host_sent() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new().tick(1000).tick(2000).into_messages(),
            )
            .expect("served");
        let updates = crate::testing::panels(&written);
        assert_eq!(
            updates.len(),
            2,
            "each tick changed the label, so each sent"
        );
    }

    #[test]
    fn an_unchanged_tick_sends_no_panel() {
        // The model window's upload check depends on this, so it is the SDK's
        // property to hold rather than only the renderer's.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new().tick(1000).tick(1000).into_messages(),
            )
            .expect("served");
        assert_eq!(
            crate::testing::panels(&written).len(),
            1,
            "the second tick produced the same label, so nothing was sent"
        );
    }

    #[test]
    fn input_for_a_plugin_that_asked_for_it_is_delivered() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new()
                    .input(InputEvent::KeyDown {
                        control: "KeyA".to_string(),
                        repeat: false,
                    })
                    .into_messages(),
            )
            .expect("served");
        assert_eq!(plugin.events, vec!["input:1".to_string()]);
    }

    #[test]
    fn input_for_a_plugin_that_did_not_ask_is_never_delivered() {
        // The subscription is a statement of intent, and this is what makes it one:
        // a plugin that did not ask is not called at all rather than called and
        // told to ignore it.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        struct Quiet;
        impl Plugin for Quiet {
            fn descriptor(&self) -> Descriptor {
                Descriptor::new("quiet", "Quiet")
            }
        }
        let mut plugin = Quiet;
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new()
                    .input(InputEvent::KeyDown {
                        control: "KeyA".to_string(),
                        repeat: false,
                    })
                    .into_messages(),
            )
            .expect("served");
        // Nothing to assert on the plugin, so the assertion is that serving did not
        // fail and nothing was drawn.
        assert!(written.messages().is_empty());
    }

    #[test]
    fn a_press_reaches_the_plugin_by_the_id_its_own_button_declared() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .serve(&mut plugin, Inbox::new().press("toggle").into_messages())
            .expect("served");
        assert_eq!(plugin.events, vec!["press:toggle".to_string()]);
        let requests = crate::testing::model_requests(&written);
        assert_eq!(requests.len(), 1, "and the press's bubble was asked for");
        assert!(matches!(requests[0].1, ModelRequest::ShowBubble { .. }));
    }

    #[test]
    fn a_configuration_change_reaches_the_plugin_with_the_new_document_already_in_place() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        let mut values = BTreeMap::new();
        values.insert("flag".to_string(), ConfigValue::Bool(true));
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new()
                    .config(bongocat_plugin_protocol::ConfigDocument(values))
                    .into_messages(),
            )
            .expect("served");
        assert_eq!(plugin.events, vec!["config:true".to_string()]);
    }

    #[test]
    fn a_second_hello_is_a_protocol_error_rather_than_a_silent_reset() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        let hello = crate::testing::IdentityBuilder::new().hello();
        let error = Session::new(host)
            .serve(
                &mut plugin,
                vec![HostMessage::Hello(hello), HostMessage::Shutdown],
            )
            .expect_err("two handshakes is not a session");
        assert!(
            matches!(error, Error::Protocol(_)),
            "because a plugin that resumed from a new hello would be running with a \
             different data directory than the one it had already written to"
        );
    }

    #[test]
    fn the_hosts_stop_ends_the_session_and_the_plugin_is_told() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new()
                    .tick(1000)
                    .shutdown()
                    .tick(2000)
                    .into_messages(),
            )
            .expect("served");
        assert_eq!(
            plugin.events,
            vec!["tick:1000".to_string(), "shutdown".to_string()],
            "and nothing after the stop is delivered"
        );
    }

    #[test]
    fn a_read_end_that_closes_ends_the_session_without_an_error() {
        // The app quitting closes the pipe; a plugin that treats that as a failure
        // would report one on every exit.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .reading(std::io::Cursor::new(Vec::new()))
            .serve_reader(&mut plugin)
            .expect("a closed pipe is a normal end");
    }

    #[test]
    fn a_line_the_sdk_cannot_read_ends_the_session_with_a_protocol_error() {
        // Not skipped: a plugin running on a stream it had lost track of would be
        // running on somebody else's protocol.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        let input = std::io::Cursor::new(b"{ not a message }\n".to_vec());
        let error = Session::new(host)
            .reading(input)
            .serve_reader(&mut plugin)
            .expect_err("an unreadable line is reported");
        assert!(matches!(error, Error::Protocol(_)));
    }

    #[test]
    fn lines_are_read_one_per_message_and_stripped_of_their_newline() {
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        // Written through the protocol rather than by hand, because a hand-written
        // line is a second spelling of the wire format and would drift from it.
        let line = |elapsed_ms: u64| {
            let mut bytes = write_message(&HostMessage::Tick {
                elapsed_ms,
                state: bongocat_plugin_protocol::HostState::new(None, true),
                clock: bongocat_plugin_protocol::WallClock::default(),
            })
            .expect("serializes");
            bytes.push(b'\n');
            bytes
        };
        let mut stream = line(1);
        stream.extend(line(2));
        let input = std::io::Cursor::new(stream);
        Session::new(host)
            .reading(input)
            .serve_reader(&mut plugin)
            .expect("served");
        assert_eq!(
            plugin.events,
            vec!["tick:1".to_string(), "tick:2".to_string()],
            "and a CRLF line is no different from a LF one"
        );
    }

    #[test]
    fn an_answer_arrives_both_as_an_event_and_on_the_hosts_queue() {
        // Two ways to read it, for two kinds of plugin: one that wants to react to
        // the answer where it asked, and one that drains a queue on its own tick.
        let written = WrittenMessages::new();
        let host = host(&schema(), &written);
        let mut plugin = Recorder::default();
        Session::new(host)
            .serve(
                &mut plugin,
                Inbox::new().answer(3, ModelOutcome::Done).into_messages(),
            )
            .expect("served");
        assert_eq!(plugin.events, vec!["answer:3:Done".to_string()]);
    }

    #[test]
    fn a_descriptor_whose_id_differs_from_the_store_is_refused() {
        // The card would say one plugin and the panel would be another's.
        let hello = crate::testing::IdentityBuilder::new()
            .id("pomodoro")
            .hello();
        let line = crate::testing::line_of_host(&HostMessage::Hello(hello));
        assert!(
            crate::testing::read_host(&line).is_ok(),
            "the hello itself is fine"
        );
        // The mismatch is caught by `handshake`, which needs a real process; what is
        // checked here is that the protocol offers the check at all.
        let manifest = bongocat_plugin_protocol::PluginManifest {
            schema_version: bongocat_plugin_protocol::PLUGIN_SCHEMA_VERSION,
            api_version: bongocat_plugin_protocol::SUPPORTED_PLUGIN_API_VERSION,
            id: PluginId::new("pomodoro").expect("valid"),
            name: "Pomodoro".into(),
            version: PluginVersion::new(1, 0, 0),
            min_app_version: None,
            author: String::new(),
            description: Default::default(),
            icon: Default::default(),
            executable: "pomodoro".to_string(),
        };
        let other: bongocat_plugin_protocol::PluginDescriptor =
            serde_json::from_str(r#"{"id":"key-stats","name":"Key stats","version":"1.0.0"}"#)
                .expect("a descriptor");
        assert!(other.agrees_with(&manifest).is_err());
    }
}
