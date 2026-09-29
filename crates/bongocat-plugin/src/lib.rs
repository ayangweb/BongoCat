//! The plugin host.
//!
//! A plugin is data, and this crate is everything that turns data into something
//! on the model window. It is four pieces, each in its own file, and the
//! boundaries between them are the design:
//!
//! * [`catalog`] — where the list of plugins comes from, and how an archive is
//!   fetched. The reachability policy is imported from `bongocat-update` rather
//!   than restated, so there is one answer to "which mirrors do we try".
//! * [`store`] — what is installed, and the atomicity of an install. One
//!   directory per version and a `current` file, so an interrupted download
//!   leaves the previous version live rather than a half-written one.
//! * [`engine`] — the behaviors, which are the whole of a plugin's runtime. Four
//!   kinds, six actions, no code.
//! * [`host`] — the handful of facts the runtime publishes to a panel, and no
//!   more.
//!
//! # The thread model
//!
//! None of this is on the render path. The runtime owns the model's state and
//! publishes a frame at up to 240 Hz; a plugin panel is produced on a worker
//! thread at a much lower cadence, and reaches the model window through
//! `bongocat_render`'s own latest-wins layer channel. The consequence worth
//! stating is that **a plugin cannot slow the model down**, and not because of a
//! budget that is checked — because there is no shared resource to contend for.
//!
//! # What "installing" means
//!
//! Installing is: fetch the archive, check the digest, check the signature,
//! unpack it into a version directory, read and validate the manifest inside it,
//! and only then mark that version live. Every step after "fetch" is a step that
//! can fail without changing what is installed.

#![forbid(unsafe_code)]

mod catalog;
mod engine;
mod host;
mod local_time;
mod store;
#[cfg(test)]
mod tests;
mod worker;

/// The protocol's error type, re-exported so a caller needs this crate alone.///
/// The plugin vocabulary is the protocol's, not this crate's, but a caller that only
/// starts a worker and reads its snapshot should not have to depend on the protocol
/// to name the errors it can be handed.
pub use bongocat_plugin_protocol::{
    OverlayContribution, PLUGIN_CATALOG_FILE_NAME, PluginAnchor, PluginError, PluginErrorCode,
    PluginId, PluginManifest, PluginVersion, SceneNode, SpacerNode,
};
pub use catalog::{
    CATALOG_REQUEST_TIMEOUT, CatalogSource, LoadedCatalog, MAXIMUM_ARCHIVE_BYTES,
    MAXIMUM_CATALOG_BYTES, agent, catalog_sources, catalog_url, download_with, fetch_archive,
    fetch_catalog, host_platform, load_local, local_catalog_directory, proxied_catalog_url,
};
pub use engine::{
    BehaviorState, CountdownState, CounterState, PluginInstance, StopwatchState, WallClock,
    format_duration, format_time, validate_bindings,
};
pub use host::{HOST_PREFIX, HostFacts};
pub use local_time::LocalTimeCache;
pub use store::{
    CURRENT_VERSION_FILE, MAXIMUM_RETAINED_VERSIONS, PluginStore, digest_hex, digest_matches,
    verify_signature,
};
pub use worker::{
    CatalogMode, EVALUATION_INTERVAL, IDLE_EVALUATION_INTERVAL, MAXIMUM_ENABLED_PLUGINS,
    PluginCommand, PluginDiagnostics, PluginEntry, PluginPhase, PluginPressSink, PluginSnapshot,
    PluginWorkerEndpoint, PluginWorkerHandle, PluginWorkerJoinError, PluginWorkerReader,
    WorkerStopper, start,
};

/// How long one archive transfer may take.
///
/// Generous, and for the same reason the updater's is: the point is to escape a
/// dead connection, not to police a slow one. A plugin archive is small compared
/// with a release payload, and the plugin center is not blocked while one is in
/// flight.
pub const ARCHIVE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1800);
