//! Whether this build may install an update at all.
//!
//! A Development build has no signed release to install, and a build with no
//! configured signing key must not install an unsigned one. Both are refusals
//! decided before the network is touched, so a build that cannot update never
//! finds out by trying.

/// The pipeline stage an update stopped in.
///
/// The stage is what lets the UI say *where* an update failed instead of only
/// *that* it failed, so it is part of the failure value rather than something a
/// caller has to infer from the error code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateStage {
    /// Reading and parsing the release manifest.
    Check,
    /// Transferring the payload.
    Download,
    /// Authenticating the downloaded payload.
    Verify,
    /// Writing the payload into the installation.
    Install,
}

impl UpdateStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Download => "download",
            Self::Verify => "verify",
            Self::Install => "install",
        }
    }
}

/// Why this build cannot check for or install updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateUnavailability {
    /// The build's channel is not allowed to update.
    DevelopmentChannel,
    /// No release signing key is provisioned.
    SigningKeyMissing,
}
