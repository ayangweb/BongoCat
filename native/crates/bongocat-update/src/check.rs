use crate::{
    UpdateDecision, UpdateManifestEndpoint, UpdateManifestFetchError, UpdateManifestSource,
    UpdateVerificationSession, UpdateVerificationSessionError,
};
use std::fmt;

/// Executes the only permitted ordering for a manifest check: fetch, verify,
/// then persist the verified anti-rollback sequence in the session.
pub struct UpdateCheckCoordinator<S> {
    source: S,
}

impl<S> UpdateCheckCoordinator<S> {
    pub const fn new(source: S) -> Self {
        Self { source }
    }

    pub fn source(&self) -> &S {
        &self.source
    }
}

#[derive(Debug)]
pub enum UpdateCheckError {
    Fetch(UpdateManifestFetchError),
    Verification(UpdateVerificationSessionError),
}

impl UpdateCheckError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Fetch(error) => error.code(),
            Self::Verification(error) => error.code(),
        }
    }
}

impl fmt::Display for UpdateCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for UpdateCheckError {}

impl<S: UpdateManifestSource> UpdateCheckCoordinator<S> {
    pub fn check(
        &self,
        session: &mut UpdateVerificationSession,
        endpoint: &UpdateManifestEndpoint,
    ) -> Result<UpdateDecision, UpdateCheckError> {
        let envelope = self
            .source
            .fetch(endpoint)
            .map_err(UpdateCheckError::Fetch)?;
        session
            .verify_envelope(&envelope)
            .map_err(UpdateCheckError::Verification)
    }
}

#[cfg(test)]
mod tests {
    use super::UpdateCheckCoordinator;
    use crate::{
        BuildEnvironment, StorageLayout, TargetTriple, TrustedPublicKey, UpdateChannel,
        UpdateDecision, UpdateManifestEndpoint, UpdateManifestEnvelope, UpdateManifestFetchError,
        UpdateManifestSource, UpdateSequenceStore,
    };
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    const KEY_ID: &str = "development-test-key";

    #[derive(Clone)]
    struct StubSource {
        result: Result<UpdateManifestEnvelope, UpdateManifestFetchError>,
    }

    impl UpdateManifestSource for StubSource {
        fn fetch(
            &self,
            _: &UpdateManifestEndpoint,
        ) -> Result<UpdateManifestEnvelope, UpdateManifestFetchError> {
            self.result.clone()
        }
    }

    fn lower_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn signed_envelope() -> (TrustedPublicKey, UpdateManifestEnvelope) {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let artifact = b"verified artifact";
        let manifest = json!({
            "schema_version": 1,
            "channel": "development",
            "release_version": "0.2.0",
            "minimum_upgradable_version": "0.1.0",
            "release_sequence": 2,
            "published_at_unix_seconds": 1,
            "release_notes_url": null,
            "artifacts": [{
                "target": "aarch64-apple-darwin",
                "architecture": "aarch64",
                "url": "https://updates.example.invalid/bongocat.zip",
                "byte_length": artifact.len(),
                "sha256": lower_hex(&Sha256::digest(artifact)),
            }],
        });
        let bytes = serde_json::to_vec(&manifest).expect("manifest bytes");
        let signature = signing_key.sign(&bytes);
        let trusted_key = TrustedPublicKey::new(
            KEY_ID,
            UpdateChannel::Development,
            signing_key.verifying_key().to_bytes(),
            1,
            None,
        )
        .expect("trusted key");
        let envelope =
            UpdateManifestEnvelope::from_headers(bytes, KEY_ID, &lower_hex(&signature.to_bytes()))
                .expect("envelope");
        (trusted_key, envelope)
    }

    #[test]
    fn verified_fetch_advances_the_environment_sequence_once() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let (trusted_key, envelope) = signed_envelope();
        let mut session = crate::UpdateVerificationSession::open(
            &layout,
            TargetTriple::Aarch64AppleDarwin,
            "0.1.0",
            vec![trusted_key],
        )
        .expect("session");
        let coordinator = UpdateCheckCoordinator::new(StubSource {
            result: Ok(envelope),
        });
        let endpoint = UpdateManifestEndpoint::new("https://updates.example.invalid/manifest.json")
            .expect("endpoint");

        assert!(matches!(
            coordinator.check(&mut session, &endpoint),
            Ok(UpdateDecision::Available(_))
        ));
        assert_eq!(
            UpdateSequenceStore::open_for_layout(&layout)
                .expect("sequence store")
                .highest_verified_sequence()
                .expect("sequence"),
            2
        );
    }

    #[test]
    fn fetch_failure_cannot_advance_the_environment_sequence() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let (trusted_key, _) = signed_envelope();
        let mut session = crate::UpdateVerificationSession::open(
            &layout,
            TargetTriple::Aarch64AppleDarwin,
            "0.1.0",
            vec![trusted_key],
        )
        .expect("session");
        let failure = UpdateManifestEndpoint::new("http://updates.example.invalid")
            .expect_err("invalid endpoint");
        let coordinator = UpdateCheckCoordinator::new(StubSource {
            result: Err(failure),
        });
        let endpoint = UpdateManifestEndpoint::new("https://updates.example.invalid/manifest.json")
            .expect("endpoint");

        assert_eq!(
            coordinator
                .check(&mut session, &endpoint)
                .expect_err("fetch failure")
                .code(),
            "update_manifest_endpoint_invalid"
        );
        assert_eq!(
            UpdateSequenceStore::open_for_layout(&layout)
                .expect("sequence store")
                .highest_verified_sequence()
                .expect("sequence"),
            0
        );
    }
}
