//! BongoCat update subsystem.
//!
//! The release-manifest fetch → version comparison → download → signature
//! verification → install → restart pipeline is provided by
//! [`cargo_packager_updater`]. This crate owns only the BongoCat-specific policy
//! around it:
//!
//! - the immutable [`ReleaseConfiguration`] (repository, channel, target) that binds
//!   one update run to a distribution channel,
//! - the [`ReleaseChannel`] gate that keeps Development builds from installing a
//!   release artifact, and
//! - the anonymous [`UpdateDiagnostics`] contract the application exports.
//!
//! # Signing
//!
//! Update payloads are authenticated with a detached Minisign signature. One shared
//! release manifest announces, per shipped target, the payload URL, its signature and
//! its format; the runtime requests that manifest, lets the library pick this host's
//! entry out of it, downloads the payload, verifies it against [`RELEASE_SIGNING_KEY`]
//! before a single byte is installed, and only then hands it to the platform installer.
//! The provisioned [`RELEASE_SIGNING_KEY`] is compiled into the build. The runtime also
//! refuses to run an update when that value is absent, empty or whitespace, so a build
//! with no configured key cannot install an unsigned payload.
//!
//! The signing side lives in `crates/bongocat-packaging`, which uses
//! `cargo_packager::sign` — the same toolchain that produces the artifacts — so the
//! signer and this verifier cannot drift apart.

#![forbid(unsafe_code)]

mod diagnostics;
mod release;
mod runtime;

pub use diagnostics::{
    UpdateDiagnostics, UpdateDiagnosticsTracker, UpdateErrorCode, is_stable_error_code,
};
pub use release::{HOST_TARGET_TRIPLE, ReleaseChannel, ReleaseConfiguration, UpdateTargetTriple};
pub use runtime::{
    RELEASE_BINARY_NAME, RELEASE_BUNDLE_NAME, RELEASE_MANIFEST_NAME, RELEASE_REPOSITORY_NAME,
    RELEASE_REPOSITORY_OWNER, RELEASE_SIGNING_KEY, UpdateError, UpdateOutcome, UpdateRuntime,
};
