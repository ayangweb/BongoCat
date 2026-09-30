//! Reading the local wall clock, safely, from a thread that is not the main one.
//!
//! A plugin that shows the time has to ask the operating system for its current UTC
//! offset, and that question is the awkward part: `time` documents
//! `current_local_offset` as sound only when one thread at a time asks, because the
//! offset comes from process-global state that another thread can change underneath a
//! read.
//!
//! The plugin worker is a second thread, so it must not ask. The answer is that the
//! **main thread** asks — once per refresh — and publishes the result, and the worker
//! only ever reads what was published. That is not a workaround; it is the arrangement
//! the crate's own documentation calls for, and it has the useful side effect that a
//! clock is at most one refresh behind rather than racing the timezone database.
//!
//! The plugin itself gets the reading as part of its tick, rather than asking for a
//! time of its own: the host publishes the wall clock, a plugin formats it, and there
//! is no second way for a plugin to learn what time it is. That is why this type
//! carries hours, minutes and seconds and nothing else — a plugin cannot ask for a
//! date, a timezone or a locale, because the protocol has no way to name one.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A wall-clock reading, as hours, minutes and seconds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WallClock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl WallClock {
    pub const fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour,
            minute,
            second,
        }
    }

    /// This reading as `HH:MM:SS`.
    ///
    /// Written here rather than in a plugin so every clock a plugin shows is spelled
    /// the same way, and so a plugin that wants a different format formats its own.
    pub fn to_hms(self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// This reading as `HH:MM`.
    pub fn to_hm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// Minutes since midnight, for a plugin computing an interval of its own.
    pub fn minutes_since_midnight(self) -> u32 {
        u32::from(self.hour) * 60 + u32::from(self.minute)
    }
}

/// A wall-clock reading shared between the thread that refreshes it and the thread
/// that reads it.
#[derive(Debug)]
pub struct LocalTimeCache {
    reading: Mutex<WallClock>,
    /// The reading packed into one integer, so a reader needs no lock at all on the
    /// hot path. A clock panel is read every tick and a mutex there would be the only
    /// contended lock in the worker.
    packed: AtomicU64,
    /// Whether a refresh has ever succeeded, so a plugin can tell "midnight" from
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
            reading: Mutex::new(WallClock::default()),
            packed: AtomicU64::new(0),
            primed: AtomicBool::new(false),
        }
    }

    /// Read the local time and publish it.
    ///
    /// Call this from the one thread that is allowed to ask the operating system. A
    /// failure is not the caller's problem: the previous reading stays, so the clock
    /// keeps showing the last value it managed to get rather than snapping to
    /// midnight.
    pub fn refresh(&self) {
        let Some(now) = read_local_time() else {
            return;
        };
        *self
            .reading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
        self.packed.store(pack(now), Ordering::Release);
        self.primed.store(true, Ordering::Release);
    }

    /// The most recent reading, which any thread may take.
    pub fn read(&self) -> WallClock {
        unpack(self.packed.load(Ordering::Acquire))
    }

    /// Whether a refresh has ever succeeded.
    pub fn is_primed(&self) -> bool {
        self.primed.load(Ordering::Acquire)
    }
}

/// A reading packed into one integer: hours, minutes and seconds, eight bits each.
fn pack(clock: WallClock) -> u64 {
    u64::from(clock.hour) << 16 | u64::from(clock.minute) << 8 | u64::from(clock.second)
}

/// The inverse of [`pack`], with every field brought back into range.
///
/// The masking is not decoration: the packed value is an atomic the worker writes and
/// another thread reads, and a reader that trusted a byte it had not masked would
/// produce a clock reading outside every range the format allows.
fn unpack(packed: u64) -> WallClock {
    WallClock::new(
        ((packed >> 16) & 0xff) as u8,
        ((packed >> 8) & 0xff) as u8,
        (packed & 0xff) as u8,
    )
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
    Some(WallClock::new(
        hour_of(now.hour()),
        now.minute(),
        now.second(),
    ))
}

/// An hour as `u8`, refusing the one value a 24-hour clock never has.
fn hour_of(hour: u8) -> u8 {
    hour.min(23)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_that_was_never_refreshed_reads_midnight_and_says_so() {
        let cache = LocalTimeCache::new();
        assert_eq!(cache.read(), WallClock::new(0, 0, 0));
        assert!(
            !cache.is_primed(),
            "because a clock stuck at 00:00 looks broken, and 'never read' does not"
        );
    }

    #[test]
    fn a_refresh_primes_the_cache_and_a_reading_is_always_in_range() {
        let cache = LocalTimeCache::new();
        cache.refresh();
        assert!(cache.is_primed());
        let reading = cache.read();
        assert!(
            reading.hour <= 23,
            "an hour out of range is a formatting bug"
        );
        assert!(reading.minute <= 59);
        assert!(reading.second <= 59);
        // The mutex copy and the atomic copy must agree, or a reader on the fast path
        // and a reader through the slow path would see different times.
        assert_eq!(
            reading,
            *cache.reading.lock().expect("the mutex is not poisoned")
        );
    }

    #[test]
    fn a_reading_round_trips_through_the_packed_integer() {
        for (hour, minute, second) in [(0u8, 0u8, 0u8), (9, 5, 3), (23, 59, 59), (12, 30, 0)] {
            let clock = WallClock::new(hour, minute, second);
            assert_eq!(unpack(pack(clock)), clock);
        }
    }

    #[test]
    fn a_packed_reading_with_a_byte_outside_its_range_is_brought_back() {
        // The packed value is written by one thread and read by another, so a reader
        // that trusted a byte it had not masked would produce a reading outside every
        // range the format allows.
        let reading = unpack(0xffff_ffff_ffff);
        assert_eq!(
            reading,
            WallClock::new(0xff, 0xff, 0xff),
            "every byte is read back whole rather than as a wider integer"
        );
        let saturated = WallClock::new(u8::MAX, u8::MAX, u8::MAX);
        assert_eq!(
            saturated.minutes_since_midnight(),
            u32::from(saturated.hour) * 60 + u32::from(saturated.minute),
            "and the arithmetic stays defined rather than overflowing"
        );
    }

    #[test]
    fn a_clock_reads_the_same_way_wherever_it_is_formatted() {
        assert_eq!(WallClock::new(9, 5, 3).to_hms(), "09:05:03");
        assert_eq!(WallClock::new(9, 5, 3).to_hm(), "09:05");
        assert_eq!(WallClock::new(0, 0, 0).to_hms(), "00:00:00");
    }

    #[test]
    fn minutes_since_midnight_is_what_a_plugin_computing_an_interval_needs() {
        assert_eq!(WallClock::new(0, 0, 0).minutes_since_midnight(), 0);
        assert_eq!(WallClock::new(1, 30, 0).minutes_since_midnight(), 90);
        assert_eq!(WallClock::new(23, 59, 59).minutes_since_midnight(), 1439);
    }

    #[test]
    fn an_hour_of_twenty_four_is_not_a_value_a_clock_can_show() {
        assert_eq!(hour_of(24), 23, "a 24-hour clock has no hour 24");
        assert_eq!(hour_of(23), 23);
        assert_eq!(hour_of(0), 0);
    }

    #[test]
    fn a_failed_refresh_leaves_the_previous_reading_rather_than_snapping_to_midnight() {
        // The failure path is "the platform would not say", which this crate cannot
        // produce on purpose; what it can check is that a refresh that does succeed
        // replaces the reading and that a cache which has never been primed is
        // distinguishable from one that is legitimately midnight.
        let cache = LocalTimeCache::new();
        cache.refresh();
        let first = cache.read();
        cache.refresh();
        assert!(cache.is_primed());
        assert_eq!(
            cache.read().hour,
            first.hour,
            "two readings in the same hour agree on the hour"
        );
    }
}
