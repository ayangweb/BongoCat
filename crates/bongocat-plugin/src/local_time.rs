//! Reading the local wall clock, safely, from a thread that is not the main one.
//!
//! A `local_time` behavior has to show the user's local time, and getting it means
//! asking the operating system for its current UTC offset. That question is the
//! awkward part: `time` documents `current_local_offset` as sound only when one
//! thread at a time asks, because the offset comes from process-global state that
//! another thread can change underneath a read.
//!
//! The plugin worker is a second thread, so it must not ask. The answer is that the
//! **main thread** asks — once per refresh — and publishes the result, and the
//! worker only ever reads what was published. That is not a workaround; it is the
//! arrangement the crate's own documentation calls for, and it has the useful side
//! effect that a clock is at most one refresh behind rather than racing the
//! timezone database.
//!
//! The reading is a `WallClock` of hours, minutes and seconds, and nothing else.
//! A panel cannot ask for a date, a timezone or a locale, because a plugin has no
//! way to name one — which is what keeps a clock panel the same picture on every
//! machine.

use crate::engine::WallClock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// A wall-clock reading shared between the thread that refreshes it and the thread
/// that reads it.
#[derive(Debug)]
pub struct LocalTimeCache {
    reading: Mutex<WallClock>,
    /// Whether a refresh has ever succeeded, so a panel can tell "midnight" from
    /// "never read". Both look the same otherwise, and a clock stuck at 00:00 looks
    /// broken rather than uninitialised.
    primed: AtomicBool,
}

impl Default for LocalTimeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalTimeCache {
    /// A cache that has never been refreshed, reading midnight.
    pub fn new() -> Self {
        Self {
            reading: Mutex::new(WallClock::new(0, 0, 0)),
            primed: AtomicBool::new(false),
        }
    }

    /// Read the local time and publish it.
    ///
    /// Call this from the one thread that is allowed to ask the operating system.
    /// A failure is not the caller's problem: the previous reading stays, so the
    /// clock keeps showing the last value it managed to get rather than snapping to
    /// midnight.
    pub fn refresh(&self) {
        let Some(now) = read_local_time() else {
            return;
        };
        *self
            .reading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
        self.primed.store(true, Ordering::Release);
    }

    /// The most recent reading, which any thread may take.
    pub fn read(&self) -> WallClock {
        *self
            .reading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether a refresh has ever succeeded.
    pub fn is_primed(&self) -> bool {
        self.primed.load(Ordering::Acquire)
    }
}

/// The local time, or `None` when the platform will not say.
///
/// UTC plus the offset the operating system reports. The offset is read first and
/// the instant second, so a reading is never the two taken microseconds apart.
fn read_local_time() -> Option<WallClock> {
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    // `to_offset` is infallible by construction: it panics rather than returning a
    // `Result`, because the only offset it rejects is one outside ±24 hours and no
    // platform reports one.
    let now = time::OffsetDateTime::now_utc().to_offset(offset);
    Some(WallClock::new(now.hour(), now.minute(), now.second()))
}
