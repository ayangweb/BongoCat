use std::fmt;

/// Stable error categories raised while loading or parsing playback clips.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackErrorCode {
    ExpressionInvalid,
    MotionInvalid,
}

impl PlaybackErrorCode {
    pub const ALL: [Self; 2] = [Self::ExpressionInvalid, Self::MotionInvalid];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExpressionInvalid => "expression_invalid",
            Self::MotionInvalid => "motion_invalid",
        }
    }
}

impl fmt::Display for PlaybackErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub struct PlaybackError {
    pub code: PlaybackErrorCode,
    pub detail: String,
}

impl PlaybackError {
    pub(crate) fn new(code: PlaybackErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for PlaybackError {}

#[cfg(test)]
mod tests {
    use super::PlaybackErrorCode;

    #[test]
    fn playback_error_codes_are_stable_and_unique() {
        let mut codes = PlaybackErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect::<Vec<_>>();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), PlaybackErrorCode::ALL.len());
    }
}
