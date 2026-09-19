use std::fmt;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use arboard::Clipboard;
#[cfg(target_os = "macos")]
use objc2::{MainThreadMarker, rc::autoreleasepool};

pub(crate) const MAX_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardError {
    UnsupportedPlatform,
    WrongThread,
    TextTooLarge,
    InvalidText,
    ReadFailed,
    WriteFailed,
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedPlatform => "clipboard_unsupported_platform",
            Self::WrongThread => "clipboard_wrong_thread",
            Self::TextTooLarge => "clipboard_text_too_large",
            Self::InvalidText => "clipboard_text_invalid",
            Self::ReadFailed => "clipboard_read_failed",
            Self::WriteFailed => "clipboard_write_failed",
        })
    }
}

impl std::error::Error for ClipboardError {}

pub fn read_clipboard_text() -> Result<Option<String>, ClipboardError> {
    #[cfg(target_os = "macos")]
    {
        let _marker = MainThreadMarker::new().ok_or(ClipboardError::WrongThread)?;
        autoreleasepool(|_| read_text())
    }

    #[cfg(target_os = "windows")]
    {
        read_text()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(ClipboardError::UnsupportedPlatform)
    }
}

pub fn write_clipboard_text(value: &str) -> Result<(), ClipboardError> {
    validate_text(value)?;

    #[cfg(target_os = "macos")]
    {
        let _marker = MainThreadMarker::new().ok_or(ClipboardError::WrongThread)?;
        autoreleasepool(|_| write_text(value))
    }

    #[cfg(target_os = "windows")]
    {
        write_text(value)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = value;
        Err(ClipboardError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn read_text() -> Result<Option<String>, ClipboardError> {
    let mut clipboard = Clipboard::new().map_err(|_| ClipboardError::ReadFailed)?;
    match clipboard.get_text() {
        Ok(text) => {
            validate_text(&text)?;
            Ok(Some(text))
        }
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(_) => Err(ClipboardError::ReadFailed),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn write_text(value: &str) -> Result<(), ClipboardError> {
    let mut clipboard = Clipboard::new().map_err(|_| ClipboardError::WriteFailed)?;
    clipboard
        .set_text(value)
        .map_err(|_| ClipboardError::WriteFailed)
}

pub(crate) fn validate_text(value: &str) -> Result<(), ClipboardError> {
    if value.len() > MAX_CLIPBOARD_TEXT_BYTES {
        return Err(ClipboardError::TextTooLarge);
    }
    if value.contains('\0') {
        return Err(ClipboardError::InvalidText);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_validation_accepts_unicode_at_the_limit() {
        let value = "a".repeat(MAX_CLIPBOARD_TEXT_BYTES - "猫".len()) + "猫";
        assert_eq!(value.len(), MAX_CLIPBOARD_TEXT_BYTES);
        assert_eq!(validate_text(&value), Ok(()));
    }

    #[test]
    fn text_validation_rejects_oversize_and_embedded_nul_without_echoing_text() {
        assert_eq!(
            validate_text(&"a".repeat(MAX_CLIPBOARD_TEXT_BYTES + 1)),
            Err(ClipboardError::TextTooLarge)
        );
        assert_eq!(
            validate_text("before\0after"),
            Err(ClipboardError::InvalidText)
        );
        assert!(!ClipboardError::InvalidText.to_string().contains("before"));
    }

    #[test]
    fn errors_have_stable_anonymous_codes() {
        assert_eq!(
            ClipboardError::WrongThread.to_string(),
            "clipboard_wrong_thread"
        );
        assert_eq!(
            ClipboardError::ReadFailed.to_string(),
            "clipboard_read_failed"
        );
        assert_eq!(
            ClipboardError::WriteFailed.to_string(),
            "clipboard_write_failed"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn clipboard_rejects_background_threads_before_touching_appkit() {
        let read_error = std::thread::spawn(read_clipboard_text)
            .join()
            .expect("clipboard reader thread")
            .expect_err("background clipboard read");
        assert_eq!(read_error, ClipboardError::WrongThread);

        let write_error = std::thread::spawn(|| write_clipboard_text("isolated test"))
            .join()
            .expect("clipboard writer thread")
            .expect_err("background clipboard write");
        assert_eq!(write_error, ClipboardError::WrongThread);
    }
}
