//! Reading JSON within the byte and depth limits a package may use.
//!
//! The depth check is not optional: a document nested a few hundred levels deep
//! is a stack overflow rather than an error message, and it is the cheapest
//! thing in the reader to get wrong.

use super::*;

pub(crate) fn read_json<T: DeserializeOwned>(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
    diagnostic: ModelDiagnostic,
) -> Result<T, ModelError> {
    let bytes = read_bounded(path, reference, maximum_bytes)?;
    parse_json_bytes(&bytes, reference, maximum_depth, diagnostic)
}

pub(crate) fn parse_json_bytes<T: DeserializeOwned>(
    bytes: &[u8],
    reference: &str,
    maximum_depth: usize,
    diagnostic: ModelDiagnostic,
) -> Result<T, ModelError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        ModelError::new(
            diagnostic,
            Some(reference),
            format!("invalid JSON: {error}"),
        )
    })?;
    validate_json_depth(&value, maximum_depth, diagnostic, reference)?;
    serde_json::from_value(value).map_err(|error| {
        ModelError::new(
            diagnostic,
            Some(reference),
            format!("invalid JSON structure: {error}"),
        )
    })
}

pub(crate) fn validate_json_depth(
    value: &serde_json::Value,
    maximum_depth: usize,
    diagnostic: ModelDiagnostic,
    reference: &str,
) -> Result<(), ModelError> {
    fn visit(value: &serde_json::Value, depth: usize, maximum_depth: usize) -> bool {
        if depth > maximum_depth {
            return false;
        }
        match value {
            serde_json::Value::Array(values) => values
                .iter()
                .all(|value| visit(value, depth + 1, maximum_depth)),
            serde_json::Value::Object(values) => values
                .values()
                .all(|value| visit(value, depth + 1, maximum_depth)),
            _ => true,
        }
    }

    if maximum_depth == 0 || !visit(value, 1, maximum_depth) {
        return Err(ModelError::new(
            diagnostic,
            Some(reference),
            format!("JSON nesting exceeds {maximum_depth} levels"),
        ));
    }
    Ok(())
}
