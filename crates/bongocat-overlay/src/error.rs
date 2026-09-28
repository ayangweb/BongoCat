//! The error the whole crate reports through.
//!
//! One error type rather than a platform error and a renderer error, because a
//! caller asking "did the overlay work" does not care which half failed, and two
//! error types would make it ask.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverlayErrorKind {
    Fatal,
    TemporaryPresentationUnavailable,
}

#[derive(Debug, thiserror::Error)]
#[error("{detail}")]
pub struct OverlayError {
    pub(crate) kind: OverlayErrorKind,
    pub(crate) detail: String,
}

impl OverlayError {
    pub(crate) fn new(detail: impl Into<String>) -> Self {
        Self {
            kind: OverlayErrorKind::Fatal,
            detail: detail.into(),
        }
    }

    pub(crate) fn temporary_presentation_unavailable(detail: impl Into<String>) -> Self {
        Self {
            kind: OverlayErrorKind::TemporaryPresentationUnavailable,
            detail: detail.into(),
        }
    }

    pub(crate) const fn is_temporary_presentation_unavailable(&self) -> bool {
        matches!(
            self.kind,
            OverlayErrorKind::TemporaryPresentationUnavailable
        )
    }
}
