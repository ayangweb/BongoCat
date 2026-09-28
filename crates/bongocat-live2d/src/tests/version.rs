//! The Core version the binary is built against, in both forms it is written in.

use super::{CUBISM_CORE_VERSION, CUBISM_CORE_VERSION_TEXT};

/// The report a user pastes into an issue names the Core version in digits and
/// dots, while the load-time check compares a packed integer. Nothing keeps those
/// two spellings in step except this test, so it is the only thing standing
/// between a Core upgrade and a report that quietly names the old version.
#[test]
fn cubism_core_version_matches_this_build() {
    let (major, minor, patch) = (
        CUBISM_CORE_VERSION >> 24,
        (CUBISM_CORE_VERSION >> 16) & 0xFF,
        CUBISM_CORE_VERSION & 0xFFFF,
    );
    assert_eq!(
        CUBISM_CORE_VERSION_TEXT,
        format!("{major}.{minor}.{patch}"),
        "the report text and the packed Core version name different versions"
    );
}
