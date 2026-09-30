//! Where the keyboard input method is remembered between the thread that may ask and the
//! thread that may not.
//!
//! The same arrangement as [`LocalTimeCache`](crate::LocalTimeCache), and for the same
//! reason: `TISCopyCurrentKeyboardInputSource` is not thread-safe, and two threads calling
//! it at once aborts inside Core Foundation with no message. It is measured rather than
//! assumed — eight threads calling it in a loop abort on every run, and one thread calling
//! it twice never does — so it is a main-thread-only read, and this is where the answer is
//! published for the worker to read.
//!
//! Unlike the clock, the reading is *not* taken here. Taking it would mean this crate
//! depending on `bongocat-platform`, and the plugin host is deliberately free of platform
//! code: the product owns the framework, so the product asks, and this holds the answer.

use bongocat_plugin_protocol::InputMethod;
use std::sync::Mutex;

/// The most recently published input method.
///
/// One `Mutex` and no atomics, because the value is two strings and a flag — there is
/// nothing to pack, and a reader that takes a lock once per tick for a value that changes
/// when a person switches keyboards is not a hot path.
#[derive(Debug, Default)]
pub struct InputMethodCache {
    current: Mutex<Option<InputMethod>>,
}

impl InputMethodCache {
    /// A cache that has never been told anything.
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish the answer, or the fact that there is none.
    ///
    /// `None` is published as `None` rather than kept as a previous answer: a platform that
    /// stops reporting an input method has *changed*, and a stale method on the panel would
    /// be a lie about the keyboard.
    pub fn publish(&self, method: Option<InputMethod>) {
        *self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = method;
    }

    /// The most recent answer, which any thread may take.
    pub fn read(&self) -> Option<InputMethod> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Whether the current source types Latin letters directly.
    ///
    /// True when there is no source to speak of, because "there is no input method here"
    /// and "the input method types something other than Latin" are not the same answer and
    /// only one of them is a reason to show an indicator.
    pub fn types_latin(&self) -> bool {
        self.read().is_none_or(|method| method.types_latin())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(id: &str, latin: bool) -> InputMethod {
        InputMethod {
            id: id.to_owned(),
            name: id.to_owned(),
            ascii_capable: latin,
        }
    }

    #[test]
    fn a_cache_that_has_never_been_told_says_nothing_rather_than_guessing() {
        let cache = InputMethodCache::new();
        assert_eq!(
            cache.read(),
            None,
            "so a panel has nothing to show rather than a wrong name"
        );
        assert!(
            cache.types_latin(),
            "and a system with no input method is not typing anything unusual"
        );
    }

    #[test]
    fn what_was_published_is_what_is_read() {
        let cache = InputMethodCache::new();
        cache.publish(Some(method("com.apple.inputmethod.SCIM.ITABC", false)));
        let read = cache.read().expect("a method");
        assert_eq!(read.id, "com.apple.inputmethod.SCIM.ITABC");
        assert!(!cache.types_latin(), "so a mode indicator knows to appear");
    }

    #[test]
    fn a_source_that_stops_being_reported_is_not_kept_as_a_previous_answer() {
        // A stale method on the panel is a lie about the keyboard, and "the platform went
        // quiet" is a change rather than an absence of one.
        let cache = InputMethodCache::new();
        cache.publish(Some(method("pinyin", false)));
        cache.publish(None);
        assert_eq!(cache.read(), None);
        assert!(cache.types_latin());
    }

    #[test]
    fn switching_methods_is_one_publish_and_one_read() {
        let cache = InputMethodCache::new();
        cache.publish(Some(method("pinyin", false)));
        assert!(!cache.types_latin());
        cache.publish(Some(method("com.apple.keylayout.ABC", true)));
        assert!(
            cache.types_latin(),
            "and back to Latin, which is the other half of a mode indicator"
        );
        assert_eq!(
            cache.read().expect("a method").id,
            "com.apple.keylayout.ABC"
        );
    }
}
