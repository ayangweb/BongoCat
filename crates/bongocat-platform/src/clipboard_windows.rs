use crate::{ClipboardError, clipboard::validate_text};
use std::{mem::size_of, ptr, slice};
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};

struct ClipboardLease;

impl ClipboardLease {
    fn open() -> Result<Self, ClipboardError> {
        // SAFETY: a null owner is the documented process-independent clipboard access form;
        // this guard closes exactly the successfully opened clipboard on the same thread.
        unsafe { OpenClipboard(None) }.map_err(|_| ClipboardError::ReadFailed)?;
        Ok(Self)
    }
}

impl Drop for ClipboardLease {
    fn drop(&mut self) {
        // SAFETY: this guard is constructed only after OpenClipboard succeeded on this thread.
        let _ = unsafe { CloseClipboard() };
    }
}

struct ClipboardMemory(Option<HGLOBAL>);

impl ClipboardMemory {
    fn allocate(byte_len: usize) -> Result<Self, ClipboardError> {
        // SAFETY: GMEM_MOVEABLE is the documented allocation class required by SetClipboardData;
        // the returned HGLOBAL is owned by this guard until successful ownership transfer.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }
            .map_err(|_| ClipboardError::WriteFailed)?;
        Ok(Self(Some(handle)))
    }

    fn transfer_to_clipboard(&mut self) -> Option<HGLOBAL> {
        self.0.take()
    }
}

impl Drop for ClipboardMemory {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // SAFETY: this guard owns the HGLOBAL until SetClipboardData succeeds. GlobalFree's
            // generated Result treats its documented null success value as an error, so it is ignored.
            let _ = unsafe { GlobalFree(Some(handle)) };
        }
    }
}

pub(crate) fn read_text() -> Result<Option<String>, ClipboardError> {
    let _clipboard = ClipboardLease::open().map_err(|_| ClipboardError::ReadFailed)?;
    // SAFETY: the clipboard is held by `_clipboard` for the availability query.
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT.0.into()) }.is_err() {
        return Ok(None);
    }
    // SAFETY: the clipboard remains open and the returned handle remains owned by the system.
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0.into()) }
        .map_err(|_| ClipboardError::ReadFailed)?;
    let global = HGLOBAL(handle.0);
    // SAFETY: `global` came from CF_UNICODETEXT while the clipboard is open; GlobalSize reports
    // its current allocation length without transferring ownership.
    let byte_len = unsafe { GlobalSize(global) };
    if byte_len < size_of::<u16>() || byte_len % size_of::<u16>() != 0 {
        return Err(ClipboardError::ReadFailed);
    }
    if byte_len > (super::clipboard::MAX_CLIPBOARD_TEXT_BYTES + 1) * size_of::<u16>() {
        return Err(ClipboardError::TextTooLarge);
    }
    // SAFETY: the clipboard remains open, `global` is a valid CF_UNICODETEXT HGLOBAL, and the
    // allocation length was checked before the pointer is converted to a u16 slice.
    let pointer = unsafe { GlobalLock(global) }.cast::<u16>();
    if pointer.is_null() {
        return Err(ClipboardError::ReadFailed);
    }
    // SAFETY: GlobalLock returned a non-null pointer to `byte_len / 2` UTF-16 code units.
    let units = unsafe { slice::from_raw_parts(pointer, byte_len / size_of::<u16>()) };
    let Some(terminator) = units.iter().position(|unit| *unit == 0) else {
        // SAFETY: this balances the successful GlobalLock before returning.
        let _ = unsafe { GlobalUnlock(global) };
        return Err(ClipboardError::ReadFailed);
    };
    let text = String::from_utf16(&units[..terminator]).map_err(|_| ClipboardError::InvalidText);
    // SAFETY: this balances the successful GlobalLock. A final unlock returns false by design,
    // so the generated Result is intentionally ignored.
    let _ = unsafe { GlobalUnlock(global) };
    let text = text?;
    validate_text(&text)?;
    Ok(Some(text))
}

pub(crate) fn write_text(value: &str) -> Result<(), ClipboardError> {
    let utf16 = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let byte_len = utf16
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or(ClipboardError::TextTooLarge)?;
    let mut memory = ClipboardMemory::allocate(byte_len)?;
    let handle = memory.0.expect("allocated clipboard memory");
    // SAFETY: `handle` owns an allocation of `byte_len` bytes, and `utf16` has exactly that many
    // initialized bytes. The copied data remains valid after the source Vec is dropped.
    let destination = unsafe { GlobalLock(handle) }.cast::<u16>();
    if destination.is_null() {
        return Err(ClipboardError::WriteFailed);
    }
    // SAFETY: destination points to `utf16.len()` writable u16 values in the locked allocation;
    // source and destination are distinct allocations.
    unsafe { ptr::copy_nonoverlapping(utf16.as_ptr(), destination, utf16.len()) };
    // SAFETY: this balances the successful GlobalLock before the HGLOBAL changes ownership.
    let _ = unsafe { GlobalUnlock(handle) };

    let _clipboard = ClipboardLease::open().map_err(|_| ClipboardError::WriteFailed)?;
    // SAFETY: `_clipboard` holds exclusive clipboard access on this thread.
    unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::WriteFailed)?;
    let transferred = memory
        .transfer_to_clipboard()
        .expect("clipboard memory remains owned before transfer");
    // SAFETY: CF_UNICODETEXT requires a GMEM_MOVEABLE, null-terminated UTF-16 HGLOBAL. On
    // success the system owns `transferred`; on failure the guard must retain ownership.
    match unsafe { SetClipboardData(CF_UNICODETEXT.0.into(), Some(HANDLE(transferred.0))) } {
        Ok(_) => Ok(()),
        Err(_) => {
            memory.0 = Some(transferred);
            Err(ClipboardError::WriteFailed)
        }
    }
}
