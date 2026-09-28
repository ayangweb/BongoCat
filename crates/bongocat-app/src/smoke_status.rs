//! How an automated run reports what it did.
//!
//! A smoke is not a product run: it writes a status line the harness reads and
//! exits, so it never leaves a window or a log line behind that a human would
//! have to recognise as a failure.

use super::*;

/// How many 50ms ticks the settings-window smoke waits for its first frame.
///
/// The page assertions read state that only a render assigns, so the smoke has
/// to wait for a frame rather than for a fixed delay: on a loaded machine the
/// old 500ms start-up delay was not always enough.
pub(crate) const SMOKE_FIRST_FRAME_WAIT_TICKS: u32 = 120;

pub(crate) fn write_smoke_status(status: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "bongocat-app: {status}")?;
    stdout.flush()
}

#[cfg(target_os = "windows")]
pub(crate) fn write_smoke_marker(path: &Path, status: &str) -> io::Result<()> {
    let mut file = atomic_write_file::AtomicWriteFile::open(path)?;
    writeln!(file, "{status}")?;
    file.commit()
}
