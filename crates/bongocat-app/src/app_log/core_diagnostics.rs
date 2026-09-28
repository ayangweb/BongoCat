//! What the Cubism side is doing, for the same report.
//!
//! The vendor's own counters are reported next to ours rather than merged into
//! them, so a support thread can tell a message that came from Cubism from one that
//! came from the product without having to know which is which.

/// Anonymous retention counters supplied by the Cubism Core log owner.
/// The application deliberately receives no Core log path or message data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreLogDiagnostics {
    pub written: u64,
    pub dropped: u64,
    pub rotated: u64,
    pub pruned: u64,
    pub bytes: u64,
    pub retained_files: u64,
    pub retained_bytes: u64,
}
