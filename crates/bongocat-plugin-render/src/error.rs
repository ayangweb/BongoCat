//! Why a panel could not be drawn.
//!
//! Separate from `bongocat_plugin_protocol::PluginError` because the two answer
//! different questions. The protocol's codes are about a document that is not
//! acceptable — a scene too deep, a path that escapes the plugin directory — and
//! they are decided at load, before anything is drawn. These are about a document
//! that was acceptable and could not be turned into pixels on this machine, which
//! is a narrower and mostly recoverable set.

/// Everything that can stop a panel from becoming pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PluginRenderErrorCode {
    /// The panel's logical size is zero, or the raster would exceed the bound
    /// every layer in `bongocat-render` enforces.
    RasterTooLarge,
    /// The scene could not be laid out. A manifest that validated should not
    /// reach this, so it means the two disagree — which is a bug worth naming
    /// rather than a condition to handle.
    SceneInvalid,
    /// A named image is missing, unreadable, or not a PNG.
    ImageUnreadable,
    /// A named image is a valid PNG but larger than a panel may be.
    ImageTooLarge,
    /// No usable face loaded, and the node needed one. Text is dropped rather
    /// than failing the panel, so this is reported for a developer and not
    /// surfaced to a user.
    FontUnavailable,
}

/// A render failure, named by a stable code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginRenderError {
    pub code: PluginRenderErrorCode,
    pub detail: Option<String>,
}

impl PluginRenderError {
    pub fn new(code: PluginRenderErrorCode, detail: impl std::fmt::Display) -> Self {
        Self {
            code,
            detail: Some(detail.to_string()),
        }
    }

    pub fn bare(code: PluginRenderErrorCode) -> Self {
        Self { code, detail: None }
    }

    pub const fn code(&self) -> PluginRenderErrorCode {
        self.code
    }
}

impl std::fmt::Display for PluginRenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.detail {
            Some(detail) => write!(formatter, "{:?}: {detail}", self.code),
            None => write!(formatter, "{:?}", self.code),
        }
    }
}

impl std::error::Error for PluginRenderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_detail_is_carried_but_never_replaces_the_code() {
        let error = PluginRenderError::new(PluginRenderErrorCode::ImageUnreadable, "no such file");
        assert_eq!(error.code(), PluginRenderErrorCode::ImageUnreadable);
        assert!(error.to_string().contains("no such file"));
        assert!(
            PluginRenderError::bare(PluginRenderErrorCode::SceneInvalid)
                .detail
                .is_none()
        );
    }
}
