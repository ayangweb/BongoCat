//! The plugin host.
//!
//! A plugin is a program. This crate is everything that runs a set of them and turns
//! what they say into something on the model window, and it is five pieces, each in
//! its own file, with the boundaries between them being the design:
//!
//! * [`catalog`] — where the list of plugins comes from, and how an archive is
//!   fetched. The reachability policy is imported from `bongocat-update` rather than
//!   restated, so there is one answer to "which mirrors do we try".
//! * [`store`] — what is installed, and the atomicity of an install. One directory
//!   per version and a `current` file, so an interrupted download leaves the previous
//!   version live rather than a half-written one.
//! * [`session`] — one plugin's process: started, spoken to, and stopped. A child
//!   with two pipes and a line protocol, which is why this crate has no `unsafe` in
//!   it and why a plugin that crashes costs a card rather than the app.
//! * [`input_feed`] — the runtime's input, republished to the plugins that asked for
//!   it. A bounded, counted, folded channel; the runtime stays the only owner of
//!   pressed state.
//! * [`bubble`] and [`model_request`] — the two ways a plugin asks the product to do
//!   something: show a line of text, or move the model by name.
//!
//! # The thread model
//!
//! None of this is on the render path. The runtime owns the model's state and
//! publishes a frame at up to 240 Hz; a plugin's panel is produced on the worker at a
//! much lower cadence, and reaches the model window through `bongocat_render`'s own
//! latest-wins layer channel. The consequence worth stating is that **a plugin cannot
//! slow the model down**, and not because of a budget that is checked — because there
//! is no shared resource to contend for.
//!
//! # What "installing" means
//!
//! Installing is: fetch the archive, check the digest, check the signature, unpack it
//! into a version directory, read and validate the `plugin.json` inside it, and only
//! then mark that version live. Every step after "fetch" is a step that can fail
//! without changing what is installed.

#![forbid(unsafe_code)]

mod bubble;
mod catalog;
mod host;
mod input_feed;
mod local_time;
mod model_request;
mod placement;
mod plugin_log;
mod session;
mod signal;
mod sound;
mod store;

mod worker;

/// The plugin vocabulary, re-exported so a caller needs this crate alone.
///
/// Everything on this list is the protocol's, not this crate's — a descriptor, a
/// settings schema, a value, an error. Re-exporting rather than asking every caller to
/// depend on `bongocat-plugin-protocol` as well is what makes the plugin boundary one
/// dependency: the application speaks to a worker, and a worker hands back this
/// vocabulary, so this crate is the whole of what the application needs to know about
/// plugins. A caller that wants the raw protocol — a plugin, or a test that is testing
/// the protocol rather than the host — depends on `bongocat-plugin-protocol` directly.
pub use bongocat_plugin_protocol::{
    ActionGlyph, CONFIG_SCHEMA_VERSION, ChoiceOption, ConfigControl, ConfigDocument, ConfigField,
    ConfigKind, ConfigSchema, ConfigValue, HostState, InputEvent, LocalizedText, LogLevel,
    MAXIMUM_ACTIONS, ModelOutcome, ModelRequest, PLUGIN_CATALOG_FILE_NAME, PLUGIN_SCHEMA_VERSION,
    PluginAction, PluginAnchor, PluginDescriptor, PluginError, PluginErrorCode, PluginIcon,
    PluginId, PluginManifest, PluginVersion, SUPPORTED_PLUGIN_API_VERSION, Subscription,
};
pub use bubble::{BUBBLE_HEIGHT, BUBBLE_WIDTH, Bubble, BubbleSet};
pub use catalog::{
    CATALOG_REQUEST_TIMEOUT, CatalogSource, LOCAL_BUILD_DIRECTORY, LoadedCatalog,
    MAXIMUM_ARCHIVE_BYTES, MAXIMUM_CATALOG_BYTES, agent, catalog_sources, catalog_url,
    download_with, fetch_archive, fetch_catalog, host_platform, load_local,
    local_catalog_directory, local_plugin_directories, proxied_catalog_url,
};
pub use host::HostFacts;
pub use input_feed::{Feed, FeedDiagnostics, FeedSet};
pub use local_time::{LocalTimeCache, WallClock};
pub use model_request::ModelRequestRouter;
pub use placement::{Claimed, POSITIONS, Placed, Placements, parse_anchor};
pub use plugin_log::Line;
pub use session::{
    HANDSHAKE_TIMEOUT, Incoming, MAXIMUM_RESTARTS, Session, SessionDiagnostics, SessionFacts,
    SessionOutcome, SessionState, restart_is_allowed,
};
pub use signal::{Arrival, Inbox, Wake};
pub use sound::{MAXIMUM_SOUND_BYTES, Refusal, SoundOutcome};
pub use store::{
    CURRENT_VERSION_FILE, MAXIMUM_RETAINED_VERSIONS, PluginStore, digest_hex, digest_matches,
    verify_signature,
};
pub use worker::{
    CatalogMode, MAXIMUM_ENABLED_PLUGINS, PluginCommand, PluginDiagnostics, PluginEntry,
    PluginInputSink, PluginPhase, PluginPressSink, PluginSnapshot, PluginWorkerEndpoint,
    PluginWorkerHandle, PluginWorkerJoinError, PluginWorkerReader, WorkerStopper, start,
};

/// How long one archive transfer may take.
///
/// Generous, and for the same reason the updater's is: the point is to escape a
/// dead connection, not to police a slow one. A plugin archive is small compared
/// with a release payload, and the plugin center is not blocked while one is in
/// flight.
pub const ARCHIVE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1800);
