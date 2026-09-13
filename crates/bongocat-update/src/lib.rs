//! BongoCat update subsystem.
//!
//! The download → checksum/signature verification → extract → install → restart
//! pipeline is provided by [`self_update`]. This crate owns only the
//! BongoCat-specific policy around it:
//!
//! - the immutable [`ReleaseConfiguration`] (repository, channel, target, asset
//!   naming) that binds one update run to a distribution channel,
//! - the [`ReleaseChannel`] gate that keeps Development builds from installing a
//!   release artifact, and
//! - the anonymous [`UpdateDiagnostics`] contract the application exports.
//!
//! # Signing
//!
//! Release archives are authenticated with a zipsign ed25519 signature. Because
//! `self_update::verify_signature` returns `Ok(())` for an empty key set, the
//! runtime refuses to install while [`RELEASE_SIGNING_KEY`] is `None` rather than
//! silently accepting unsigned artifacts. See
//! `docs/adr/0029-third-party-update-library-boundary.md`.

#![forbid(unsafe_code)]

mod diagnostics;
mod release;
mod runtime;

pub use diagnostics::{
    UpdateDiagnostics, UpdateDiagnosticsTracker, UpdateErrorCode, is_stable_error_code,
};
pub use release::{HOST_TARGET_TRIPLE, ReleaseChannel, ReleaseConfiguration, UpdateTargetTriple};
pub use runtime::{
    RELEASE_BINARY_NAME, RELEASE_BUNDLE_NAME, RELEASE_REPOSITORY_NAME, RELEASE_REPOSITORY_OWNER,
    RELEASE_SIGNING_KEY, UpdateError, UpdateOutcome, UpdateRuntime,
};
