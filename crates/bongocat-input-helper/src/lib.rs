//! Privileged input mode linked into the application binary.
#[cfg(target_os = "linux")]
mod linux;
pub mod protocol;
#[cfg(target_os = "linux")]
pub use linux::run;

#[cfg(target_os = "linux")]
pub fn is_root() -> bool {
    // SAFETY: geteuid has no pointer arguments or side effects.
    unsafe { libc::geteuid() == 0 }
}
