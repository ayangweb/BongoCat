//! Turning a Cubism count into a Rust length, or refusing to.
//!
//! Every read out of the native library goes through one of these. A count is an
//! `i32` the library produced, and a negative or absurd one has to be refused at
//! the boundary rather than wrapped into a `usize` — a wrapped length is a
//! read of memory the process does not own.

use super::*;

pub(crate) fn nonnegative(value: i32, name: &str) -> Result<usize, Live2dError> {
    usize::try_from(value).map_err(|_| {
        Live2dError::new(
            Live2dErrorCode::InvalidCoreValue,
            format!("Core returned a negative {name}"),
        )
    })
}

pub(crate) unsafe fn checked_slice<'a, T>(
    pointer: *const T,
    count: usize,
    name: &str,
) -> Result<&'a [T], Live2dError> {
    if count == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreArray,
            format!("Core returned a null {name} array for {count} values"),
        ));
    }
    // SAFETY: the Core owns at least count elements for this pointer/count
    // pair while the caller's Model owner remains alive.
    Ok(unsafe { std::slice::from_raw_parts(pointer, count) })
}

pub(crate) unsafe fn checked_slice_mut<'a, T>(
    pointer: *mut T,
    count: usize,
    name: &str,
) -> Result<&'a mut [T], Live2dError> {
    if count == 0 {
        return Ok(&mut []);
    }
    if pointer.is_null() {
        return Err(Live2dError::new(
            Live2dErrorCode::InvalidCoreArray,
            format!("Core returned a null {name} array for {count} values"),
        ));
    }
    // SAFETY: the Core owns at least count uniquely writable elements for
    // this pointer/count pair while the caller's Model owner remains alive.
    Ok(unsafe { std::slice::from_raw_parts_mut(pointer, count) })
}
