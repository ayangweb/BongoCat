use crate::{ClipboardError, clipboard::validate_text};
use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

pub(crate) fn read_text() -> Result<Option<String>, ClipboardError> {
    let _marker = MainThreadMarker::new().ok_or(ClipboardError::WrongThread)?;
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        // SAFETY: AppKit exposes this process-lifetime immutable pasteboard type constant.
        let string_type = unsafe { NSPasteboardTypeString };
        let Some(value) = pasteboard.stringForType(string_type) else {
            return Ok(None);
        };
        if value.len() > super::clipboard::MAX_CLIPBOARD_TEXT_BYTES {
            return Err(ClipboardError::TextTooLarge);
        }
        let text = value.to_string();
        validate_text(&text)?;
        Ok(Some(text))
    })
}

pub(crate) fn write_text(value: &str) -> Result<(), ClipboardError> {
    let _marker = MainThreadMarker::new().ok_or(ClipboardError::WrongThread)?;
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        let text = NSString::from_str(value);
        // `_marker` proves these AppKit calls stay on the process main thread. The retained
        // pasteboard and NSString values are confined to this autorelease pool and do not escape.
        // SAFETY: AppKit exposes this process-lifetime immutable pasteboard type constant.
        let string_type = unsafe { NSPasteboardTypeString };
        pasteboard.clearContents();
        pasteboard
            .setString_forType(&text, string_type)
            .then_some(())
            .ok_or(ClipboardError::WriteFailed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasteboard_rejects_background_threads_before_touching_appkit() {
        let read_error = std::thread::spawn(read_text)
            .join()
            .expect("clipboard reader thread")
            .expect_err("background clipboard read");
        assert_eq!(read_error, ClipboardError::WrongThread);

        let write_error = std::thread::spawn(|| write_text("isolated test"))
            .join()
            .expect("clipboard writer thread")
            .expect_err("background clipboard write");
        assert_eq!(write_error, ClipboardError::WrongThread);
    }
}
