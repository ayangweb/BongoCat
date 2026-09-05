#![forbid(unsafe_code)]

mod check;
mod download;
mod retry;
mod schedule;
mod sequence;
mod staging;
mod transport;
pub use check::{UpdateCheckCoordinator, UpdateCheckError};
pub use download::{
    CompletedUpdateDownload, UpdateDownloadCoordinator, UpdateDownloadError,
    UpdateDownloadErrorCode,
};
pub use retry::{
    UPDATE_DOWNLOAD_MAX_ATTEMPTS, UpdateDownloadAttemptFailure, UpdateDownloadRetryPolicy,
};
pub use schedule::{
    AUTOMATIC_UPDATE_CHECK_INTERVAL, AutomaticUpdateCheckReason, AutomaticUpdateCheckScheduler,
    UpdateScheduleError, UpdateScheduleErrorCode,
};
pub use sequence::{
    UPDATE_SEQUENCE_STATE_SCHEMA_VERSION, UpdateSequenceStore, UpdateSequenceStoreError,
    UpdateSequenceStoreErrorCode,
};
pub use staging::{StagedUpdateArtifact, UpdateStagingError, UpdateStagingErrorCode};
pub use transport::{
    UpdateManifestEndpoint, UpdateManifestFetchError, UpdateManifestSource,
    UpdateManifestTransportErrorCode, UreqUpdateManifestSource,
};

use bongocat_config::{BuildEnvironment, StorageLayout};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt, io::Read};
use url::Url;

pub const UPDATE_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const MAX_UPDATE_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_UPDATE_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const UPDATE_MANIFEST_KEY_ID_HEADER: &str = "bongocat-update-key-id";
pub const UPDATE_MANIFEST_SIGNATURE_HEADER: &str = "bongocat-update-signature-ed25519";
const MAX_UPDATE_ARTIFACTS: usize = 8;
const MAX_UPDATE_URL_BYTES: usize = 2_048;
const MAX_VERSION_BYTES: usize = 64;
const ED25519_SIGNATURE_BYTES: usize = 64;
const ED25519_SIGNATURE_HEX_BYTES: usize = ED25519_SIGNATURE_BYTES * 2;

const fn update_channel(environment: BuildEnvironment) -> UpdateChannel {
    match environment {
        BuildEnvironment::Development => UpdateChannel::Development,
        BuildEnvironment::Production => UpdateChannel::Production,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Development,
    Production,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    Aarch64,
    X86_64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum TargetTriple {
    #[serde(rename = "aarch64-apple-darwin")]
    Aarch64AppleDarwin,
    #[serde(rename = "x86_64-apple-darwin")]
    X86_64AppleDarwin,
    #[serde(rename = "aarch64-pc-windows-msvc")]
    Aarch64PcWindowsMsvc,
    #[serde(rename = "x86_64-pc-windows-msvc")]
    X86_64PcWindowsMsvc,
}

impl TargetTriple {
    pub const fn architecture(self) -> Architecture {
        match self {
            Self::Aarch64AppleDarwin | Self::Aarch64PcWindowsMsvc => Architecture::Aarch64,
            Self::X86_64AppleDarwin | Self::X86_64PcWindowsMsvc => Architecture::X86_64,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UpdateTarget {
    pub target: TargetTriple,
    pub architecture: Architecture,
}

impl UpdateTarget {
    pub const fn new(target: TargetTriple) -> Self {
        Self {
            target,
            architecture: target.architecture(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateErrorCode {
    ArtifactHashInvalid,
    ArtifactHashMismatch,
    ArtifactLengthInvalid,
    ArtifactLengthMismatch,
    ArtifactReadFailed,
    ArtifactTargetDuplicate,
    ArtifactTargetInvalid,
    ArtifactTargetMissing,
    ArtifactUrlInvalid,
    CurrentVersionInvalid,
    CurrentVersionTooOld,
    ManifestArtifactCountInvalid,
    ManifestChannelMismatch,
    ManifestJsonInvalid,
    ManifestKeyIdInvalid,
    ManifestMinimumVersionInvalid,
    ManifestPublishedAtInvalid,
    ManifestReleaseNotesUrlInvalid,
    ManifestReleaseSequenceInvalid,
    ManifestSchemaUnsupported,
    ManifestSignatureInvalid,
    ManifestSignatureEncodingInvalid,
    ManifestSignatureLengthInvalid,
    ManifestTooLarge,
    ManifestVersionInvalid,
    RollbackDetected,
    TrustedKeyDuplicate,
    TrustedKeyIdInvalid,
    TrustedKeyInvalid,
    TrustedKeyMissing,
    TrustedKeySequenceInvalid,
    TrustedKeyUnknown,
}

impl UpdateErrorCode {
    pub const ALL: [Self; 32] = [
        Self::ArtifactHashInvalid,
        Self::ArtifactHashMismatch,
        Self::ArtifactLengthInvalid,
        Self::ArtifactLengthMismatch,
        Self::ArtifactReadFailed,
        Self::ArtifactTargetDuplicate,
        Self::ArtifactTargetInvalid,
        Self::ArtifactTargetMissing,
        Self::ArtifactUrlInvalid,
        Self::CurrentVersionInvalid,
        Self::CurrentVersionTooOld,
        Self::ManifestArtifactCountInvalid,
        Self::ManifestChannelMismatch,
        Self::ManifestJsonInvalid,
        Self::ManifestKeyIdInvalid,
        Self::ManifestMinimumVersionInvalid,
        Self::ManifestPublishedAtInvalid,
        Self::ManifestReleaseNotesUrlInvalid,
        Self::ManifestReleaseSequenceInvalid,
        Self::ManifestSchemaUnsupported,
        Self::ManifestSignatureInvalid,
        Self::ManifestSignatureEncodingInvalid,
        Self::ManifestSignatureLengthInvalid,
        Self::ManifestTooLarge,
        Self::ManifestVersionInvalid,
        Self::RollbackDetected,
        Self::TrustedKeyDuplicate,
        Self::TrustedKeyIdInvalid,
        Self::TrustedKeyInvalid,
        Self::TrustedKeyMissing,
        Self::TrustedKeySequenceInvalid,
        Self::TrustedKeyUnknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArtifactHashInvalid => "artifact_hash_invalid",
            Self::ArtifactHashMismatch => "artifact_hash_mismatch",
            Self::ArtifactLengthInvalid => "artifact_length_invalid",
            Self::ArtifactLengthMismatch => "artifact_length_mismatch",
            Self::ArtifactReadFailed => "artifact_read_failed",
            Self::ArtifactTargetDuplicate => "artifact_target_duplicate",
            Self::ArtifactTargetInvalid => "artifact_target_invalid",
            Self::ArtifactTargetMissing => "artifact_target_missing",
            Self::ArtifactUrlInvalid => "artifact_url_invalid",
            Self::CurrentVersionInvalid => "current_version_invalid",
            Self::CurrentVersionTooOld => "current_version_too_old",
            Self::ManifestArtifactCountInvalid => "manifest_artifact_count_invalid",
            Self::ManifestChannelMismatch => "manifest_channel_mismatch",
            Self::ManifestJsonInvalid => "manifest_json_invalid",
            Self::ManifestKeyIdInvalid => "manifest_key_id_invalid",
            Self::ManifestMinimumVersionInvalid => "manifest_minimum_version_invalid",
            Self::ManifestPublishedAtInvalid => "manifest_published_at_invalid",
            Self::ManifestReleaseNotesUrlInvalid => "manifest_release_notes_url_invalid",
            Self::ManifestReleaseSequenceInvalid => "manifest_release_sequence_invalid",
            Self::ManifestSchemaUnsupported => "manifest_schema_unsupported",
            Self::ManifestSignatureInvalid => "manifest_signature_invalid",
            Self::ManifestSignatureEncodingInvalid => "manifest_signature_encoding_invalid",
            Self::ManifestSignatureLengthInvalid => "manifest_signature_length_invalid",
            Self::ManifestTooLarge => "manifest_too_large",
            Self::ManifestVersionInvalid => "manifest_version_invalid",
            Self::RollbackDetected => "rollback_detected",
            Self::TrustedKeyDuplicate => "trusted_key_duplicate",
            Self::TrustedKeyIdInvalid => "trusted_key_id_invalid",
            Self::TrustedKeyInvalid => "trusted_key_invalid",
            Self::TrustedKeyMissing => "trusted_key_missing",
            Self::TrustedKeySequenceInvalid => "trusted_key_sequence_invalid",
            Self::TrustedKeyUnknown => "trusted_key_unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateError {
    pub code: UpdateErrorCode,
}

impl UpdateError {
    const fn new(code: UpdateErrorCode) -> Self {
        Self { code }
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for UpdateError {}

/// A bounded, transport-neutral signed manifest response.
///
/// The caller supplies the exact response body and the two protocol headers.
/// This type retains no HTTP client state and performs no parsing before the
/// verifier authenticates `manifest_bytes`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateManifestEnvelope {
    key_id: String,
    signature: [u8; ED25519_SIGNATURE_BYTES],
    manifest_bytes: Vec<u8>,
}

impl UpdateManifestEnvelope {
    pub fn from_headers(
        manifest_bytes: Vec<u8>,
        key_id: impl Into<String>,
        signature_hex: &str,
    ) -> Result<Self, UpdateError> {
        if manifest_bytes.is_empty() || manifest_bytes.len() > MAX_UPDATE_MANIFEST_BYTES {
            return Err(UpdateError::new(UpdateErrorCode::ManifestTooLarge));
        }
        let key_id = key_id.into();
        if !is_valid_key_id(&key_id) {
            return Err(UpdateError::new(UpdateErrorCode::ManifestKeyIdInvalid));
        }
        let signature = decode_signature(signature_hex)?;
        Ok(Self {
            key_id,
            signature,
            manifest_bytes,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }

    pub fn signature(&self) -> &[u8; ED25519_SIGNATURE_BYTES] {
        &self.signature
    }
}

#[derive(Clone, Debug)]
pub struct TrustedPublicKey {
    key_id: String,
    channel: UpdateChannel,
    verifying_key: VerifyingKey,
    valid_from_sequence: u64,
    valid_through_sequence: Option<u64>,
}

impl TrustedPublicKey {
    pub fn new(
        key_id: impl Into<String>,
        channel: UpdateChannel,
        public_key: [u8; 32],
        valid_from_sequence: u64,
        valid_through_sequence: Option<u64>,
    ) -> Result<Self, UpdateError> {
        let key_id = key_id.into();
        if !is_valid_key_id(&key_id) {
            return Err(UpdateError::new(UpdateErrorCode::TrustedKeyIdInvalid));
        }
        if valid_from_sequence == 0
            || valid_through_sequence.is_some_and(|through| through < valid_from_sequence)
        {
            return Err(UpdateError::new(UpdateErrorCode::TrustedKeySequenceInvalid));
        }
        let verifying_key = VerifyingKey::from_bytes(&public_key)
            .map_err(|_| UpdateError::new(UpdateErrorCode::TrustedKeyInvalid))?;
        Ok(Self {
            key_id,
            channel,
            verifying_key,
            valid_from_sequence,
            valid_through_sequence,
        })
    }

    fn accepts_sequence(&self, sequence: u64) -> bool {
        sequence >= self.valid_from_sequence
            && self
                .valid_through_sequence
                .is_none_or(|through| sequence <= through)
    }
}

fn is_valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Debug)]
pub struct UpdateVerifier {
    channel: UpdateChannel,
    target: UpdateTarget,
    current_version: Version,
    installed_sequence: u64,
    trusted_keys: Vec<TrustedPublicKey>,
}

impl UpdateVerifier {
    pub fn new(
        channel: UpdateChannel,
        target: TargetTriple,
        current_version: &str,
        installed_sequence: u64,
        trusted_keys: Vec<TrustedPublicKey>,
    ) -> Result<Self, UpdateError> {
        let current_version =
            parse_version(current_version, UpdateErrorCode::CurrentVersionInvalid)?;
        if trusted_keys.is_empty() {
            return Err(UpdateError::new(UpdateErrorCode::TrustedKeyMissing));
        }
        let mut key_ids = BTreeSet::new();
        if trusted_keys
            .iter()
            .any(|key| !key_ids.insert(key.key_id.as_str()))
        {
            return Err(UpdateError::new(UpdateErrorCode::TrustedKeyDuplicate));
        }
        Ok(Self {
            channel,
            target: UpdateTarget::new(target),
            current_version,
            installed_sequence,
            trusted_keys,
        })
    }

    pub fn verify_manifest(
        &self,
        manifest_bytes: &[u8],
        key_id: &str,
        signature_bytes: &[u8],
    ) -> Result<UpdateDecision, UpdateError> {
        if manifest_bytes.is_empty() || manifest_bytes.len() > MAX_UPDATE_MANIFEST_BYTES {
            return Err(UpdateError::new(UpdateErrorCode::ManifestTooLarge));
        }
        let signature_bytes: &[u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| UpdateError::new(UpdateErrorCode::ManifestSignatureLengthInvalid))?;
        let key = self
            .trusted_keys
            .iter()
            .find(|key| key.key_id == key_id && key.channel == self.channel)
            .ok_or_else(|| UpdateError::new(UpdateErrorCode::TrustedKeyUnknown))?;
        let signature = Signature::from_bytes(signature_bytes);
        key.verifying_key
            .verify_strict(manifest_bytes, &signature)
            .map_err(|_| UpdateError::new(UpdateErrorCode::ManifestSignatureInvalid))?;

        let manifest: UpdateManifest = serde_json::from_slice(manifest_bytes)
            .map_err(|_| UpdateError::new(UpdateErrorCode::ManifestJsonInvalid))?;
        self.validate_manifest(manifest, key)
    }

    pub fn verify_envelope(
        &self,
        envelope: &UpdateManifestEnvelope,
    ) -> Result<UpdateDecision, UpdateError> {
        self.verify_manifest(
            envelope.manifest_bytes(),
            envelope.key_id(),
            envelope.signature(),
        )
    }

    fn validate_manifest(
        &self,
        manifest: UpdateManifest,
        key: &TrustedPublicKey,
    ) -> Result<UpdateDecision, UpdateError> {
        if manifest.schema_version != UPDATE_MANIFEST_SCHEMA_VERSION {
            return Err(UpdateError::new(UpdateErrorCode::ManifestSchemaUnsupported));
        }
        if manifest.channel != self.channel {
            return Err(UpdateError::new(UpdateErrorCode::ManifestChannelMismatch));
        }
        if manifest.release_sequence == 0 {
            return Err(UpdateError::new(
                UpdateErrorCode::ManifestReleaseSequenceInvalid,
            ));
        }
        if manifest.published_at_unix_seconds == 0 {
            return Err(UpdateError::new(
                UpdateErrorCode::ManifestPublishedAtInvalid,
            ));
        }
        if !key.accepts_sequence(manifest.release_sequence) {
            return Err(UpdateError::new(UpdateErrorCode::TrustedKeySequenceInvalid));
        }
        if manifest.release_sequence < self.installed_sequence {
            return Err(UpdateError::new(UpdateErrorCode::RollbackDetected));
        }

        let release_version = parse_version(
            &manifest.release_version,
            UpdateErrorCode::ManifestVersionInvalid,
        )?;
        let minimum_upgradable_version = parse_version(
            &manifest.minimum_upgradable_version,
            UpdateErrorCode::ManifestMinimumVersionInvalid,
        )?;
        if let Some(url) = manifest.release_notes_url.as_deref() {
            validate_https_url(url)
                .map_err(|_| UpdateError::new(UpdateErrorCode::ManifestReleaseNotesUrlInvalid))?;
        }
        if manifest.artifacts.is_empty() || manifest.artifacts.len() > MAX_UPDATE_ARTIFACTS {
            return Err(UpdateError::new(
                UpdateErrorCode::ManifestArtifactCountInvalid,
            ));
        }

        let mut seen_targets = BTreeSet::new();
        let mut selected_artifact = None;
        for artifact in manifest.artifacts {
            let target = UpdateTarget {
                target: artifact.target,
                architecture: artifact.architecture,
            };
            if target.architecture != target.target.architecture() {
                return Err(UpdateError::new(UpdateErrorCode::ArtifactTargetInvalid));
            }
            if !seen_targets.insert(target) {
                return Err(UpdateError::new(UpdateErrorCode::ArtifactTargetDuplicate));
            }
            let verified = validate_artifact(artifact)?;
            if target == self.target {
                selected_artifact = Some(verified);
            }
        }
        let artifact = selected_artifact
            .ok_or_else(|| UpdateError::new(UpdateErrorCode::ArtifactTargetMissing))?;

        if release_version <= self.current_version {
            return Ok(UpdateDecision::UpToDate {
                release_version: manifest.release_version,
                release_sequence: manifest.release_sequence,
            });
        }
        if manifest.release_sequence <= self.installed_sequence {
            return Err(UpdateError::new(UpdateErrorCode::RollbackDetected));
        }
        if self.current_version < minimum_upgradable_version {
            return Err(UpdateError::new(UpdateErrorCode::CurrentVersionTooOld));
        }
        Ok(UpdateDecision::Available(VerifiedUpdate {
            channel: manifest.channel,
            release_version: manifest.release_version,
            minimum_upgradable_version: manifest.minimum_upgradable_version,
            release_sequence: manifest.release_sequence,
            published_at_unix_seconds: manifest.published_at_unix_seconds,
            release_notes_url: manifest.release_notes_url,
            artifact,
        }))
    }
}

/// Couples manifest verification to the immutable environment-local sequence
/// store. It has no network or installer capability.
pub struct UpdateVerificationSession {
    verifier: UpdateVerifier,
    sequence_store: UpdateSequenceStore,
}

#[derive(Debug)]
pub enum UpdateVerificationSessionError {
    SequenceStore(UpdateSequenceStoreError),
    Verification(UpdateError),
}

impl fmt::Display for UpdateVerificationSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SequenceStore(error) => error.fmt(formatter),
            Self::Verification(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for UpdateVerificationSessionError {}

impl UpdateVerificationSessionError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SequenceStore(error) => error.code().as_str(),
            Self::Verification(error) => error.code.as_str(),
        }
    }
}

impl UpdateVerificationSession {
    pub fn open(
        layout: &StorageLayout,
        target: TargetTriple,
        current_version: &str,
        trusted_keys: Vec<TrustedPublicKey>,
    ) -> Result<Self, UpdateVerificationSessionError> {
        let sequence_store = UpdateSequenceStore::open_for_layout(layout)
            .map_err(UpdateVerificationSessionError::SequenceStore)?;
        let installed_sequence = sequence_store
            .highest_verified_sequence()
            .map_err(UpdateVerificationSessionError::SequenceStore)?;
        let verifier = UpdateVerifier::new(
            update_channel(layout.environment),
            target,
            current_version,
            installed_sequence,
            trusted_keys,
        )
        .map_err(UpdateVerificationSessionError::Verification)?;
        Ok(Self {
            verifier,
            sequence_store,
        })
    }

    pub fn verify_manifest(
        &mut self,
        manifest_bytes: &[u8],
        key_id: &str,
        signature_bytes: &[u8],
    ) -> Result<UpdateDecision, UpdateVerificationSessionError> {
        let decision = self
            .verifier
            .verify_manifest(manifest_bytes, key_id, signature_bytes)
            .map_err(UpdateVerificationSessionError::Verification)?;
        let sequence = decision.release_sequence();
        let recorded = self
            .sequence_store
            .record_verified_sequence(sequence)
            .map_err(UpdateVerificationSessionError::SequenceStore)?;
        self.verifier.installed_sequence = recorded;
        Ok(decision)
    }

    pub fn verify_envelope(
        &mut self,
        envelope: &UpdateManifestEnvelope,
    ) -> Result<UpdateDecision, UpdateVerificationSessionError> {
        self.verify_manifest(
            envelope.manifest_bytes(),
            envelope.key_id(),
            envelope.signature(),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateDecision {
    UpToDate {
        release_version: String,
        release_sequence: u64,
    },
    Available(VerifiedUpdate),
}

impl UpdateDecision {
    const fn release_sequence(&self) -> u64 {
        match self {
            Self::UpToDate {
                release_sequence, ..
            } => *release_sequence,
            Self::Available(update) => update.release_sequence,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedUpdate {
    pub channel: UpdateChannel,
    pub release_version: String,
    pub minimum_upgradable_version: String,
    pub release_sequence: u64,
    pub published_at_unix_seconds: u64,
    pub release_notes_url: Option<String>,
    pub artifact: VerifiedArtifact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedArtifact {
    pub target: UpdateTarget,
    pub url: String,
    pub byte_length: u64,
    sha256: [u8; 32],
}

impl VerifiedArtifact {
    pub fn verify_reader(&self, mut reader: impl Read) -> Result<(), UpdateError> {
        let mut hasher = Sha256::new();
        let mut byte_length = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|_| UpdateError::new(UpdateErrorCode::ArtifactReadFailed))?;
            if read == 0 {
                break;
            }
            byte_length = byte_length
                .checked_add(read as u64)
                .ok_or_else(|| UpdateError::new(UpdateErrorCode::ArtifactLengthMismatch))?;
            if byte_length > self.byte_length {
                return Err(UpdateError::new(UpdateErrorCode::ArtifactLengthMismatch));
            }
            hasher.update(&buffer[..read]);
        }
        if byte_length != self.byte_length {
            return Err(UpdateError::new(UpdateErrorCode::ArtifactLengthMismatch));
        }
        if hasher.finalize().as_slice() != self.sha256 {
            return Err(UpdateError::new(UpdateErrorCode::ArtifactHashMismatch));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifest {
    schema_version: u32,
    channel: UpdateChannel,
    release_version: String,
    minimum_upgradable_version: String,
    release_sequence: u64,
    published_at_unix_seconds: u64,
    release_notes_url: Option<String>,
    artifacts: Vec<UpdateArtifact>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateArtifact {
    target: TargetTriple,
    architecture: Architecture,
    url: String,
    byte_length: u64,
    sha256: String,
}

fn validate_artifact(artifact: UpdateArtifact) -> Result<VerifiedArtifact, UpdateError> {
    if artifact.byte_length == 0 || artifact.byte_length > MAX_UPDATE_ARTIFACT_BYTES {
        return Err(UpdateError::new(UpdateErrorCode::ArtifactLengthInvalid));
    }
    validate_https_url(&artifact.url)
        .map_err(|_| UpdateError::new(UpdateErrorCode::ArtifactUrlInvalid))?;
    let sha256 = decode_sha256(&artifact.sha256)?;
    Ok(VerifiedArtifact {
        target: UpdateTarget {
            target: artifact.target,
            architecture: artifact.architecture,
        },
        url: artifact.url,
        byte_length: artifact.byte_length,
        sha256,
    })
}

fn validate_https_url(value: &str) -> Result<(), ()> {
    if value.len() > MAX_UPDATE_URL_BYTES {
        return Err(());
    }
    let url = Url::parse(value).map_err(|_| ())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(());
    }
    Ok(())
}

fn parse_version(value: &str, code: UpdateErrorCode) -> Result<Version, UpdateError> {
    if value.is_empty() || value.len() > MAX_VERSION_BYTES {
        return Err(UpdateError::new(code));
    }
    Version::parse(value).map_err(|_| UpdateError::new(code))
}

fn decode_sha256(value: &str) -> Result<[u8; 32], UpdateError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::new(UpdateErrorCode::ArtifactHashInvalid));
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_lower_hex_digit(pair[0])?;
        let low = decode_lower_hex_digit(pair[1])?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_signature(value: &str) -> Result<[u8; ED25519_SIGNATURE_BYTES], UpdateError> {
    if value.len() != ED25519_SIGNATURE_HEX_BYTES {
        return Err(UpdateError::new(
            UpdateErrorCode::ManifestSignatureEncodingInvalid,
        ));
    }
    let mut decoded = [0_u8; ED25519_SIGNATURE_BYTES];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_lower_hex_digit(pair[0])
            .map_err(|_| UpdateError::new(UpdateErrorCode::ManifestSignatureEncodingInvalid))?;
        let low = decode_lower_hex_digit(pair[1])
            .map_err(|_| UpdateError::new(UpdateErrorCode::ManifestSignatureEncodingInvalid))?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_lower_hex_digit(byte: u8) -> Result<u8, UpdateError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(UpdateError::new(UpdateErrorCode::ArtifactHashInvalid)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::{BuildEnvironment, StorageLayout};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{Value, json};
    use std::io;
    use tempfile::tempdir;

    const SIGNING_SECRET: [u8; 32] = [7; 32];
    const KEY_ID: &str = "release-2026";

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&SIGNING_SECRET)
    }

    fn verifier(
        channel: UpdateChannel,
        current_version: &str,
        installed_sequence: u64,
        valid_from: u64,
        valid_through: Option<u64>,
    ) -> UpdateVerifier {
        let key = signing_key();
        UpdateVerifier::new(
            channel,
            TargetTriple::Aarch64AppleDarwin,
            current_version,
            installed_sequence,
            vec![
                TrustedPublicKey::new(
                    KEY_ID,
                    channel,
                    key.verifying_key().to_bytes(),
                    valid_from,
                    valid_through,
                )
                .expect("trusted key"),
            ],
        )
        .expect("verifier")
    }

    fn manifest(artifact_bytes: &[u8]) -> Value {
        json!({
            "schema_version": 1,
            "channel": "development",
            "release_version": "0.2.0",
            "minimum_upgradable_version": "0.1.0",
            "release_sequence": 2,
            "published_at_unix_seconds": 1788566400_u64,
            "release_notes_url": "https://updates.example.invalid/releases/0.2.0",
            "artifacts": [{
                "target": "aarch64-apple-darwin",
                "architecture": "aarch64",
                "url": "https://updates.example.invalid/0.2.0/BongoCat.app.tar.zst",
                "byte_length": artifact_bytes.len(),
                "sha256": lower_hex(&Sha256::digest(artifact_bytes)),
            }],
        })
    }

    fn signed_decision(
        verifier: &UpdateVerifier,
        manifest: &Value,
    ) -> Result<UpdateDecision, UpdateError> {
        let bytes = serde_json::to_vec(manifest).expect("serialize manifest");
        let signature = signing_key().sign(&bytes);
        verifier.verify_manifest(&bytes, KEY_ID, &signature.to_bytes())
    }

    fn lower_hex(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            encoded.push(HEX[usize::from(byte >> 4)] as char);
            encoded.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        encoded
    }

    #[test]
    fn signed_manifest_selects_the_exact_target_and_verifies_artifact() {
        let artifact = b"signed update artifact";
        let decision = signed_decision(
            &verifier(UpdateChannel::Development, "0.1.0", 1, 1, None),
            &manifest(artifact),
        )
        .expect("verified update");
        let UpdateDecision::Available(update) = decision else {
            panic!("expected available update");
        };
        assert_eq!(update.release_version, "0.2.0");
        assert_eq!(update.release_sequence, 2);
        assert_eq!(
            update.artifact.target,
            UpdateTarget::new(TargetTriple::Aarch64AppleDarwin)
        );
        update
            .artifact
            .verify_reader(artifact.as_slice())
            .expect("artifact hash and length");
    }

    #[test]
    fn verification_session_persists_only_successful_manifest_sequences() {
        let directory = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(directory.path(), BuildEnvironment::Development);
        let key = signing_key();
        let trusted_key = TrustedPublicKey::new(
            KEY_ID,
            UpdateChannel::Development,
            key.verifying_key().to_bytes(),
            1,
            None,
        )
        .expect("trusted key");
        let mut session = UpdateVerificationSession::open(
            &layout,
            TargetTriple::Aarch64AppleDarwin,
            "0.1.0",
            vec![trusted_key],
        )
        .expect("verification session");
        let manifest =
            serde_json::to_vec(&manifest(b"signed update artifact")).expect("serialize manifest");
        let signature = key.sign(&manifest);

        assert!(matches!(
            session.verify_manifest(&manifest, KEY_ID, b"invalid signature"),
            Err(UpdateVerificationSessionError::Verification(error))
                if error.code == UpdateErrorCode::ManifestSignatureLengthInvalid
        ));
        let store = UpdateSequenceStore::open_for_layout(&layout).expect("sequence store");
        assert_eq!(
            store.highest_verified_sequence().expect("initial sequence"),
            0
        );

        let envelope = UpdateManifestEnvelope::from_headers(
            manifest.clone(),
            KEY_ID,
            &lower_hex(&signature.to_bytes()),
        )
        .expect("signed envelope");
        assert!(matches!(
            session
                .verify_envelope(&envelope)
                .expect("verified manifest"),
            UpdateDecision::Available(_)
        ));
        assert_eq!(
            store.highest_verified_sequence().expect("stored sequence"),
            2
        );
        assert!(matches!(
            session.verify_manifest(&manifest, KEY_ID, &signature.to_bytes()),
            Err(UpdateVerificationSessionError::Verification(error))
                if error.code == UpdateErrorCode::RollbackDetected
        ));
    }

    #[test]
    fn verification_session_records_a_verified_up_to_date_manifest() {
        let directory = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(directory.path(), BuildEnvironment::Development);
        let key = signing_key();
        let trusted_key = TrustedPublicKey::new(
            KEY_ID,
            UpdateChannel::Development,
            key.verifying_key().to_bytes(),
            1,
            None,
        )
        .expect("trusted key");
        let mut session = UpdateVerificationSession::open(
            &layout,
            TargetTriple::Aarch64AppleDarwin,
            "0.2.0",
            vec![trusted_key],
        )
        .expect("verification session");
        let manifest =
            serde_json::to_vec(&manifest(b"signed update artifact")).expect("serialize manifest");
        let signature = key.sign(&manifest);

        assert!(matches!(
            session
                .verify_manifest(&manifest, KEY_ID, &signature.to_bytes())
                .expect("verified up-to-date manifest"),
            UpdateDecision::UpToDate {
                release_sequence: 2,
                ..
            }
        ));
        assert_eq!(
            UpdateSequenceStore::open_for_layout(&layout)
                .expect("sequence store")
                .highest_verified_sequence()
                .expect("stored sequence"),
            2
        );
    }

    #[test]
    fn signature_is_checked_before_manifest_parsing() {
        let bytes = br#"{"schema_version":1}"#;
        let signature = signing_key().sign(b"different bytes");
        assert_eq!(
            verifier(UpdateChannel::Development, "0.1.0", 1, 1, None)
                .verify_manifest(bytes, KEY_ID, &signature.to_bytes())
                .expect_err("tampered manifest"),
            UpdateError::new(UpdateErrorCode::ManifestSignatureInvalid)
        );
    }

    #[test]
    fn envelope_bounds_headers_and_preserves_the_exact_signed_bytes() {
        let manifest = serde_json::to_vec(&manifest(b"artifact")).expect("serialize manifest");
        let signature = signing_key().sign(&manifest);
        let envelope = UpdateManifestEnvelope::from_headers(
            manifest.clone(),
            KEY_ID,
            &lower_hex(&signature.to_bytes()),
        )
        .expect("valid envelope");
        assert_eq!(envelope.key_id(), KEY_ID);
        assert_eq!(envelope.manifest_bytes(), manifest);
        assert_eq!(envelope.signature().as_slice(), signature.to_bytes());
        assert!(matches!(
            verifier(UpdateChannel::Development, "0.1.0", 1, 1, None).verify_envelope(&envelope),
            Ok(UpdateDecision::Available(_))
        ));

        assert_eq!(
            UpdateManifestEnvelope::from_headers(vec![b'x'], "invalid key id", "00"),
            Err(UpdateError::new(UpdateErrorCode::ManifestKeyIdInvalid))
        );
        assert_eq!(
            UpdateManifestEnvelope::from_headers(vec![b'x'], KEY_ID, "00"),
            Err(UpdateError::new(
                UpdateErrorCode::ManifestSignatureEncodingInvalid
            ))
        );
        let uppercase_signature = format!("A{}", "0".repeat(ED25519_SIGNATURE_HEX_BYTES - 1));
        assert_eq!(
            UpdateManifestEnvelope::from_headers(vec![b'x'], KEY_ID, &uppercase_signature),
            Err(UpdateError::new(
                UpdateErrorCode::ManifestSignatureEncodingInvalid
            ))
        );
        assert_eq!(
            UpdateManifestEnvelope::from_headers(
                vec![b'x'; MAX_UPDATE_MANIFEST_BYTES + 1],
                KEY_ID,
                &lower_hex(&signature.to_bytes()),
            ),
            Err(UpdateError::new(UpdateErrorCode::ManifestTooLarge))
        );
    }

    #[test]
    fn manifest_rejects_wrong_environment_insecure_urls_and_unknown_fields() {
        let production = verifier(UpdateChannel::Production, "0.1.0", 1, 1, None);
        assert_eq!(
            signed_decision(&production, &manifest(b"artifact"))
                .expect_err("cross-environment manifest"),
            UpdateError::new(UpdateErrorCode::ManifestChannelMismatch)
        );

        let development = verifier(UpdateChannel::Development, "0.1.0", 1, 1, None);
        let mut insecure = manifest(b"artifact");
        insecure["artifacts"][0]["url"] = json!("http://updates.example.invalid/artifact");
        assert_eq!(
            signed_decision(&development, &insecure).expect_err("insecure URL"),
            UpdateError::new(UpdateErrorCode::ArtifactUrlInvalid)
        );

        let mut unknown = manifest(b"artifact");
        unknown["untrusted_extension"] = json!(true);
        assert_eq!(
            signed_decision(&development, &unknown).expect_err("unknown manifest field"),
            UpdateError::new(UpdateErrorCode::ManifestJsonInvalid)
        );
    }

    #[test]
    fn trusted_keys_are_environment_bound_and_unknown_keys_are_rejected() {
        let bytes = serde_json::to_vec(&manifest(b"artifact")).expect("serialize manifest");
        let signature = signing_key().sign(&bytes);
        let verifier = verifier(UpdateChannel::Development, "0.1.0", 1, 1, None);
        assert_eq!(
            verifier
                .verify_manifest(&bytes, "unknown-key", &signature.to_bytes())
                .expect_err("unknown key"),
            UpdateError::new(UpdateErrorCode::TrustedKeyUnknown)
        );

        let production_key = TrustedPublicKey::new(
            KEY_ID,
            UpdateChannel::Production,
            signing_key().verifying_key().to_bytes(),
            1,
            None,
        )
        .expect("production key");
        let development = UpdateVerifier::new(
            UpdateChannel::Development,
            TargetTriple::Aarch64AppleDarwin,
            "0.1.0",
            1,
            vec![production_key],
        )
        .expect("development verifier");
        assert_eq!(
            development
                .verify_manifest(&bytes, KEY_ID, &signature.to_bytes())
                .expect_err("cross-environment key"),
            UpdateError::new(UpdateErrorCode::TrustedKeyUnknown)
        );
    }

    #[test]
    fn verifier_rejects_an_empty_or_duplicate_trust_store() {
        assert_eq!(
            UpdateVerifier::new(
                UpdateChannel::Development,
                TargetTriple::Aarch64AppleDarwin,
                "0.1.0",
                1,
                Vec::new(),
            )
            .expect_err("empty trust store"),
            UpdateError::new(UpdateErrorCode::TrustedKeyMissing)
        );

        let key = TrustedPublicKey::new(
            KEY_ID,
            UpdateChannel::Development,
            signing_key().verifying_key().to_bytes(),
            1,
            None,
        )
        .expect("trusted key");
        assert_eq!(
            UpdateVerifier::new(
                UpdateChannel::Development,
                TargetTriple::Aarch64AppleDarwin,
                "0.1.0",
                1,
                vec![key.clone(), key],
            )
            .expect_err("duplicate key"),
            UpdateError::new(UpdateErrorCode::TrustedKeyDuplicate)
        );
    }

    #[test]
    fn manifest_and_artifact_size_limits_are_enforced() {
        assert_eq!(
            verifier(UpdateChannel::Development, "0.1.0", 1, 1, None)
                .verify_manifest(&vec![b' '; MAX_UPDATE_MANIFEST_BYTES + 1], KEY_ID, &[])
                .expect_err("oversized manifest"),
            UpdateError::new(UpdateErrorCode::ManifestTooLarge)
        );

        let verifier = verifier(UpdateChannel::Development, "0.1.0", 1, 1, None);
        for byte_length in [0, MAX_UPDATE_ARTIFACT_BYTES + 1] {
            let mut invalid = manifest(b"artifact");
            invalid["artifacts"][0]["byte_length"] = json!(byte_length);
            assert_eq!(
                signed_decision(&verifier, &invalid).expect_err("invalid artifact size"),
                UpdateError::new(UpdateErrorCode::ArtifactLengthInvalid)
            );
        }
    }

    #[test]
    fn future_schema_and_invalid_semver_are_rejected() {
        let verifier = verifier(UpdateChannel::Development, "0.1.0", 1, 1, None);
        let mut future = manifest(b"artifact");
        future["schema_version"] = json!(2);
        assert_eq!(
            signed_decision(&verifier, &future).expect_err("future schema"),
            UpdateError::new(UpdateErrorCode::ManifestSchemaUnsupported)
        );

        let mut invalid_version = manifest(b"artifact");
        invalid_version["release_version"] = json!("release-two");
        assert_eq!(
            signed_decision(&verifier, &invalid_version).expect_err("invalid SemVer"),
            UpdateError::new(UpdateErrorCode::ManifestVersionInvalid)
        );
    }

    #[test]
    fn manifest_rejects_target_ambiguity_and_architecture_mismatch() {
        let verifier = verifier(UpdateChannel::Development, "0.1.0", 1, 1, None);
        let mut mismatch = manifest(b"artifact");
        mismatch["artifacts"][0]["architecture"] = json!("x86_64");
        assert_eq!(
            signed_decision(&verifier, &mismatch).expect_err("architecture mismatch"),
            UpdateError::new(UpdateErrorCode::ArtifactTargetInvalid)
        );

        let mut duplicate = manifest(b"artifact");
        let repeated = duplicate["artifacts"][0].clone();
        duplicate["artifacts"]
            .as_array_mut()
            .expect("artifacts")
            .push(repeated);
        assert_eq!(
            signed_decision(&verifier, &duplicate).expect_err("duplicate artifact"),
            UpdateError::new(UpdateErrorCode::ArtifactTargetDuplicate)
        );
    }

    #[test]
    fn version_floor_sequence_and_key_windows_prevent_downgrades() {
        let update = manifest(b"artifact");
        assert_eq!(
            signed_decision(
                &verifier(UpdateChannel::Development, "0.0.9", 1, 1, None),
                &update,
            )
            .expect_err("version below supported floor"),
            UpdateError::new(UpdateErrorCode::CurrentVersionTooOld)
        );
        assert_eq!(
            signed_decision(
                &verifier(UpdateChannel::Development, "0.1.0", 3, 1, None),
                &update,
            )
            .expect_err("sequence rollback"),
            UpdateError::new(UpdateErrorCode::RollbackDetected)
        );
        assert_eq!(
            signed_decision(
                &verifier(UpdateChannel::Development, "0.1.0", 1, 3, None),
                &update,
            )
            .expect_err("key used before rotation window"),
            UpdateError::new(UpdateErrorCode::TrustedKeySequenceInvalid)
        );
    }

    #[test]
    fn current_or_older_release_is_not_offered() {
        let decision = signed_decision(
            &verifier(UpdateChannel::Development, "0.2.0", 2, 1, None),
            &manifest(b"artifact"),
        )
        .expect("valid current manifest");
        assert_eq!(
            decision,
            UpdateDecision::UpToDate {
                release_version: "0.2.0".to_owned(),
                release_sequence: 2,
            }
        );
    }

    #[test]
    fn artifact_verification_rejects_length_hash_and_reader_failures() {
        let decision = signed_decision(
            &verifier(UpdateChannel::Development, "0.1.0", 1, 1, None),
            &manifest(b"artifact"),
        )
        .expect("verified update");
        let UpdateDecision::Available(update) = decision else {
            panic!("expected available update");
        };
        assert_eq!(
            update
                .artifact
                .verify_reader(b"short".as_slice())
                .expect_err("length mismatch")
                .code,
            UpdateErrorCode::ArtifactLengthMismatch
        );
        assert_eq!(
            update
                .artifact
                .verify_reader(b"wrong-by".as_slice())
                .expect_err("hash mismatch")
                .code,
            UpdateErrorCode::ArtifactHashMismatch
        );
        assert_eq!(
            update
                .artifact
                .verify_reader(FailingReader)
                .expect_err("reader failure")
                .code,
            UpdateErrorCode::ArtifactReadFailed
        );
    }

    #[test]
    fn shared_valid_manifest_fixture_matches_the_rust_contract() {
        let bytes = include_bytes!(
            "../../../../shared/update/fixtures/valid-development-aarch64-macos.json"
        );
        let signature = signing_key().sign(bytes);
        verifier(UpdateChannel::Development, "0.1.0", 1, 1, None)
            .verify_manifest(bytes, KEY_ID, &signature.to_bytes())
            .expect("shared update fixture");
    }

    #[test]
    fn verifier_error_codes_are_stable_and_unique() {
        let mut codes = UpdateErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.iter().all(|code| !code.is_empty()));
        assert!(codes.iter().all(|code| {
            code.chars()
                .all(|character| character.is_ascii_lowercase() || character == '_')
        }));
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), UpdateErrorCode::ALL.len());
        assert_eq!(
            UpdateErrorCode::ManifestSignatureInvalid.as_str(),
            "manifest_signature_invalid"
        );
    }

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected"))
        }
    }
}
