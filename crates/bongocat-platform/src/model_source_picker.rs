//! Native pickers for a model source and for a model's own cover image.
//!
//! A model reaches the product either as the folder a user exported or as the
//! `.zip` archive a model site handed out, so this module owns one picker per
//! source kind and one shared result vocabulary. The store decides what it was
//! actually given; a picker's only job is to return a real path the user chose,
//! while keeping cancellation and backend failure distinguishable from it.
//!
//! Selection is deliberately *not* where a source is judged: the archive picker
//! accepts any regular file rather than filtering on the file name, because the
//! model store recognizes an archive by content and reports one stable
//! diagnostic when the file is not one. Rejecting a renamed archive here would
//! turn a working import into a dialog-level failure. The cover picker keeps the
//! same split: it offers PNG in the dialog as a convenience, and the settings
//! service still validates the bytes before they replace an existing cover.

use std::{fmt, path::PathBuf};

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use std::fs;

#[cfg(target_os = "macos")]
use objc2::{MainThreadMarker, rc::autoreleasepool};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSApplication;

/// The file names the archive dialog offers. The filter is a convenience for
/// finding an export, never a rule the import depends on.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const ARCHIVE_EXTENSIONS: [&str; 1] = ["zip"];

/// The file names the cover dialog offers. Like the archive filter this is a
/// convenience: the settings service validates the selected bytes themselves.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const COVER_EXTENSIONS: [&str; 1] = ["png"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelSourcePickerOutcome {
    Selected(PathBuf),
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelSourcePickerError {
    UnsupportedPlatform,
    WrongThread,
    BackendUnavailable,
    SelectionUnavailable,
    SelectionInvalid,
}

impl ModelSourcePickerError {
    pub const ALL: [Self; 5] = [
        Self::UnsupportedPlatform,
        Self::WrongThread,
        Self::BackendUnavailable,
        Self::SelectionUnavailable,
        Self::SelectionInvalid,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "model_source_picker_unsupported_platform",
            Self::WrongThread => "model_source_picker_wrong_thread",
            Self::BackendUnavailable => "model_source_picker_backend_unavailable",
            Self::SelectionUnavailable => "model_source_picker_selection_unavailable",
            Self::SelectionInvalid => "model_source_picker_selection_invalid",
        }
    }
}

impl fmt::Display for ModelSourcePickerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for ModelSourcePickerError {}

#[cfg(target_os = "macos")]
fn asynchronous_sheet_is_available(mtm: MainThreadMarker) -> bool {
    let application = NSApplication::sharedApplication(mtm);
    let has_window = application.mainWindow().is_some() || !application.windows().is_empty();
    application.isRunning() && has_window
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn spawn_picker_worker<F, P>(on_complete: F, pick: P) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
    P: FnOnce() -> Result<ModelSourcePickerOutcome, ModelSourcePickerError> + Send + 'static,
{
    std::thread::Builder::new()
        .name("bongocat-model-source-picker".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(pick))
                .unwrap_or(Err(ModelSourcePickerError::BackendUnavailable));
            on_complete(result);
        })
        .map(|_| ())
        .map_err(|_| ModelSourcePickerError::BackendUnavailable)
}

#[cfg(target_os = "macos")]
pub(crate) fn pick_model_directory<F>(on_complete: F) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(ModelSourcePickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        // `rfd` falls back to a synchronous `runModal` when no sheet parent exists. That reenters
        // GPUI's event loop and previously caused `RefCell already borrowed`, so reject the call.
        return Err(ModelSourcePickerError::BackendUnavailable);
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
            Some(handle) => validate_selected_directory(handle.path().to_path_buf()),
            None => Ok(ModelSourcePickerOutcome::Cancelled),
        }
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn pick_model_directory<F>(on_complete: F) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
{
    spawn_picker_worker(on_complete, || {
        // The Windows backend runs the common item dialog on this dedicated worker's STA. It sets
        // the folder-only option; Rust validation below remains the authority for the returned path.
        match rfd::FileDialog::new()
            .set_can_create_directories(false)
            .pick_folder()
        {
            Some(path) => validate_selected_directory(path),
            None => Ok(ModelSourcePickerOutcome::Cancelled),
        }
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn pick_model_archive<F>(on_complete: F) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(ModelSourcePickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        return Err(ModelSourcePickerError::BackendUnavailable);
    }

    // Opening the panel is main-thread work exactly like the folder panel, so
    // the archive picker shares the same sheet requirement and worker handoff.
    let task = autoreleasepool(|_| {
        rfd::AsyncFileDialog::new()
            .set_can_create_directories(false)
            .add_filter("Model archive", &ARCHIVE_EXTENSIONS)
            .pick_file()
    });
    spawn_picker_worker(on_complete, move || match async_io::block_on(task) {
        Some(handle) => validate_selected_archive(handle.path().to_path_buf()),
        None => Ok(ModelSourcePickerOutcome::Cancelled),
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn pick_model_archive<F>(on_complete: F) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
{
    spawn_picker_worker(on_complete, || {
        match rfd::FileDialog::new()
            .set_can_create_directories(false)
            .add_filter("Model archive", &ARCHIVE_EXTENSIONS)
            .pick_file()
        {
            Some(path) => validate_selected_archive(path),
            None => Ok(ModelSourcePickerOutcome::Cancelled),
        }
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn pick_model_cover<F>(on_complete: F) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(ModelSourcePickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        return Err(ModelSourcePickerError::BackendUnavailable);
    }

    let task = autoreleasepool(|_| {
        rfd::AsyncFileDialog::new()
            .set_can_create_directories(false)
            .add_filter("Cover image", &COVER_EXTENSIONS)
            .pick_file()
    });
    spawn_picker_worker(on_complete, move || match async_io::block_on(task) {
        Some(handle) => validate_selected_image(handle.path().to_path_buf()),
        None => Ok(ModelSourcePickerOutcome::Cancelled),
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn pick_model_cover<F>(on_complete: F) -> Result<(), ModelSourcePickerError>
where
    F: FnOnce(Result<ModelSourcePickerOutcome, ModelSourcePickerError>) + Send + 'static,
{
    spawn_picker_worker(on_complete, || {
        match rfd::FileDialog::new()
            .set_can_create_directories(false)
            .add_filter("Cover image", &COVER_EXTENSIONS)
            .pick_file()
        {
            Some(path) => validate_selected_image(path),
            None => Ok(ModelSourcePickerOutcome::Cancelled),
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) fn validate_selected_directory(
    selected: PathBuf,
) -> Result<ModelSourcePickerOutcome, ModelSourcePickerError> {
    let canonical = canonicalize_selection(&selected)?;
    if !canonical.is_dir() {
        return Err(ModelSourcePickerError::SelectionInvalid);
    }
    Ok(ModelSourcePickerOutcome::Selected(canonical))
}

/// Validate a selected cover image.
///
/// "A real regular file" is all the dialog layer claims, for the same reason as
/// the archive picker: whether the bytes are a usable PNG is the settings
/// service's judgement, and it reports one stable code when they are not.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) fn validate_selected_image(
    selected: PathBuf,
) -> Result<ModelSourcePickerOutcome, ModelSourcePickerError> {
    let canonical = canonicalize_selection(&selected)?;
    if !canonical.is_file() {
        return Err(ModelSourcePickerError::SelectionInvalid);
    }
    Ok(ModelSourcePickerOutcome::Selected(canonical))
}

/// Validate a selected archive.
///
/// Only "a real regular file" is required here. Whether the file is a usable
/// model archive is the model store's decision, so an archive that was renamed —
/// or one that turns out not to be a zip at all — still reaches the product's
/// own stable diagnostic instead of being rejected by the dialog layer.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) fn validate_selected_archive(
    selected: PathBuf,
) -> Result<ModelSourcePickerOutcome, ModelSourcePickerError> {
    let canonical = canonicalize_selection(&selected)?;
    if !canonical.is_file() {
        return Err(ModelSourcePickerError::SelectionInvalid);
    }
    Ok(ModelSourcePickerOutcome::Selected(canonical))
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn canonicalize_selection(selected: &std::path::Path) -> Result<PathBuf, ModelSourcePickerError> {
    if !selected.is_absolute() {
        return Err(ModelSourcePickerError::SelectionInvalid);
    }
    let metadata = fs::metadata(selected).map_err(|_| ModelSourcePickerError::SelectionInvalid)?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(ModelSourcePickerError::SelectionInvalid);
    }
    let canonical = selected
        .canonicalize()
        .map_err(|_| ModelSourcePickerError::SelectionInvalid)?;
    if !canonical.is_absolute() {
        return Err(ModelSourcePickerError::SelectionInvalid);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn selected_directory_is_revalidated_and_canonicalized() {
        let root = tempdir().expect("selected directory");
        let selected =
            validate_selected_directory(root.path().to_owned()).expect("valid selected directory");
        assert_eq!(
            selected,
            ModelSourcePickerOutcome::Selected(root.path().canonicalize().expect("canonical root"))
        );
    }

    #[test]
    fn selected_archive_is_revalidated_and_canonicalized() {
        let root = tempdir().expect("selected archive root");
        let archive = root.path().join("模型.zip");
        fs::write(&archive, b"PK\x03\x04").expect("selected archive");
        assert_eq!(
            validate_selected_archive(archive.clone()).expect("valid selected archive"),
            ModelSourcePickerOutcome::Selected(archive.canonicalize().expect("canonical archive"))
        );
        // The picker never decides whether the file is a usable archive: a file
        // that is not a zip at all is still a valid selection, because the model
        // store owns that judgement and its stable diagnostic.
        let plain = root.path().join("not-an-archive.bin");
        fs::write(&plain, b"payload").expect("selected plain file");
        assert!(validate_selected_archive(plain).is_ok());
    }

    #[test]
    fn a_directory_is_not_an_archive_and_an_archive_is_not_a_directory() {
        let root = tempdir().expect("picker root");
        let archive = root.path().join("模型.zip");
        fs::write(&archive, b"PK\x03\x04").expect("archive");

        assert_eq!(
            validate_selected_directory(archive).expect_err("archive as directory"),
            ModelSourcePickerError::SelectionInvalid
        );
        assert_eq!(
            validate_selected_archive(root.path().to_owned())
                .expect_err("directory as archive")
                .to_string(),
            "model_source_picker_selection_invalid"
        );
    }

    #[test]
    fn files_missing_paths_and_relative_paths_are_rejected_without_path_details() {
        let root = tempdir().expect("picker root");
        let file = root.path().join("model.txt");
        fs::write(&file, b"not a directory").expect("picker file");
        for selected in [file, root.path().join("missing"), PathBuf::from("relative")] {
            let error = validate_selected_directory(selected).expect_err("invalid selection");
            assert_eq!(error, ModelSourcePickerError::SelectionInvalid);
            assert_eq!(error.to_string(), "model_source_picker_selection_invalid");
        }
        for selected in [
            root.path().join("missing.zip"),
            PathBuf::from("relative.zip"),
            PathBuf::from(""),
        ] {
            assert_eq!(
                validate_selected_archive(selected).expect_err("invalid archive selection"),
                ModelSourcePickerError::SelectionInvalid
            );
        }
    }

    #[test]
    fn stable_error_codes_cover_every_picker_failure() {
        let mut codes = ModelSourcePickerError::ALL
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
        assert_eq!(codes.len(), ModelSourcePickerError::ALL.len());
        for (error, expected) in [
            (
                ModelSourcePickerError::UnsupportedPlatform,
                "model_source_picker_unsupported_platform",
            ),
            (
                ModelSourcePickerError::WrongThread,
                "model_source_picker_wrong_thread",
            ),
            (
                ModelSourcePickerError::BackendUnavailable,
                "model_source_picker_backend_unavailable",
            ),
            (
                ModelSourcePickerError::SelectionUnavailable,
                "model_source_picker_selection_unavailable",
            ),
            (
                ModelSourcePickerError::SelectionInvalid,
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
        let directory = std::thread::spawn(|| pick_model_directory(|_| {}))
            .join()
            .expect("picker test thread");
        let archive = std::thread::spawn(|| pick_model_archive(|_| {}))
            .join()
            .expect("picker test thread");
        let cover = std::thread::spawn(|| pick_model_cover(|_| {}))
            .join()
            .expect("picker test thread");
        assert_eq!(
            directory.expect_err("background picker"),
            ModelSourcePickerError::WrongThread
        );
        assert_eq!(
            archive.expect_err("background picker"),
            ModelSourcePickerError::WrongThread
        );
        assert_eq!(
            cover.expect_err("background picker"),
            ModelSourcePickerError::WrongThread
        );
    }
}
