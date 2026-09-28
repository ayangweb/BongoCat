//! Turning a declared reference into a path that stays inside the package.
//!
//! A reference is what a package says about its own files, so it is untrusted
//! input: absolute paths, `..` segments and Windows reserved names are all
//! refused here rather than at each of the many places a reference could
//! otherwise be resolved.

use super::*;

/// Normalize and validate a package-relative resource reference.
///
/// This is the shared path-safety primitive used by the read-only package
/// parser and by the model-store import adapter.
pub fn normalize_reference(reference: &str) -> Result<String, ModelError> {
    let normalized = reference.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.is_empty()
        || normalized.contains('\0')
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ModelError::new(
            ModelDiagnostic::ModelReferenceEscapesRoot,
            Some(reference),
            "resource path is absolute, empty, or traverses outside the package",
        ));
    }
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| {
            if part.contains(':') {
                Err(ModelError::new(
                    ModelDiagnostic::ModelReferenceEscapesRoot,
                    Some(reference),
                    "resource path contains a platform path prefix",
                ))
            } else {
                Ok(part)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err(ModelError::new(
            ModelDiagnostic::ModelReferenceInvalid,
            Some(reference),
            "resource path does not name a file",
        ));
    }
    Ok(parts.join("/"))
}

/// Convert a normalized slash-separated package reference to a native path.
///
/// This function only changes separators; it does not validate the reference.
/// Callers handling untrusted input must call [`normalize_reference`] first.
pub fn path_from_reference(reference: &str) -> PathBuf {
    reference.split('/').collect()
}
