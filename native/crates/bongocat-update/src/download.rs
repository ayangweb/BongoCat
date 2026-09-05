use crate::{
    StagedUpdateArtifact, StorageLayout, UpdateDownloadAttemptFailure, UpdateDownloadRetryPolicy,
    UpdateErrorCode, UpdateStagingError, UpdateStagingErrorCode, VerifiedArtifact,
};
use std::{fmt, io::Read, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateDownloadErrorCode {
    Cancelled,
    HttpStatus,
    Integrity,
    Staging,
    Transport,
}

impl UpdateDownloadErrorCode {
    pub const ALL: [Self; 5] = [
        Self::Cancelled,
        Self::HttpStatus,
        Self::Integrity,
        Self::Staging,
        Self::Transport,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "update_download_cancelled",
            Self::HttpStatus => "update_download_http_status",
            Self::Integrity => "update_download_integrity_failed",
            Self::Staging => "update_download_staging_failed",
            Self::Transport => "update_download_transport_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UpdateDownloadError {
    code: UpdateDownloadErrorCode,
    attempts: u8,
}

impl UpdateDownloadError {
    const fn new(code: UpdateDownloadErrorCode, attempts: u8) -> Self {
        Self { code, attempts }
    }

    pub const fn code(self) -> UpdateDownloadErrorCode {
        self.code
    }

    pub const fn attempts(self) -> u8 {
        self.attempts
    }
}

impl fmt::Display for UpdateDownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.as_str())
    }
}

impl std::error::Error for UpdateDownloadError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedUpdateDownload {
    artifact: StagedUpdateArtifact,
    attempts: u8,
}

impl CompletedUpdateDownload {
    pub fn artifact(&self) -> &StagedUpdateArtifact {
        &self.artifact
    }

    pub const fn attempts(&self) -> u8 {
        self.attempts
    }
}

/// Coordinates fresh artifact streams with the fixed retry policy.
///
/// `open_reader` must create a new HTTPS response reader for every call.
/// `wait_retry` must return `false` when the caller observes cancellation
/// while waiting; it may use a condition variable instead of sleeping for the
/// entire delay. Neither closure may expose transport-library errors or paths.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateDownloadCoordinator {
    retry_policy: UpdateDownloadRetryPolicy,
}

impl UpdateDownloadCoordinator {
    pub const fn retry_policy(self) -> UpdateDownloadRetryPolicy {
        self.retry_policy
    }

    pub fn stage_with_retry<R, OpenReader, WaitRetry, Cancel>(
        &self,
        artifact: &VerifiedArtifact,
        layout: &StorageLayout,
        mut open_reader: OpenReader,
        mut wait_retry: WaitRetry,
        mut cancelled: Cancel,
    ) -> Result<CompletedUpdateDownload, UpdateDownloadError>
    where
        R: Read,
        OpenReader: FnMut() -> Result<R, UpdateDownloadAttemptFailure>,
        WaitRetry: FnMut(Duration, &mut Cancel) -> bool,
        Cancel: FnMut() -> bool,
    {
        for attempts in 1..=self.retry_policy.max_attempts() {
            if cancelled() {
                return Err(UpdateDownloadError::new(
                    UpdateDownloadErrorCode::Cancelled,
                    attempts.saturating_sub(1),
                ));
            }

            let result = match open_reader() {
                Ok(reader) => artifact
                    .stage_reader(layout, reader, &mut cancelled)
                    .map_err(classify_staging_failure),
                Err(failure) => Err(failure),
            };
            match result {
                Ok(artifact) => return Ok(CompletedUpdateDownload { artifact, attempts }),
                Err(failure) => {
                    let code = error_code(failure);
                    let Some(delay) = self.retry_policy.retry_delay(attempts, failure) else {
                        return Err(UpdateDownloadError::new(code, attempts));
                    };
                    if !wait_retry(delay, &mut cancelled) || cancelled() {
                        return Err(UpdateDownloadError::new(
                            UpdateDownloadErrorCode::Cancelled,
                            attempts,
                        ));
                    }
                }
            }
        }
        unreachable!("the retry policy always stops at its maximum attempt count")
    }
}

fn classify_staging_failure(error: UpdateStagingError) -> UpdateDownloadAttemptFailure {
    match error.code {
        UpdateStagingErrorCode::Cancelled => UpdateDownloadAttemptFailure::Cancelled,
        UpdateStagingErrorCode::Integrity(UpdateErrorCode::ArtifactReadFailed) => {
            UpdateDownloadAttemptFailure::Transport
        }
        UpdateStagingErrorCode::Integrity(_) => UpdateDownloadAttemptFailure::Integrity,
        UpdateStagingErrorCode::DirectoryCreateFailed
        | UpdateStagingErrorCode::DirectoryInvalid
        | UpdateStagingErrorCode::DirectoryPermissionFailed
        | UpdateStagingErrorCode::FileCreateFailed
        | UpdateStagingErrorCode::FileSyncFailed
        | UpdateStagingErrorCode::FileWriteFailed
        | UpdateStagingErrorCode::RemoveFailed => UpdateDownloadAttemptFailure::Staging,
    }
}

const fn error_code(failure: UpdateDownloadAttemptFailure) -> UpdateDownloadErrorCode {
    match failure {
        UpdateDownloadAttemptFailure::Cancelled => UpdateDownloadErrorCode::Cancelled,
        UpdateDownloadAttemptFailure::HttpStatus(_) => UpdateDownloadErrorCode::HttpStatus,
        UpdateDownloadAttemptFailure::Integrity => UpdateDownloadErrorCode::Integrity,
        UpdateDownloadAttemptFailure::Staging => UpdateDownloadErrorCode::Staging,
        UpdateDownloadAttemptFailure::Transport => UpdateDownloadErrorCode::Transport,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::BuildEnvironment;
    use sha2::{Digest, Sha256};
    use std::{
        collections::VecDeque,
        io::{self, Cursor},
    };
    use tempfile::tempdir;

    fn artifact(bytes: &[u8]) -> VerifiedArtifact {
        VerifiedArtifact {
            target: crate::UpdateTarget::new(crate::TargetTriple::Aarch64AppleDarwin),
            url: "https://updates.example.invalid/bongocat.pkg".to_owned(),
            byte_length: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }

    #[test]
    fn error_codes_are_stable_and_unique() {
        let mut codes = UpdateDownloadErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .all(|code| code.starts_with("update_download_"))
        );
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), UpdateDownloadErrorCode::ALL.len());
    }

    #[test]
    fn transient_failures_retry_fresh_streams_then_stage_the_verified_artifact() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let bytes = b"downloaded artifact";
        let mut sources = VecDeque::from([
            Err(UpdateDownloadAttemptFailure::Transport),
            Err(UpdateDownloadAttemptFailure::HttpStatus(503)),
            Ok(Cursor::new(bytes.to_vec())),
        ]);
        let mut delays = Vec::new();

        let completed = UpdateDownloadCoordinator::default()
            .stage_with_retry(
                &artifact(bytes),
                &layout,
                || sources.pop_front().expect("source attempt"),
                |delay, _| {
                    delays.push(delay);
                    true
                },
                || false,
            )
            .expect("successful retry");

        assert_eq!(completed.attempts(), 3);
        assert_eq!(delays, vec![Duration::from_secs(1), Duration::from_secs(2)]);
        assert_eq!(
            std::fs::read(completed.artifact().path()).expect("staged bytes"),
            bytes
        );
    }

    #[test]
    fn interrupted_stream_retries_from_a_clean_staging_file() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let bytes = b"retry after read failure";
        let mut sources = VecDeque::from([
            TestReader::Failing,
            TestReader::Bytes(Cursor::new(bytes.to_vec())),
        ]);
        let mut delays = Vec::new();

        let completed = UpdateDownloadCoordinator::default()
            .stage_with_retry(
                &artifact(bytes),
                &layout,
                || Ok(sources.pop_front().expect("source attempt")),
                |delay, _| {
                    delays.push(delay);
                    true
                },
                || false,
            )
            .expect("retry after reader failure");

        assert_eq!(completed.attempts(), 2);
        assert_eq!(delays, vec![Duration::from_secs(1)]);
        assert_eq!(
            std::fs::read(completed.artifact().path()).expect("staged bytes"),
            bytes
        );
        assert_eq!(
            std::fs::read_dir(&layout.update_staging)
                .expect("staging directory")
                .count(),
            1
        );
    }

    #[test]
    fn terminal_failures_and_exhausted_retries_do_not_wait_again() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let bytes = b"terminal failure";
        let mut calls = 0;

        let terminal = UpdateDownloadCoordinator::default()
            .stage_with_retry::<Cursor<Vec<u8>>, _, _, _>(
                &artifact(bytes),
                &layout,
                || Err(UpdateDownloadAttemptFailure::HttpStatus(404)),
                |_, _| {
                    calls += 1;
                    true
                },
                || false,
            )
            .expect_err("terminal response");
        assert_eq!(terminal.code(), UpdateDownloadErrorCode::HttpStatus);
        assert_eq!(terminal.attempts(), 1);
        assert_eq!(calls, 0);

        let mut attempts = 0;
        let exhausted = UpdateDownloadCoordinator::default()
            .stage_with_retry::<Cursor<Vec<u8>>, _, _, _>(
                &artifact(bytes),
                &layout,
                || {
                    attempts += 1;
                    Err(UpdateDownloadAttemptFailure::Transport)
                },
                |_, _| true,
                || false,
            )
            .expect_err("exhausted retries");
        assert_eq!(exhausted.code(), UpdateDownloadErrorCode::Transport);
        assert_eq!(exhausted.attempts(), 3);
        assert_eq!(attempts, 3);
    }

    #[test]
    fn integrity_and_staging_failures_do_not_retry_or_leave_partial_artifacts() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let mut waits = 0;

        let integrity = UpdateDownloadCoordinator::default()
            .stage_with_retry(
                &artifact(b"expected"),
                &layout,
                || Ok(Cursor::new(b"wrong---".to_vec())),
                |_, _| {
                    waits += 1;
                    true
                },
                || false,
            )
            .expect_err("integrity failure");
        assert_eq!(integrity.code(), UpdateDownloadErrorCode::Integrity);
        assert_eq!(integrity.attempts(), 1);
        assert_eq!(waits, 0);
        assert_eq!(
            std::fs::read_dir(&layout.update_staging)
                .expect("staging directory")
                .count(),
            0
        );

        let staging = UpdateDownloadCoordinator::default()
            .stage_with_retry::<Cursor<Vec<u8>>, _, _, _>(
                &artifact(b"expected"),
                &layout,
                || Err(UpdateDownloadAttemptFailure::Staging),
                |_, _| {
                    waits += 1;
                    true
                },
                || false,
            )
            .expect_err("staging failure");
        assert_eq!(staging.code(), UpdateDownloadErrorCode::Staging);
        assert_eq!(staging.attempts(), 1);
        assert_eq!(waits, 0);
    }

    #[test]
    fn cancellation_stops_before_opening_or_during_retry_wait() {
        let temporary = tempdir().expect("temporary directory");
        let layout = StorageLayout::under(temporary.path(), BuildEnvironment::Development);
        let bytes = b"cancelled download";
        let mut opened = false;

        let before_open = UpdateDownloadCoordinator::default()
            .stage_with_retry::<Cursor<Vec<u8>>, _, _, _>(
                &artifact(bytes),
                &layout,
                || {
                    opened = true;
                    Ok(Cursor::new(bytes.to_vec()))
                },
                |_, _| true,
                || true,
            )
            .expect_err("cancel before open");
        assert_eq!(before_open.code(), UpdateDownloadErrorCode::Cancelled);
        assert_eq!(before_open.attempts(), 0);
        assert!(!opened);

        let during_wait = UpdateDownloadCoordinator::default()
            .stage_with_retry::<Cursor<Vec<u8>>, _, _, _>(
                &artifact(bytes),
                &layout,
                || Err(UpdateDownloadAttemptFailure::Transport),
                |_, _| false,
                || false,
            )
            .expect_err("cancel during retry wait");
        assert_eq!(during_wait.code(), UpdateDownloadErrorCode::Cancelled);
        assert_eq!(during_wait.attempts(), 1);
    }

    enum TestReader {
        Bytes(Cursor<Vec<u8>>),
        Failing,
    }

    impl Read for TestReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Bytes(reader) => reader.read(buffer),
                Self::Failing => Err(io::Error::other("synthetic reader failure")),
            }
        }
    }
}
