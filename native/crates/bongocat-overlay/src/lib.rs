#[cfg(target_os = "macos")]
mod macos;

use std::{fmt, path::Path, time::Duration};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewReport {
    pub frames_presented: u64,
    pub dynamic_snapshots: u64,
    pub drawable_count: usize,
    pub masked_drawable_count: usize,
    pub texture_count: usize,
}

#[derive(Debug)]
pub struct OverlayError(String);

impl OverlayError {
    pub(crate) fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl fmt::Display for OverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for OverlayError {}

pub fn run_model_preview(
    model_id: &str,
    model_root: &Path,
    duration: Duration,
) -> Result<PreviewReport, OverlayError> {
    #[cfg(target_os = "macos")]
    {
        macos::run_model_preview(model_id, model_root, duration)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (model_id, model_root, duration);
        Err(OverlayError::new(
            "the first visible Live2D renderer is currently available on macOS",
        ))
    }
}
