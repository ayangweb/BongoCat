//! Why an update could not be completed, and what it did before failing.
//!
//! An error names the stage it happened at, because the same message means
//! different things before and after a byte is installed: a download that failed
//! left the machine as it was, and an install that failed may not have. The
//! stable codes are what the application exports, so they are part of the
//! contract rather than a rendering of the message.

use super::*;

/// A stable-coded update failure, tagged with the stage that produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{}", .code.as_str())]
pub struct UpdateError {
    pub(crate) code: UpdateErrorCode,
    pub(crate) stage: UpdateStage,
}

impl UpdateError {
    pub(crate) const fn new(code: UpdateErrorCode) -> Self {
        Self {
            code,
            stage: UpdateStage::Check,
        }
    }

    /// Build a failure that stopped in a specific stage.
    ///
    /// Public because the stage is part of the value's meaning: a caller that
    /// classifies a failure itself — or a test that scripts one — has to be able to
    /// say which step it happened in.
    pub const fn at(stage: UpdateStage, code: UpdateErrorCode) -> Self {
        Self { code, stage }
    }

    pub const fn code(self) -> UpdateErrorCode {
        self.code
    }

    pub const fn code_str(self) -> &'static str {
        self.code.as_str()
    }

    pub const fn stage(self) -> UpdateStage {
        self.stage
    }

    /// The stage a library failure happened in once the transport has already been
    /// entered.
    ///
    /// `cargo_packager_updater::Update::download` reads the payload and verifies its
    /// signature in one call, so a single library error can come from either step;
    /// the code decides which. Anything unrecognized is reported as a transfer
    /// failure, which is the conservative choice: it does not claim the payload was
    /// authenticated.
    pub(crate) const fn download_stage(code: UpdateErrorCode) -> UpdateStage {
        match code {
            UpdateErrorCode::SignatureInvalid => UpdateStage::Verify,
            _ => UpdateStage::Download,
        }
    }

    /// Map a library failure onto the stable code catalog.
    ///
    /// `cargo_packager_updater::Error` is `#[non_exhaustive]`, so an unrecognized
    /// variant degrades to [`UpdateErrorCode::Internal`] rather than leaking library
    /// text into the diagnostics export.
    pub(crate) fn from_library(error: cargo_packager_updater::Error) -> Self {
        use cargo_packager_updater::Error;

        let code = match error {
            // Configuration and build-time mismatches: nothing can be requested.
            Error::EmptyEndpoints
            | Error::UrlParse(_)
            | Error::UnsupportedArch
            | Error::UnsupportedOs
            | Error::Http(_) => UpdateErrorCode::NotConfigured,

            // The published release carries no manifest this build could fetch.
            Error::ReleaseNotFound => UpdateErrorCode::ReleaseFetchFailed,

            // The manifest arrived but is not one this build can read.
            //
            // `Semver` belongs here rather than with the configuration failures: this
            // build's own version is parsed before any request is made, so a semver
            // error out of the library can only be the manifest's `version` field.
            Error::Serialization(_) | Error::Semver(_) => UpdateErrorCode::ReleaseManifestInvalid,

            // A well-formed manifest that says nothing about this host.
            Error::TargetNotFound(_) => UpdateErrorCode::NoMatchingAsset,

            Error::Network(_) | Error::Reqwest(_) => UpdateErrorCode::DownloadTransportFailed,

            // Authenticity of the downloaded payload.
            Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
                UpdateErrorCode::SignatureInvalid
            }

            // Payload shape and the install step itself.
            Error::UnsupportedUpdateFormat
            | Error::FailedToDetermineExtractPath
            | Error::TempDirNotOnSameMountPoint
            | Error::PersistError(_)
            | Error::Io(_) => UpdateErrorCode::InstallFailed,

            _ => UpdateErrorCode::Internal,
        };

        Self::new(code)
    }
}
