//! What a BongoCat plugin is written against.
//!
//! This crate is the whole of a plugin author's toolkit, and it is deliberately
//! small. A plugin depends on this and on `serde`, and it learns nothing about the
//! product's internals: there is no GPUI, no renderer, no runtime, no platform
//! code in this dependency graph. What a plugin gets is a **host connection**, a
//! **panel builder**, and a way to **declare its own settings** — and everything
//! else it wants to do, it does with the Rust standard library.
//!
//! # A plugin in full
//!
//! ```no_run
//! use bongocat_plugin_sdk::prelude::*;
//!
//! struct KeyCount {
//!     pressed: u64,
//!     panel: Panel,
//! }
//!
//! impl Plugin for KeyCount {
//!     fn descriptor(&self) -> Descriptor {
//!         Descriptor::new("key-count", "Key Count")
//!             .icon("⌨️")
//!             .description("How many keys you have pressed today.")
//!             .subscribe(Subscription::Input)
//!     }
//!
//!     fn on_input(&mut self, events: Vec<InputEvent>, host: &mut Host) {
//!         for event in &events {
//!             if event.changes_pressed_tally() {
//!                 self.pressed += 1;
//!             }
//!         }
//!         self.draw(host);
//!     }
//!
//!     fn on_tick(&mut self, _tick: Tick, host: &mut Host) {
//!         self.draw(host);
//!     }
//! }
//!
//! impl KeyCount {
//!     fn draw(&mut self, host: &mut Host) {
//!         self.panel.rebuild(|p| {
//!             p.surface(6.0, [12.0, 10.0], |c| {
//!                 c.heading(&self.pressed.to_string(), 30.0);
//!                 c.muted("keys today", 12.0);
//!             })
//!         });
//!         host.panel(&mut self.panel);
//!     }
//! }
//!
//! fn main() -> bongocat_plugin_sdk::Result<()> {
//!     KeyCount { pressed: 0, panel: Panel::new(160, 70) }.run()
//! }
//! ```
//!
//! # What the host does and what the plugin does
//!
//! The line is the one ADR-0079 draws, and it is worth being able to state it in
//! one sentence: **the plugin decides what is shown and the host decides how it
//! looks.** A plugin hands over a tree of nodes; the host lays them out, picks the
//! font, applies the panel's placement arithmetic and rasterizes the result. So a
//! panel looks like part of the product without the plugin knowing what a theme is,
//! and a plugin never touches a GPU handle, a parameter, or a texture.
//!
//! Everything else is the plugin's. Its state, its arithmetic, its persistence, its
//! copy, its dependencies and their versions. That is not a restriction; it is the
//! reason the requests this system answers are answerable at all.
//!
//! # The two directions
//!
//! * The plugin **pushes** — a panel, a log line, a motion it wants played. Each
//!   is one line on the plugin's stdout.
//! * The host **pushes** — a tick with the time elapsed and the host's facts, an
//!   input batch, a press, a configuration change. Each is one line on the
//!   plugin's stdin, delivered to [`Plugin::on_tick`], [`Plugin::on_input`],
//!   [`Plugin::on_press`] and [`Plugin::on_config_changed`].
//!
//! The plugin's own diagnostics go to **stderr**, which the host forwards to its
//! log. The SDK routes [`Host::log`] there, so an author never has to remember
//! which stream is which — and printing to stdout by hand is the one mistake that
//! breaks the protocol, which is why it is a visible failure rather than a silent
//! one.
//!
//! # Testing a plugin
//!
//! [`testing`] is how a plugin tests itself. It supplies an in-memory host whose
//! writes are captured as messages and a way to feed a whole session as data, so a
//! plugin's behaviour is checked through the same dispatch the product runs —
//! rather than by spawning the binary, reading its stdout by hand, and
//! re-implementing the protocol in the test.

#![forbid(unsafe_code)]

mod host;
mod panel;
mod plugin;
mod run;
mod settings;
pub mod testing;

pub use bongocat_plugin_protocol::{
    ConfigDocument, ConfigKind, ConfigSchema, ConfigValue, HostMessage, InputEvent, LocalizedText,
    LogLevel, ModelOutcome, ModelRequest, ModelRequestKind, PluginAnchor, PluginMessage,
    Subscription, control_label,
};
pub use host::{Host, Identity, Outcome};
pub use panel::{
    Panel, SceneBuilder, bar, button, button_disabled, button_secondary, chip, divider, heading,
    image, muted, panel_surface, ring, spacer, spacer_share, text, text_colored,
};
pub use plugin::{Descriptor, Event, Plugin, Tick};
pub use run::{Session, run};
pub use settings::{
    Choice, Decimal, Field, Integer, Option_, Settings, Store, TextField, Toggle, Values,
};

/// Everything a plugin normally names, in one `use`.
///
/// A prelude is a convenience rather than a necessity: every item here is
/// re-exported from the crate root, so a plugin that would rather be explicit
/// about what it uses can be.
pub mod prelude {
    pub use crate::host::{Host, Identity, Outcome};
    pub use crate::panel::{
        Panel, SceneBuilder, bar, button, button_disabled, button_secondary, chip, divider,
        heading, image, muted, panel_surface, ring, spacer, spacer_share, text, text_colored,
    };
    pub use crate::plugin::{Descriptor, Event, Limits, Plugin, Tick};
    pub use crate::run::Session;
    pub use crate::settings::{
        Choice, Decimal, Field, Integer, Option_, Settings, Store, TextField, Toggle, Values,
    };
    pub use crate::{
        ConfigDocument, ConfigKind, ConfigValue, HostMessage, InputEvent, LocalizedText, LogLevel,
        ModelRequest, Subscription, control_label,
    };
    pub use bongocat_plugin_protocol::{
        MAXIMUM_BUBBLE_MILLIS, MINIMUM_BUBBLE_MILLIS, ModelRequestKind, PluginAnchor, SceneNode,
    };
}

/// What went wrong inside the SDK.
///
/// Three cases, and the distinction matters to whoever reads the message: a
/// protocol problem is a bug or a version mismatch, an I/O problem is the pipes
/// closing, and a plugin's own failure is something the author chose to report.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A line on the wire was not a message this host knows.
    #[error("the host sent a message this plugin does not understand: {0}")]
    Protocol(String),
    /// The pipes failed, or the host went away.
    #[error("the connection to BongoCat ended: {0}")]
    Disconnected(String),
    /// The plugin reported its own failure with [`Host::fail`].
    #[error("{0}")]
    Failed(String),
    /// A file the plugin owns could not be read or written.
    #[error("{path}: {detail}")]
    Io { path: String, detail: String },
    /// The configuration document is not the shape the plugin's own struct expects.
    #[error("this plugin's saved settings are not readable: {0}")]
    Config(String),
}

impl Error {
    /// The exit code this error should end the process with.
    ///
    /// Distinct per case because the host reports them differently: a protocol
    /// mismatch is a refusal worth naming, an I/O end is a normal shutdown after
    /// [`Host::shutdown`], and a plugin's own failure is the one a user sees.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Protocol(_) => 2,
            Self::Disconnected(_) => 0,
            Self::Failed(_) => 1,
            Self::Io { .. } | Self::Config(_) => 3,
        }
    }
}

/// What a plugin returns from [`Plugin::run`].
pub type Result<T = ()> = std::result::Result<T, Error>;
