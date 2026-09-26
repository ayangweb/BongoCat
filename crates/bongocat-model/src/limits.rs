//! What a package may contain, and the names a model may take.
//!
//! The limits are the product's answer to a hostile or broken package: a
//! texture larger than the GPU will hold, a JSON document nested deeply enough
//! to exhaust the stack. They are values rather than constants because the
//! settings window shows them and a test has to be able to lower one.

use super::*;

pub const INDEX_SCHEMA_VERSION: u32 = 1;

/// Maximum byte length of a portable model id; also the directory name limit
/// for installed models.
pub const MODEL_ID_MAXIMUM_LENGTH: usize = 64;

/// Directory inside a model package that holds every image the package ships:
/// the background, the cover and the per-key artwork.
pub const PACKAGE_RESOURCES_DIRECTORY: &str = "resources";

/// The cover image a package may ship. It is display artwork for the settings
/// model catalog, so a package without one is a package with nothing to show,
/// not an invalid package.
pub const PACKAGE_COVER_FILE: &str = "cover.png";

pub(crate) const MOTION_TIME_TOLERANCE: f32 = 0.000_001;

/// Absolute path of a package root's cover image, whether or not it exists.
///
/// The layout is shared with the BongoCatMver conversion, which installs the
/// legacy `cat.png` under exactly this name, so both sides read one constant
/// instead of repeating the path.
pub fn package_cover_path(root: &Path) -> PathBuf {
    root.join(PACKAGE_RESOURCES_DIRECTORY)
        .join(PACKAGE_COVER_FILE)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelPackageLimits {
    pub maximum_texture_dimension: u32,
    pub maximum_json_bytes: u64,
    pub maximum_json_depth: usize,
    pub maximum_file_bytes: u64,
    pub maximum_package_bytes: u64,
    pub maximum_file_count: usize,
    pub maximum_directory_depth: usize,
}

impl Default for ModelPackageLimits {
    fn default() -> Self {
        Self {
            maximum_texture_dimension: 8_192,
            maximum_json_bytes: 16 * 1024 * 1024,
            maximum_json_depth: 64,
            maximum_file_bytes: 512 * 1024 * 1024,
            maximum_package_bytes: 1024 * 1024 * 1024,
            maximum_file_count: 4_096,
            maximum_directory_depth: 32,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ModelId(pub(crate) String);

impl ModelId {
    pub fn parse(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MODEL_ID_MAXIMUM_LENGTH
            && !value.starts_with('.')
            && !value.ends_with('.')
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && !is_windows_reserved_name(&value);
        if !valid {
            return Err(ModelError::new(
                ModelDiagnostic::InvalidModelId,
                None,
                "model id must be a portable 1-64 character ASCII store key",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn is_windows_reserved_name(value: &str) -> bool {
    let stem = value.split('.').next().unwrap_or(value);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}
