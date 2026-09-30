//! Which keyboard input source macOS is using.
//!
//! # Why this lives in the product and not in a plugin
//!
//! Because the API is a C function in `HIToolbox` and a plugin may not hold an `unsafe`
//! boundary: the plugins workspace forbids `unsafe`, and a plugin that reached for this
//! would have to turn that off to answer a question about the keyboard. So the product
//! reads it — once, on its own thread, in the one crate whose whole job is platform APIs —
//! and hands the answer to plugins as a fact in the host's own protocol vocabulary. What
//! that buys is the property that matters: a plugin that wants to show the input method has
//! to write a plugin, and the product grows no code for it.
//!
//! # What is read, and what is deliberately not
//!
//! Three things, all of them properties of the *input source* rather than of any
//! application:
//!
//! * **`id`** — the source's own identifier, such as `com.apple.inputmethod.SCIM.ITABC`.
//!   Stable, machine-readable, and the same in every language, so it is what a plugin
//!   matches against when it wants "only while a Chinese method is selected".
//! * **`name`** — the localized name the system itself carries, such as `拼音` or `ABC`.
//!   Read from the system's own table rather than from a plugin's, and that is the point:
//!   a plugin would otherwise need a translation table for every input method on earth, and
//!   it would be wrong about all the ones it had not heard of.
//! * **`ascii_capable`** — whether this source types Latin letters directly, from
//!   `kTISPropertyInputSourceIsASCIICapable`. This is the
//!   fact a mode indicator is actually about: it is the difference between "you are typing
//!   English" and "you have a Chinese method selected", and it is a property of the source
//!   rather than a guess from its name.
//!
//! What is not read is anything about *which application* has focus, or what a plugin is
//! doing. A plugin that wanted that would be asking for a different fact, and there is no
//! reason for this one to carry it.

/// The one fact this module produces, in the project's own types.
///
/// No platform type escapes: a `CFStringRef` would be a handle into a framework's memory
/// with a lifetime the caller cannot see, and the plugin protocol has to carry this over a
/// pipe. Everything here is owned, and everything here is a fact rather than a capability.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputMethod {
    /// The source's own identifier, e.g. `com.apple.inputmethod.SCIM.ITABC`.
    pub id: String,
    /// The system's own name for it, in the user's language, e.g. `拼音`.
    pub name: String,
    /// Whether this source types Latin letters directly.
    pub ascii_capable: bool,
}

impl InputMethod {
    /// Whether this source is one that types Latin letters directly.
    ///
    /// The accessor rather than the field, because it is the question a plugin asks and a
    /// question with a name reads better at the call site than a field.
    pub fn types_latin(&self) -> bool {
        self.ascii_capable
    }
}

// ---------------------------------------------------------------------------
// The FFI shim. macOS only, and the only `unsafe` in this module.
// ---------------------------------------------------------------------------

/// The four `HIToolbox` and Core Foundation entry points, and nothing else.
///
/// `TISCopyCurrentKeyboardInputSource` returns a **retained** source — the caller owns a
/// reference and must release it — and `TISGetInputSourceProperty` returns a value the
/// source *owns*. Those are different rules, and confusing them is a crash inside Core
/// Foundation rather than a compile error, which is what the first version of this file
/// did twice. So the copy is released explicitly, and the borrowed property is only ever
/// read.
#[cfg(target_os = "macos")]
mod macos_ffi {
    use std::ffi::c_void;

    unsafe extern "C" {
        /// A retained copy of the current keyboard input source, or null.
        pub fn TISCopyCurrentKeyboardInputSource() -> *mut c_void;
        /// One property of a source, **borrowed** from it, or null.
        pub fn TISGetInputSourceProperty(
            source: *mut c_void,
            property: *const c_void,
        ) -> *mut c_void;
        /// The value of a `CFBooleanRef`.
        pub fn CFBooleanGetValue(boolean: *mut c_void) -> bool;
        /// The length of a `CFStringRef` in UTF-16 code units.
        pub fn CFStringGetLength(string: *mut c_void) -> isize;
        /// Write a `CFStringRef` into a buffer as NUL-terminated UTF-8.
        pub fn CFStringGetCString(
            string: *mut c_void,
            buffer: *mut u8,
            size: isize,
            encoding: u32,
        ) -> bool;
        /// Give up one reference.
        pub fn CFRelease(value: *mut c_void);
    }
}

/// The three property keys, as the framework's own data symbols.
///
/// These are **variables** in `HIToolbox` holding an interned `CFStringRef`, not macros and
/// not strings. Two things follow, and both cost a crash to learn:
///
/// * They cannot be spelled. `CFSTR("kTISPropertyInputSourceID")` builds a *different*
///   string, and the API matches its key by identity rather than by value — so a spelled
///   key simply is not found, and what comes back is not an error but a null, which reads
///   as "this source has no name" rather than as "this key was wrong". The symbols are
///   therefore declared as they are declared in the framework's header: three `extern`
///   statics. Their addresses are the framework's own; this crate reads them and never
///   writes them.
/// * The obvious shorter spelling is not a near miss. The key is
///   `kTISPropertyInputSourceIsASCIICapable`, and a wrong name is a *wrong answer* rather
///   than a refusal.
///
/// Read at every call rather than cached: a `static` of a raw pointer has no sound `Sync`
/// story without one, and the read is a load from a page the framework has already mapped.
#[cfg(target_os = "macos")]
mod keys {
    use std::ffi::c_void;

    unsafe extern "C" {
        /// The key naming a source's identifier.
        pub static kTISPropertyInputSourceID: *const c_void;
        /// The key naming a source's name, in the user's language.
        pub static kTISPropertyLocalizedName: *const c_void;
        /// The key naming whether a source types Latin letters directly.
        pub static kTISPropertyInputSourceIsASCIICapable: *const c_void;
    }
}

/// One string property of an input source, copied out as an owned `String`.
///
/// Two steps, and both are documented accessors on a `CFStringRef` that change no reference
/// count: ask for the length, allocate one byte more, ask for the text, copy it out. The
/// copy is the point — the value belongs to the source, and the source is released before
/// this function's caller could have used it otherwise.
#[cfg(target_os = "macos")]
fn string_property(source: *mut std::ffi::c_void, key: *const std::ffi::c_void) -> Option<String> {
    use macos_ffi::{CFStringGetCString, CFStringGetLength, TISGetInputSourceProperty};

    /// `kCFStringEncodingUTF8`, the encoding `CFStringGetCString` is asked for.
    const UTF8: u32 = 0x0800_0100;

    // SAFETY: the caller holds a live, retained source, and `key` is a live `CFStringRef`
    // for the whole call. The returned value is owned by the source, which outlives it.
    let value = unsafe { TISGetInputSourceProperty(source, key) };
    if value.is_null() {
        return None;
    }
    // SAFETY: the value is a `CFStringRef` when the key names a string property, and
    // `CFStringGetLength` only reads it.
    let length = unsafe { CFStringGetLength(value) };
    if length <= 0 {
        return None;
    }
    // Sized in *bytes*, and that is the whole of the second crash this function had: a
    // UTF-16 code unit is up to three UTF-8 bytes and a surrogate pair is four, so
    // `length + 1` is not an upper bound on the encoded form. For a name such as `微信输入法`
    // the buffer is then a third of the size it needs, `CFStringGetCString` reports failure,
    // and the name silently reads as empty. Four bytes per unit is a bound that cannot be
    // wrong, plus the terminator the call writes.
    let mut buffer = vec![0_u8; (length as usize).saturating_mul(4).saturating_add(1)];
    // SAFETY: the buffer is at least one byte longer than the string, so the call cannot
    // overflow it, and the pointer is a live `CFStringRef` that is only read.
    let written =
        unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) };
    if !written {
        return None;
    }
    let text = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    std::str::from_utf8(&buffer[..text])
        .ok()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// One boolean property of an input source.
#[cfg(target_os = "macos")]
fn bool_property(source: *mut std::ffi::c_void, key: *const std::ffi::c_void) -> Option<bool> {
    use macos_ffi::TISGetInputSourceProperty;

    // SAFETY: as above, for a key that names a `CFBooleanRef` property.
    let value = unsafe { TISGetInputSourceProperty(source, key) };
    if value.is_null() {
        return None;
    }
    // SAFETY: the value is a `CFBooleanRef` and `CFBooleanGetValue` only reads it.
    Some(unsafe { macos_ffi::CFBooleanGetValue(value) })
}

/// The current keyboard input source, or `None` when the system will not say.
///
/// `None` is a normal answer on a machine with no input sources configured, and it is what
/// a caller gets on a platform that has no such concept — a caller that treated it as a
/// failure would have to invent a second meaning for "the keyboard is not there".
#[cfg(target_os = "macos")]
pub fn current_input_method() -> Option<InputMethod> {
    use macos_ffi::{CFRelease, TISCopyCurrentKeyboardInputSource};

    // SAFETY: the function returns a retained source or null, and the reference is released
    // on every path below. The three properties are read into owned values before the
    // release, and none of them outlives this call.
    let source = unsafe { TISCopyCurrentKeyboardInputSource() };
    if source.is_null() {
        return None;
    }
    // SAFETY: the three statics are `const CFStringRef`s inside `HIToolbox`, initialised
    // before `main` and never written afterwards, so reading their values is a load of a
    // live framework object. Each is read once, on the thread this function requires.
    let (id, name, ascii_capable) = unsafe {
        let id = string_property(source, keys::kTISPropertyInputSourceID).unwrap_or_default();
        // The localized name is the one worth showing and the one a source may not have, so
        // the identifier stands in for it rather than leaving a panel with nothing in it.
        let name = string_property(source, keys::kTISPropertyLocalizedName)
            .or_else(|| string_property(source, keys::kTISPropertyInputSourceID))
            .unwrap_or_default();
        let ascii_capable =
            bool_property(source, keys::kTISPropertyInputSourceIsASCIICapable).unwrap_or(false);
        (id, name, ascii_capable)
    };
    // SAFETY: one reference was taken by the copy above and this gives it back, once.
    unsafe { CFRelease(source) };
    if id.is_empty() && name.is_empty() {
        return None;
    }
    Some(InputMethod {
        id,
        name,
        ascii_capable,
    })
}

/// The current keyboard input source on a platform that has no such concept.
#[cfg(not(target_os = "macos"))]
pub fn current_input_method() -> Option<InputMethod> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_method_that_types_latin_says_so() {
        let method = InputMethod {
            id: "com.apple.keylayout.ABC".to_owned(),
            name: "ABC".to_owned(),
            ascii_capable: true,
        };
        assert!(method.types_latin());
        let chinese = InputMethod {
            id: "com.apple.inputmethod.SCIM.ITABC".to_owned(),
            name: "拼音".to_owned(),
            ascii_capable: false,
        };
        assert!(
            !chinese.types_latin(),
            "because the difference between the two is the whole reason a mode indicator exists"
        );
    }

    #[test]
    fn the_real_system_answers_twice_in_a_row_on_one_thread() {
        // The one test that touches the real input source, and it is deliberately the only
        // one: the API aborts when two threads call it at once, and a test suite that ran
        // two of these in parallel would fail for a reason that has nothing to do with the
        // code under test. So the product's actual pattern — one thread, two readings in a
        // row — is what is checked here.
        let first = current_input_method();
        let second = current_input_method();
        if cfg!(not(target_os = "macos")) {
            assert_eq!(
                first, None,
                "a platform with no such concept has nothing to say"
            );
            return;
        }
        assert_eq!(
            first.is_none(),
            second.is_none(),
            "so whether there is a source at all is stable across two reads, which is what \
             lets the caller decide whether to cache it"
        );
        if let Some(method) = &first {
            // The two failures this function has actually had, both of which read as "no
            // source" rather than as an error, so they are pinned here against a real
            // system rather than only against a mock:
            //
            // * a property key spelled rather than declared, which the API does not match
            //   because it compares keys by identity;
            // * a buffer sized in UTF-16 units, which is a third of what `微信输入法` needs
            //   and therefore reports the name as empty.
            assert!(
                !method.id.is_empty(),
                "a source that will not name itself is not a source: {method:?}"
            );
            assert!(
                !method.name.is_empty(),
                "and the name falls back to the identifier rather than to nothing, because a \
                 panel with a blank in it is worse than one with an identifier in it: \
                 {method:?}"
            );
            assert_eq!(
                Some(method),
                second.as_ref(),
                "and two readings of an unchanged keyboard agree, because the value is \
                 republished on a timer and a caller must not see it flicker"
            );
        }
    }
}
