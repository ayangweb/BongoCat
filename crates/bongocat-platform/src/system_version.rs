//! The version of the operating system this build is running on.
//!
//! The product knows its own version at compile time, but almost nothing about
//! a bug report can be acted on without the system it came from: a rendering
//! difference, a Raw Input behaviour and a TCC decision all depend on the exact
//! macOS or Windows build, and none of them can be inferred from the app's own
//! version. This module is the one place that asks the operating system.
//!
//! Both platforms answer the same two questions in their own notation, and
//! neither notation is rewritten into the other's:
//!
//! | | macOS | Windows |
//! | --- | --- | --- |
//! | [`version`](OperatingSystemVersion::version) | `kern.osproductversion` — "15.6" | major/minor under `CurrentVersion` — "10.0" |
//! | [`build`](OperatingSystemVersion::build) | `kern.osversion` — "24G90" | `CurrentBuildNumber` + `UBR` — "22631.4169" |
//!
//! macOS is read through `sysctlbyname` rather than `NSProcessInfo` on purpose:
//! `operatingSystemVersion` answers with the compatibility version a process
//! linked against an older SDK sees, while the kernel's own `kern.os*` keys
//! always name the system the process is running on. The build is read from
//! `kern.osversion` rather than the older `kern.osbuildversion`: both carry the
//! same build identifier where both exist, and `kern.osbuildversion` is absent
//! on current macOS releases, so the key that is still there is the one to ask.
//! Windows is read from
//! `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` because the supported
//! `RtlGetVersion` reports no update build number, and "which patch level" is
//! exactly what a rendering or Raw Input report has to carry. The key is read
//! through the 64-bit view because the product only ships an `x86_64` binary,
//! so nothing redirects it to `WOW6432Node`.

/// The running operating system's version, in the notation its own API uses.
///
/// Both fields are kept separate because they answer different questions: the
/// version is the one a release note and a help article name, while the build
/// is the only form that pins one exact installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatingSystemVersion {
    /// The version the platform itself reports: "15.6" on macOS, "10.0" on Windows.
    pub version: String,
    /// The build identity: "24G90" on macOS, "22631.4169" on Windows.
    pub build: String,
}

/// The version of the operating system this process is running on.
///
/// `None` when the platform cannot report one. That is not a failure the caller
/// has to handle: a system that does not name its own version still produces a
/// usable bug report, so the caller reports the version it does have and leaves
/// this out rather than inventing a value. A version that is present is never
/// empty and never carries a terminator — the platform's own separators decide
/// its shape.
pub fn operating_system_version() -> Option<OperatingSystemVersion> {
    platform::version()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::OperatingSystemVersion;
    use std::{ffi::CString, ptr};

    /// The two kernel keys the marketing version and the build identifier live
    /// under. They are `char` sysctls, so each one is a NUL-terminated string.
    const PRODUCT_VERSION_KEY: &str = "kern.osproductversion";
    const BUILD_VERSION_KEY: &str = "kern.osversion";

    pub(super) fn version() -> Option<OperatingSystemVersion> {
        Some(OperatingSystemVersion {
            version: sysctl_string(PRODUCT_VERSION_KEY)?,
            build: sysctl_string(BUILD_VERSION_KEY)?,
        })
    }

    /// Read one NUL-terminated `char` sysctl as a Rust `String`.
    ///
    /// `sysctlbyname` is read in its documented two-call form: a call with a null
    /// value pointer reports the buffer size it needs, and a second call fills
    /// that buffer. Reporting no value is the answer for every way the two calls
    /// can disagree — a key the running system does not have, a size that grows
    /// between the calls, a value that is not valid UTF-8 — because a truncated
    /// or unterminated build identifier is worse than a missing one.
    ///
    /// The key is copied into a [`CString`] rather than passed as a `&str`
    /// pointer: `sysctlbyname` reads a C string, and a `&str` is not
    /// NUL-terminated, so the name it would read runs into whatever follows it.
    fn sysctl_string(key: &str) -> Option<String> {
        let key = CString::new(key).ok()?;
        let mut size = 0;
        // SAFETY: `key` is a NUL-terminated name that outlives the call, and a
        // null value pointer with a null new value asks `sysctlbyname` to write
        // the required buffer size through `size` and change nothing else.
        let queried = unsafe {
            libc::sysctlbyname(
                key.as_ptr(),
                ptr::null_mut(),
                ptr::from_mut(&mut size),
                ptr::null_mut(),
                0,
            )
        };
        if queried != 0 || size == 0 {
            return None;
        }
        let mut buffer = vec![0_u8; size];
        // SAFETY: `buffer` is a live allocation of the `size` writable bytes the
        // same key just asked for, and `size` is a live `size_t` the call writes
        // the byte count it stored through. The value pointer is null, so the
        // call cannot write to `key` or anywhere else. A `char` sysctl stores
        // bytes rather than `c_char`s, so the buffer is read back as bytes.
        let read = unsafe {
            libc::sysctlbyname(
                key.as_ptr(),
                buffer.as_mut_ptr().cast(),
                ptr::from_mut(&mut size),
                ptr::null_mut(),
                0,
            )
        };
        if read != 0 {
            return None;
        }
        // The stored byte count covers the NUL terminator. A shorter answer than
        // the first call promised is a value that changed underneath both
        // calls, and is reported as no value for the same reason.
        let stored = size.min(buffer.len());
        let bytes = &buffer[..stored];
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        String::from_utf8(bytes[..end].to_vec()).ok()
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::OperatingSystemVersion;
    use std::{ffi::c_void, mem::size_of, ptr::from_mut};
    use windows::{
        Win32::System::Registry::{
            HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
        },
        core::w,
    };

    /// The key Windows itself records the running build under.
    const CURRENT_VERSION_KEY: windows::core::PCWSTR =
        w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    const CURRENT_BUILD_KEY: windows::core::PCWSTR = w!("CurrentBuildNumber");
    const UPDATE_BUILD_KEY: windows::core::PCWSTR = w!("UBR");
    const MAJOR_VERSION_KEY: windows::core::PCWSTR = w!("CurrentMajorVersionNumber");
    const MINOR_VERSION_KEY: windows::core::PCWSTR = w!("CurrentMinorVersionNumber");
    /// A value name, not a version, is bounded by the registry itself; this only
    /// has to be large enough for `CurrentBuildNumber`, and is not a limit the
    /// product imposes on the system.
    const MAX_VALUE_BYTES: u32 = 64;

    pub(super) fn version() -> Option<OperatingSystemVersion> {
        let major = read_dword(MAJOR_VERSION_KEY)?;
        let minor = read_dword(MINOR_VERSION_KEY)?;
        let build = read_string(CURRENT_BUILD_KEY)?;
        // `UBR` is absent on builds that never recorded an update build number.
        // The build identifier then stays the build alone rather than ending in
        // a `.0` that no system ever reported.
        let build = match read_dword(UPDATE_BUILD_KEY) {
            Some(update_build) => format!("{build}.{update_build}"),
            None => build,
        };
        Some(OperatingSystemVersion {
            version: format!("{major}.{minor}"),
            build,
        })
    }

    fn read_dword(name: windows::core::PCWSTR) -> Option<u32> {
        let mut value = 0_u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: the call reads one `REG_DWORD` from a preopened key into the
        // out pointer below, which is a live `u32` for the duration of the call,
        // and `size` carries its size. The type filter rejects every other value
        // type, so nothing else is written.
        let read = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                CURRENT_VERSION_KEY,
                name,
                RRF_RT_REG_DWORD,
                None,
                Some(from_mut(&mut value).cast::<c_void>()),
                Some(&mut size),
            )
        };
        // `RegGetValueW` answers with a `WIN32_ERROR` rather than a `Result`, so
        // a missing value is `ERROR_FILE_NOT_FOUND` instead of a `Result::Err`.
        read.is_ok().then_some(value)
    }

    fn read_string(name: windows::core::PCWSTR) -> Option<String> {
        let mut buffer = [0_u16; MAX_VALUE_BYTES as usize];
        let mut size = size_of_val(&buffer) as u32;
        // SAFETY: the call reads one `REG_SZ` from a preopened key into the out
        // pointer below, which is a live array of the `size` bytes `size` claims,
        // and `size` is a live `u32` the call writes the stored byte count
        // through. The type filter rejects every other value type, and a value
        // larger than the buffer is reported as a failure rather than truncated.
        let read = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                CURRENT_VERSION_KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(from_mut(&mut buffer).cast::<c_void>()),
                Some(&mut size),
            )
        };
        if !read.is_ok() {
            return None;
        }
        // The stored byte count includes the NUL terminator, and a value written
        // by the system always ends in one; an unterminated read is reported as
        // no value so no caller ever sees half a wide character.
        let units = size as usize / size_of::<u16>();
        let units = units.min(buffer.len());
        let end = buffer[..units]
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units);
        String::from_utf16(&buffer[..end]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::operating_system_version;

    /// Both supported platforms have to answer, not just the one this test runs
    /// on: a system that names no version produces a report that is missing the
    /// one field a triage reads first, and nothing else would notice. The `None`
    /// return is for a system that answers unreliably, not for a platform that
    /// has no way to answer — and this module only compiles where there is one.
    #[test]
    fn the_running_system_reports_a_complete_version() {
        let version = operating_system_version()
            .expect("both supported platforms report a version, and this build is one of them");
        assert!(!version.version.is_empty());
        assert!(!version.build.is_empty());
        for value in [&version.version, &version.build] {
            assert!(!value.contains('\0'), "a terminator leaked into {value}");
            assert!(
                value
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric()
                        || matches!(character, '.' | '_' | '-')),
                "{value} is not a plain version string"
            );
        }
    }

    /// The two fields answer different questions, so a value that is present in
    /// both would mean one of them is a restatement of the other. A system whose
    /// build happens to equal its version is not one to trust either way.
    #[test]
    fn the_version_and_the_build_are_different_facts() {
        if let Some(version) = operating_system_version() {
            assert_ne!(version.version, version.build);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    pub fn version() -> Option<super::OperatingSystemVersion> {
        let release = std::fs::read_to_string("/etc/os-release").ok()?;
        let version = release
            .lines()
            .find_map(|line| line.strip_prefix("PRETTY_NAME="))
            .map(|s| s.trim_matches('"').to_owned())?;
        let build = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()?
            .trim()
            .to_owned();
        Some(super::OperatingSystemVersion { version, build })
    }
}
