//! The product's native file dialogs: a model folder, a model's cover image, and an
//! audio file a plugin asked to be able to play.
//!
//! They are one module because they are one capability — the host's own file panel, on
//! the two platforms that have one, off the thread that is allowed to open it — and
//! three near-identical copies of it would be three places to fix the threading.
//!
//! A model reaches the product as the folder a user exported, and choosing that
//! folder is the only decision the source picker asks for: both supported
//! platforms open their own folder panel, so there is no mode for the user to
//! pick before they can browse.
//!
//! A model *archive* is not offered here, and nothing else reads one either: the
//! store's archive source was removed with its reader, its limits and its
//! diagnostics (ADR-0036 已撤回), so the folder this picker returns is the only
//! shape the rest of the path accepts. Archive import comes back as a feature of
//! its own — see that ADR for the design it would restore.
//!
//! Selection is deliberately *not* where a source is judged. The picker requires
//! a real directory and stops there — the dialog carries no filter on purpose,
//! because what the directory contains is the store's judgement and it reports
//! its own stable diagnostic. The cover picker keeps the same split: it offers
//! PNG in the dialog as a convenience, and the settings service still validates
//! the bytes before they replace an existing cover.

use std::path::PathBuf;

use std::fs;

#[cfg(target_os = "macos")]
use objc2::{MainThreadMarker, rc::autoreleasepool};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSApplication;

/// The file names the cover dialog offers. This is a convenience filter: the
/// settings service validates the selected bytes themselves.
const COVER_EXTENSIONS: [&str; 1] = ["png"];

/// The file names the audio dialog offers.
///
/// A convenience filter in the strict sense, and the comment on the cover extensions
/// applies here too: the dialog narrows what a person scrolls through, and the decoder
/// decides what can actually be played. MP3 and FLAC are the two the product's own
/// decoder is built for, and the rest are offered because a user who has such a file
/// would rather choose it than be told the dialog does not list it.
const AUDIO_EXTENSIONS: [&str; 4] = ["mp3", "flac", "wav", "m4a"];

/// The longest an extension in a dialog filter may be.
///
/// A bound rather than a rule: a file extension is a handful of characters, and a filter
/// carrying a megabyte of them is a declaration the host has to hold rather than show.
const MAXIMUM_EXTENSION_BYTES: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilePickerOutcome {
    Selected(PathBuf),
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FilePickerError {
    #[error("{}", Self::WrongThread.as_str())]
    WrongThread,
    #[error("{}", Self::BackendUnavailable.as_str())]
    BackendUnavailable,
    #[error("{}", Self::SelectionUnavailable.as_str())]
    SelectionUnavailable,
    #[error("{}", Self::SelectionInvalid.as_str())]
    SelectionInvalid,
}

impl FilePickerError {
    pub const ALL: [Self; 4] = [
        Self::WrongThread,
        Self::BackendUnavailable,
        Self::SelectionUnavailable,
        Self::SelectionInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongThread => "model_source_picker_wrong_thread",
            Self::BackendUnavailable => "model_source_picker_backend_unavailable",
            Self::SelectionUnavailable => "model_source_picker_selection_unavailable",
            Self::SelectionInvalid => "model_source_picker_selection_invalid",
        }
    }
}

#[cfg(target_os = "macos")]
fn asynchronous_sheet_is_available(mtm: MainThreadMarker) -> bool {
    let application = NSApplication::sharedApplication(mtm);
    let has_window = application.mainWindow().is_some() || !application.windows().is_empty();
    application.isRunning() && has_window
}

fn spawn_picker_worker<F, P>(on_complete: F, pick: P) -> Result<(), FilePickerError>
where
    F: FnOnce(Result<FilePickerOutcome, FilePickerError>) + Send + 'static,
    P: FnOnce() -> Result<FilePickerOutcome, FilePickerError> + Send + 'static,
{
    std::thread::Builder::new()
        .name("bongocat-model-source-picker".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(pick))
                .unwrap_or(Err(FilePickerError::BackendUnavailable));
            on_complete(result);
        })
        .map(|_| ())
        .map_err(|_| FilePickerError::BackendUnavailable)
}

/// Let the user choose the model folder to import.
#[cfg(target_os = "macos")]
pub(crate) fn pick_model_folder<F>(on_complete: F) -> Result<(), FilePickerError>
where
    F: FnOnce(Result<FilePickerOutcome, FilePickerError>) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(FilePickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        // `rfd` falls back to a synchronous `runModal` when no sheet parent exists. That reenters
        // GPUI's event loop and previously caused `RefCell already borrowed`, so reject the call.
        return Err(FilePickerError::BackendUnavailable);
    }

    let task = autoreleasepool(|_| {
        rfd::AsyncFileDialog::new()
            .set_can_create_directories(false)
            .pick_folder()
    });
    spawn_picker_worker(on_complete, move || {
        // `rfd` maps both cancellation and backend failure to `None`; cancellation is the only
        // outcome the public API can represent without inventing information.
        match async_io::block_on(task) {
            Some(handle) => validate_selected_folder(handle.path().to_path_buf()),
            None => Ok(FilePickerOutcome::Cancelled),
        }
    })
}

/// Let the user choose the model folder to import.
#[cfg(target_os = "windows")]
pub(crate) fn pick_model_folder<F>(on_complete: F) -> Result<(), FilePickerError>
where
    F: FnOnce(Result<FilePickerOutcome, FilePickerError>) + Send + 'static,
{
    spawn_picker_worker(on_complete, || {
        // The Windows backend runs the common item dialog on this dedicated worker's STA.
        match rfd::FileDialog::new()
            .set_can_create_directories(false)
            .pick_folder()
        {
            Some(path) => validate_selected_folder(path),
            None => Ok(FilePickerOutcome::Cancelled),
        }
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn pick_model_cover<F>(on_complete: F) -> Result<(), FilePickerError>
where
    F: FnOnce(Result<FilePickerOutcome, FilePickerError>) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(FilePickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        return Err(FilePickerError::BackendUnavailable);
    }

    let task = autoreleasepool(|_| {
        rfd::AsyncFileDialog::new()
            .set_can_create_directories(false)
            .add_filter("Cover image", &COVER_EXTENSIONS)
            .pick_file()
    });
    spawn_picker_worker(on_complete, move || match async_io::block_on(task) {
        Some(handle) => validate_selected_image(handle.path().to_path_buf()),
        None => Ok(FilePickerOutcome::Cancelled),
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn pick_model_cover<F>(on_complete: F) -> Result<(), FilePickerError>
where
    F: FnOnce(Result<FilePickerOutcome, FilePickerError>) + Send + 'static,
{
    spawn_picker_worker(on_complete, || {
        match rfd::FileDialog::new()
            .set_can_create_directories(false)
            .add_filter("Cover image", &COVER_EXTENSIONS)
            .pick_file()
        {
            Some(path) => validate_selected_image(path),
            None => Ok(FilePickerOutcome::Cancelled),
        }
    })
}

/// Open the audio file dialog, for a plugin that declared a file setting.
///
/// The third picker rather than a new capability: the threading, the two platform
/// backends and the validation are the cover picker's, and a plugin's sound file is the
/// same decision about a different kind of file. A plugin names the extensions it wants
/// in its own schema and this is where they become a filter, which keeps the vocabulary
/// in the plugin and the dialog in the host.
pub(crate) fn pick_audio_file<F>(
    extensions: &[String],
    on_complete: F,
) -> Result<(), FilePickerError>
where
    F: FnOnce(Result<FilePickerOutcome, FilePickerError>) + Send + 'static,
{
    // A plugin that declared no extension, or only ones with nothing usable in them, is
    // given the product's own list rather than an unfiltered dialog: an open dialog is a
    // dialog showing everything on the machine, which is not a useful thing to offer
    // somebody who asked for a sound. Normalized, sorted and de-duplicated on the way, so
    // two plugins that both asked for `mp3` and `.MP3` produce the same dialog and a
    // declaration cannot make the filter longer than the list the dialog will take.
    let mut declared: Vec<String> = extensions
        .iter()
        .map(|extension| {
            extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase()
        })
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= MAXIMUM_EXTENSION_BYTES
                && extension
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
        .collect();
    declared.sort();
    declared.dedup();
    if declared.is_empty() {
        declared = AUDIO_EXTENSIONS
            .iter()
            .map(|extension| (*extension).to_owned())
            .collect();
    }
    let offered: Vec<&str> = declared.iter().map(String::as_str).collect();

    #[cfg(target_os = "macos")]
    {
        let mtm = MainThreadMarker::new().ok_or(FilePickerError::WrongThread)?;
        if !asynchronous_sheet_is_available(mtm) {
            return Err(FilePickerError::BackendUnavailable);
        }
        let task = autoreleasepool(|_| {
            rfd::AsyncFileDialog::new()
                .set_can_create_directories(false)
                .add_filter("Audio file", &offered)
                .pick_file()
        });
        spawn_picker_worker(on_complete, move || match async_io::block_on(task) {
            Some(handle) => validate_selected_file(handle.path().to_path_buf()),
            None => Ok(FilePickerOutcome::Cancelled),
        })
    }
    #[cfg(target_os = "windows")]
    {
        spawn_picker_worker(on_complete, || {
            match rfd::FileDialog::new()
                .set_can_create_directories(false)
                .add_filter("Audio file", &offered)
                .pick_file()
            {
                Some(path) => validate_selected_file(path),
                None => Ok(FilePickerOutcome::Cancelled),
            }
        })
    }
}

/// Validate a file a plugin chose.
///
/// "A real regular file at an absolute path" is the whole rule, for the same reason the
/// model pickers stop where they stop: what a plugin may *play* is the host's judgement
/// and it reports its own stable diagnostic. This only refuses a selection that is not a
/// file at all, which is the one case where handing the path on would be handing on
/// something the host has to open to find out about.
pub(crate) fn validate_selected_file(
    selected: PathBuf,
) -> Result<FilePickerOutcome, FilePickerError> {
    let canonical = canonicalize_selection(&selected)?;
    if !canonical.is_file() {
        return Err(FilePickerError::SelectionInvalid);
    }
    Ok(FilePickerOutcome::Selected(canonical))
}

/// Validate a selected model folder.
///
/// "A real directory" is the whole rule. Which model package that directory
/// contains, and whether it contains one at all, stays the store's judgement —
/// it reads the bytes and reports its own stable diagnostic — so nothing about
/// the folder's contents is decided at the dialog layer.
pub(crate) fn validate_selected_folder(
    selected: PathBuf,
) -> Result<FilePickerOutcome, FilePickerError> {
    let canonical = canonicalize_selection(&selected)?;
    if !canonical.is_dir() {
        return Err(FilePickerError::SelectionInvalid);
    }
    Ok(FilePickerOutcome::Selected(canonical))
}

/// Validate a selected cover image.
///
/// "A real regular file" is all the dialog layer claims, for the same reason as
/// the source picker: whether the bytes are a usable PNG is the settings
/// service's judgement, and it reports one stable code when they are not.
pub(crate) fn validate_selected_image(
    selected: PathBuf,
) -> Result<FilePickerOutcome, FilePickerError> {
    let canonical = canonicalize_selection(&selected)?;
    if !canonical.is_file() {
        return Err(FilePickerError::SelectionInvalid);
    }
    Ok(FilePickerOutcome::Selected(canonical))
}

fn canonicalize_selection(selected: &std::path::Path) -> Result<PathBuf, FilePickerError> {
    if !selected.is_absolute() {
        return Err(FilePickerError::SelectionInvalid);
    }
    let metadata = fs::metadata(selected).map_err(|_| FilePickerError::SelectionInvalid)?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(FilePickerError::SelectionInvalid);
    }
    let canonical = selected
        .canonicalize()
        .map_err(|_| FilePickerError::SelectionInvalid)?;
    if !canonical.is_absolute() {
        return Err(FilePickerError::SelectionInvalid);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn a_selected_folder_is_revalidated_and_canonicalized() {
        let root = tempdir().expect("selected directory");
        validate_selected_folder(root.path().to_owned()).expect("valid selected directory");
        // Only a directory is a model source. A regular file is not a folder a
        // user exported, so it is rejected here instead of being passed on to the
        // store, which would only have to report a layout it cannot use.
        let file = root.path().join("我的猫 · 标准模式");
        fs::write(&file, b"payload").expect("selected file");
        assert_eq!(
            validate_selected_folder(file).expect_err("a regular file is not a folder"),
            FilePickerError::SelectionInvalid
        );
    }

    #[test]
    fn selected_cover_image_is_revalidated_and_canonicalized() {
        let root = tempdir().expect("selected cover root");
        let cover = root.path().join("cover.png");
        fs::write(&cover, b"\x89PNG\r\n\x1a\n").expect("selected cover");
        assert_eq!(
            validate_selected_image(cover.clone()).expect("valid selected cover"),
            FilePickerOutcome::Selected(cover.canonicalize().expect("canonical cover"))
        );
        // Whether the bytes are a usable PNG is the settings service's judgement
        // rather than the dialog layer's, so a file that is not a PNG is still a
        // valid selection.
        let plain = root.path().join("not-an-image.bin");
        fs::write(&plain, b"payload").expect("selected plain file");
        assert!(validate_selected_image(plain).is_ok());
        // A directory is not a regular file, so it never passes this check.
        assert_eq!(
            validate_selected_image(root.path().to_owned()).expect_err("directory as cover"),
            FilePickerError::SelectionInvalid
        );
    }

    #[test]
    fn missing_paths_and_relative_paths_are_rejected_without_path_details() {
        let root = tempdir().expect("picker root");
        for selected in [root.path().join("missing"), PathBuf::from("relative")] {
            let error = validate_selected_folder(selected).expect_err("invalid selection");
            assert_eq!(error, FilePickerError::SelectionInvalid);
            assert_eq!(error.to_string(), "model_source_picker_selection_invalid");
        }
    }

    #[test]
    fn stable_error_codes_cover_every_picker_failure() {
        let mut codes = FilePickerError::ALL
            .iter()
            .map(|error| error.as_str())
            .collect::<Vec<_>>();
        assert!(
            codes
                .iter()
                .all(|code| code.starts_with("model_source_picker_"))
        );
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), FilePickerError::ALL.len());
        for (error, expected) in [
            (
                FilePickerError::WrongThread,
                "model_source_picker_wrong_thread",
            ),
            (
                FilePickerError::BackendUnavailable,
                "model_source_picker_backend_unavailable",
            ),
            (
                FilePickerError::SelectionUnavailable,
                "model_source_picker_selection_unavailable",
            ),
            (
                FilePickerError::SelectionInvalid,
                "model_source_picker_selection_invalid",
            ),
        ] {
            assert_eq!(error.as_str(), expected);
            assert_eq!(error.to_string(), expected);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pickers_reject_background_threads_before_touching_appkit() {
        let source = std::thread::spawn(|| pick_model_folder(|_| {}))
            .join()
            .expect("picker test thread");
        let cover = std::thread::spawn(|| pick_model_cover(|_| {}))
            .join()
            .expect("picker test thread");
        assert_eq!(
            source.expect_err("background picker"),
            FilePickerError::WrongThread
        );
        assert_eq!(
            cover.expect_err("background picker"),
            FilePickerError::WrongThread
        );
    }
}
