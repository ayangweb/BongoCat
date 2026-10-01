//! Talking to the SDK in a test, without a host.
//!
//! A plugin is a program that talks to another program, which makes it awkward to
//! test — and awkwardly in three specific ways.
//!
//! A plugin is a program that talks to another program, which makes it hard to
//! test: the obvious way is to run it and read what it wrote, and the obvious
//! reading of that is fragile in three specific ways. A test that spawns the
//! plugin binary has to build it first, so a unit test in the SDK would depend on
//! `cargo` succeeding. A test that spawns a child has to deal with pipes and
//! timeouts, so a hung plugin becomes a hung test suite. And a test that asserts on
//! the plugin's stdout by hand ends up re-implementing the protocol, which means
//! the thing being tested is the test.
//!
//! So this module supplies the other half: an in-memory [`Host`] whose writes are
//! captured as messages, an [`Inbox`] a test fills with the host's messages, and a
//! [`Session`] that dispatches them. A plugin's test says what happened and what
//! the plugin drew, and never spawns a process.
//!
//! It is part of the public surface because a plugin's tests live in the plugin's
//! own crate: without it, the only way to test a plugin would be the three things
//! above.

use crate::Identity;
use crate::panel::Panel;
use crate::settings::Values;
use bongocat_plugin_protocol::{
    ConfigDocument, ConfigSchema, HostMessage, PluginId, PluginMessage, PluginVersion, SceneNode,
    Subscription, write_message,
};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A writer that records every line it is given as a message.
///
/// Shares the record behind an `Arc` so a test can read the messages after the
/// plugin has written them, and after the [`Host`] that owns the writer is gone —
/// which matters because a plugin's `run` consumes the host.
#[derive(Clone, Debug, Default)]
pub struct WrittenMessages {
    lines: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl WrittenMessages {
    pub fn new() -> Self {
        Self::default()
    }

    /// The messages written so far, in order.
    pub fn messages(&self) -> Vec<PluginMessage> {
        self.lines
            .lock()
            .expect("the record is not poisoned")
            .iter()
            .map(|line| {
                bongocat_plugin_protocol::parse_plugin_message(line)
                    .expect("the SDK wrote a message this SDK can read")
            })
            .collect()
    }

    /// The raw lines, for a test that wants to see the framing.
    pub fn lines(&self) -> Vec<Vec<u8>> {
        self.lines
            .lock()
            .expect("the record is not poisoned")
            .clone()
    }

    /// A writer that appends to this record.
    pub fn writer(&self) -> Box<dyn Write + Send> {
        Box::new(RecordingWriter {
            lines: Arc::clone(&self.lines),
        })
    }

    /// A writer that fails every write, for a test about a host that has gone.
    pub fn failing_writer(&self) -> Box<dyn Write + Send> {
        Box::new(FailingWriter)
    }
}

#[derive(Debug)]
struct RecordingWriter {
    lines: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Write for RecordingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        // The SDK writes one whole line per call, so a partial write would be a bug
        // rather than a shape to handle. Recorded without the newline, because what
        // a test asserts on is the message and not the framing.
        self.lines
            .lock()
            .expect("the record is not poisoned")
            .push(buffer.trim_ascii().to_vec());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the host went away",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the host went away",
        ))
    }
}

/// What a test needs to build an [`Identity`], and how.
#[derive(Clone, Debug)]
pub struct IdentityBuilder {
    id: String,
    version: PluginVersion,
    app_version: String,
    plugin_dir: PathBuf,
    data_dir: PathBuf,
    locale: String,
}

impl Default for IdentityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityBuilder {
    pub fn new() -> Self {
        Self {
            id: "test-plugin".to_string(),
            version: PluginVersion::new(1, 0, 0),
            app_version: "2.0.1".to_string(),
            plugin_dir: PathBuf::from("/tmp/test-plugin"),
            data_dir: PathBuf::from("/tmp/test-plugin-data"),
            locale: "en-US".to_string(),
        }
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = id.to_string();
        self
    }

    pub fn locale(mut self, locale: &str) -> Self {
        self.locale = locale.to_string();
        self
    }

    pub fn data_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.data_dir = directory.into();
        self
    }

    /// The protocol's `hello`, for a test that reads the handshake.
    pub fn hello(&self) -> bongocat_plugin_protocol::Hello {
        bongocat_plugin_protocol::Hello {
            protocol_version: bongocat_plugin_protocol::PROTOCOL_VERSION,
            app_version: self.app_version.clone(),
            id: PluginId::new(self.id.clone()).expect("a test id is a valid id"),
            version: self.version,
            plugin_dir: self.plugin_dir.display().to_string(),
            data_dir: self.data_dir.display().to_string(),
            locale: self.locale.clone(),
        }
    }

    /// The protocol's `hello` for an identity, which is what a test that already
    /// built one needs.
    pub fn hello_for(identity: &Identity) -> bongocat_plugin_protocol::Hello {
        bongocat_plugin_protocol::Hello {
            protocol_version: bongocat_plugin_protocol::PROTOCOL_VERSION,
            app_version: identity.app_version.clone(),
            id: identity.id.clone(),
            version: identity.version,
            plugin_dir: identity.plugin_dir.display().to_string(),
            data_dir: identity.data_dir.display().to_string(),
            locale: identity.locale.clone(),
        }
    }

    pub fn build(&self) -> Identity {
        Identity {
            id: PluginId::new(self.id.clone()).expect("a test id is a valid id"),
            version: self.version,
            app_version: self.app_version.clone(),
            plugin_dir: self.plugin_dir.clone(),
            data_dir: self.data_dir.clone(),
            locale: self.locale.clone(),
        }
    }
}

/// The messages a test wants a plugin to receive, in order.
#[derive(Clone, Debug, Default)]
pub struct Inbox {
    messages: Vec<HostMessage>,
}

impl Inbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a tick at `elapsed_ms`, with the window visible and no model loaded.
    pub fn tick(self, elapsed_ms: u64) -> Self {
        self.tick_with(
            elapsed_ms,
            bongocat_plugin_protocol::HostState::new(None, true),
        )
    }

    /// Queue a tick with the host's own facts.
    pub fn tick_with(self, elapsed_ms: u64, state: bongocat_plugin_protocol::HostState) -> Self {
        self.tick_at(
            elapsed_ms,
            state,
            bongocat_plugin_protocol::WallClock::default(),
        )
    }

    /// Queue a tick with the host's facts and its reading of the local clock.
    ///
    /// The clock is a separate argument rather than something a tick invents,
    /// because it is the one fact a plugin cannot work out for itself: the local UTC
    /// offset is only sound when one thread at a time asks, and a plugin's thread is
    /// not that one.
    pub fn tick_at(
        mut self,
        elapsed_ms: u64,
        state: bongocat_plugin_protocol::HostState,
        clock: bongocat_plugin_protocol::WallClock,
    ) -> Self {
        self.messages.push(HostMessage::Tick {
            elapsed_ms,
            state,
            clock,
        });
        self
    }

    /// Queue one input event.
    pub fn input(mut self, event: bongocat_plugin_protocol::InputEvent) -> Self {
        self.messages.push(HostMessage::Input {
            events: vec![event],
        });
        self
    }

    /// Queue a press on a button.
    pub fn press(mut self, id: &str) -> Self {
        self.messages
            .push(HostMessage::Press { id: id.to_string() });
        self
    }

    /// Queue a configuration document.
    pub fn config(mut self, document: bongocat_plugin_protocol::ConfigDocument) -> Self {
        self.messages
            .push(HostMessage::ConfigChanged { config: document });
        self
    }

    /// Queue the answer to a model request.
    pub fn answer(mut self, id: u64, outcome: bongocat_plugin_protocol::ModelOutcome) -> Self {
        self.messages.push(HostMessage::ModelAnswer { id, outcome });
        self
    }

    /// Queue the host's stop.
    pub fn shutdown(mut self) -> Self {
        self.messages.push(HostMessage::Shutdown);
        self
    }

    /// Queue the handshake.
    pub fn hello(mut self, identity: &Identity) -> Self {
        self.messages
            .push(HostMessage::Hello(IdentityBuilder::hello_for(identity)));
        self
    }

    pub fn into_messages(self) -> Vec<HostMessage> {
        self.messages
    }
}

/// A schema and its defaults, for a test that wants values without a file.
pub fn values(schema: &ConfigSchema) -> Values {
    crate::settings::fit(&schema.defaults(), schema)
}

/// A schema and a document the test chose, fitted to it.
///
/// The same fitting the run loop does, so a test that asks "what does my plugin do with
/// this value" is asking through the path the value actually arrives by — including the
/// clamping, which is part of what the plugin is entitled to rely on.
pub fn values_from(document: &ConfigDocument, schema: &ConfigSchema) -> Values {
    crate::settings::fit(document, schema)
}

/// A one-field schema, for a test whose only setting is a switch.
pub fn one_flag(key: &str, default: bool) -> ConfigSchema {
    bongocat_plugin_protocol::ConfigSchema {
        schema_version: bongocat_plugin_protocol::CONFIG_SCHEMA_VERSION,
        fields: vec![bongocat_plugin_protocol::ConfigField {
            key: key.to_string(),
            label: bongocat_plugin_protocol::LocalizedText::from(key),
            description: None,
            control: bongocat_plugin_protocol::ConfigControl::Toggle { default },
        }],
    }
}

/// The protocol's update a panel would send.
pub fn update_of(panel: &Panel) -> bongocat_plugin_protocol::PanelUpdate {
    panel.to_update()
}

/// A panel's tree, collected into something a test can ask questions of.
///
/// A deliberately shallow stand-in for the renderer: it records which pressables
/// exist, what each is labelled, and which are greyed. It is *not* the product's
/// layout — that is `bongocat-plugin-render`, exercised against the same node
/// vocabulary in the application's own tests. What a plugin's test needs is "did my
/// panel draw two pressable buttons with these ids", and this answers that without a
/// font file, a GPU or a second process.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelSummary {
    /// Every button id, in tree order.
    pub buttons: Vec<String>,
    /// Every button drawn greyed.
    pub disabled: Vec<String>,
    /// Every label on the panel, in tree order.
    pub labels: Vec<String>,
}

impl PanelSummary {
    /// The pressables' ids, sorted, so an assertion does not depend on draw order.
    pub fn press_targets(&self) -> Vec<String> {
        let mut ids = self.buttons.clone();
        ids.sort();
        ids
    }

    /// The greyed pressables' ids.
    pub fn disabled_targets(&self) -> Vec<String> {
        self.disabled.clone()
    }

    /// How many pressables the panel has.
    pub fn hit_test_count(&self) -> usize {
        self.buttons.len()
    }

    /// One pressable, by id.
    pub fn target(&self, id: &str) -> Option<&String> {
        self.buttons
            .iter()
            .find(|candidate| candidate.as_str() == id)
    }
}

/// Walk a panel's tree and summarise it.
pub fn panel_from(panel: &Panel) -> PanelSummary {
    let update = panel.to_update();
    let mut summary = PanelSummary::default();
    collect(&update.scene, &mut summary);
    summary
}

/// A panel's pressable by id, for a test that asks about one of them.
pub fn rendered_hit<'a>(summary: &'a PanelSummary, id: &str) -> Option<&'a String> {
    summary.target(id)
}

fn collect(scene: &SceneNode, out: &mut PanelSummary) {
    match scene {
        bongocat_plugin_protocol::SceneNode::Text(text) => out.labels.push(text.value.clone()),
        bongocat_plugin_protocol::SceneNode::Button(button) => {
            out.buttons.push(button.id.clone());
            out.labels.push(button.label.clone());
            if button.disabled {
                out.disabled.push(button.id.clone());
            }
        }
        bongocat_plugin_protocol::SceneNode::Stack(stack) => {
            for child in &stack.children {
                collect(child, out);
            }
        }
        _ => {}
    }
}

/// Every label in a panel's tree, in tree order.
///
/// A test that wants to know what a plugin *said* rather than what it drew: the
/// numbers in a panel are labels, and reading them out of the tree is more robust
/// than reading them out of pixels.
pub fn labels_in(scene: &SceneNode) -> Vec<String> {
    let mut labels = Vec::new();
    collect_labels(scene, &mut labels);
    labels
}

fn collect_labels(scene: &SceneNode, out: &mut Vec<String>) {
    match scene {
        bongocat_plugin_protocol::SceneNode::Text(text) => out.push(text.value.clone()),
        bongocat_plugin_protocol::SceneNode::Button(button) => out.push(button.label.clone()),
        bongocat_plugin_protocol::SceneNode::Stack(stack) => {
            for child in &stack.children {
                collect_labels(child, out);
            }
        }
        _ => {}
    }
}

/// Every button id in a panel's tree, in tree order.
pub fn button_ids(scene: &SceneNode) -> Vec<String> {
    let mut ids = Vec::new();
    collect_buttons(scene, &mut ids);
    ids
}

fn collect_buttons(scene: &SceneNode, out: &mut Vec<String>) {
    match scene {
        bongocat_plugin_protocol::SceneNode::Button(button) => out.push(button.id.clone()),
        bongocat_plugin_protocol::SceneNode::Stack(stack) => {
            for child in &stack.children {
                collect_buttons(child, out);
            }
        }
        _ => {}
    }
}

/// Whether a message is a plugin's announcement of itself.
pub fn is_ready(message: &PluginMessage) -> bool {
    matches!(message, PluginMessage::Ready { .. })
}

/// The descriptor a plugin announced, from what it wrote.
pub fn announced_descriptor(
    written: &WrittenMessages,
) -> Option<bongocat_plugin_protocol::PluginDescriptor> {
    written
        .messages()
        .into_iter()
        .find_map(|message| match message {
            PluginMessage::Ready { descriptor } => Some(*descriptor),
            _ => None,
        })
}

/// The panel messages a plugin sent, in order.
pub fn panels(written: &WrittenMessages) -> Vec<bongocat_plugin_protocol::PanelUpdate> {
    written
        .messages()
        .into_iter()
        .filter_map(|message| match message {
            PluginMessage::Panel(update) => Some(*update),
            _ => None,
        })
        .collect()
}

/// The last panel a plugin sent.
pub fn last_panel(written: &WrittenMessages) -> Option<bongocat_plugin_protocol::PanelUpdate> {
    panels(written).into_iter().next_back()
}

/// The model requests a plugin made, as `(id, request)` pairs.
pub fn model_requests(
    written: &WrittenMessages,
) -> Vec<(u64, bongocat_plugin_protocol::ModelRequest)> {
    written
        .messages()
        .into_iter()
        .filter_map(|message| match message {
            PluginMessage::Request { id, request } => Some((id, *request)),
            _ => None,
        })
        .collect()
}

/// The feeds a descriptor asks for.
pub fn subscriptions(descriptor: &crate::Descriptor) -> Vec<Subscription> {
    descriptor.subscriptions().to_vec()
}

/// A document built from pairs, for a test that wants a specific configuration.
pub fn document(
    values: BTreeMap<String, bongocat_plugin_protocol::ConfigValue>,
) -> bongocat_plugin_protocol::ConfigDocument {
    bongocat_plugin_protocol::ConfigDocument(values)
}

/// The line a message would be written as, for a test about the wire.
pub fn line_of(message: &PluginMessage) -> Vec<u8> {
    write_message(message).expect("a message this SDK writes serializes")
}

/// The line a host message would be written as.
pub fn line_of_host(message: &HostMessage) -> Vec<u8> {
    write_message(message).expect("a host message serializes")
}

/// Read one host message, for a test that writes a line by hand.
pub fn read_host(line: &[u8]) -> std::result::Result<HostMessage, crate::Error> {
    crate::run::read_host_message(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Host;

    #[test]
    fn a_recording_writer_captures_messages_a_test_can_read_back() {
        let written = WrittenMessages::new();
        let mut host = Host::new(
            written.writer(),
            IdentityBuilder::new().build(),
            one_flag("flag", true),
            values(&one_flag("flag", true)),
        )
        .expect("a host");
        host.hide_panel();
        assert_eq!(written.messages().len(), 1);
        assert!(matches!(written.messages()[0], PluginMessage::HidePanel));
    }

    #[test]
    fn the_record_survives_the_host_that_wrote_to_it() {
        // A plugin's `run` consumes the host, so a test reads the record through a
        // clone rather than through the connection.
        let written = WrittenMessages::new();
        {
            let mut host = Host::new(
                written.writer(),
                IdentityBuilder::new().build(),
                one_flag("flag", true),
                values(&one_flag("flag", true)),
            )
            .expect("a host");
            host.hide_panel();
        }
        assert_eq!(written.messages().len(), 1);
    }
}
