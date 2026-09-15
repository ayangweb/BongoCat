use std::{fmt, path::PathBuf};

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use std::fs;

#[cfg(target_os = "macos")]
use objc2::{MainThreadMarker, rc::autoreleasepool};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSApplication;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectoryPickerOutcome {
    Selected(PathBuf),
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryPickerError {
    UnsupportedPlatform,
    WrongThread,
    BackendUnavailable,
    SelectionUnavailable,
    SelectionInvalid,
}

impl DirectoryPickerError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "directory_picker_unsupported_platform",
            Self::WrongThread => "directory_picker_wrong_thread",
            Self::BackendUnavailable => "directory_picker_backend_unavailable",
            Self::SelectionUnavailable => "directory_picker_selection_unavailable",
            Self::SelectionInvalid => "directory_picker_selection_invalid",
        }
    }
}

impl fmt::Display for DirectoryPickerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for DirectoryPickerError {}

#[cfg(target_os = "macos")]
fn asynchronous_sheet_is_available(mtm: MainThreadMarker) -> bool {
    let application = NSApplication::sharedApplication(mtm);
    let has_window = application.mainWindow().is_some() || !application.windows().is_empty();
    application.isRunning() && has_window
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn spawn_picker_worker<F, P>(on_complete: F, pick: P) -> Result<(), DirectoryPickerError>
where
    F: FnOnce(Result<DirectoryPickerOutcome, DirectoryPickerError>) + Send + 'static,
    P: FnOnce() -> Result<DirectoryPickerOutcome, DirectoryPickerError> + Send + 'static,
{
    std::thread::Builder::new()
        .name("bongocat-directory-picker".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(pick))
                .unwrap_or(Err(DirectoryPickerError::BackendUnavailable));
            on_complete(result);
        })
        .map(|_| ())
        .map_err(|_| DirectoryPickerError::BackendUnavailable)
}

#[cfg(target_os = "macos")]
pub(crate) fn pick_model_directory<F>(on_complete: F) -> Result<(), DirectoryPickerError>
where
    F: FnOnce(Result<DirectoryPickerOutcome, DirectoryPickerError>) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(DirectoryPickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        // `rfd` falls back to a synchronous `runModal` when no sheet parent exists. That reenters
        // GPUI's event loop and previously caused `RefCell already borrowed`, so reject the call.
        return Err(DirectoryPickerError::BackendUnavailable);
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
            None => Ok(DirectoryPickerOutcome::Cancelled),
        }
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn pick_model_directory<F>(on_complete: F) -> Result<(), DirectoryPickerError>
where
    F: FnOnce(Result<DirectoryPickerOutcome, DirectoryPickerError>) + Send + 'static,
{
    spawn_picker_worker(on_complete, || {
        // The Windows backend runs the common item dialog on this dedicated worker's STA. It sets
        // the folder-only option; Rust validation below remains the authority for the returned path.
        match rfd::FileDialog::new()
            .set_can_create_directories(false)
            .pick_folder()
        {
            Some(path) => validate_selected_directory(path),
            None => Ok(DirectoryPickerOutcome::Cancelled),
        }
    })
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(crate) fn validate_selected_directory(
    selected: PathBuf,
) -> Result<DirectoryPickerOutcome, DirectoryPickerError> {
    if !selected.is_absolute() {
        return Err(DirectoryPickerError::SelectionInvalid);
    }
    let metadata = fs::metadata(&selected).map_err(|_| DirectoryPickerError::SelectionInvalid)?;
    if !metadata.is_dir() {
        return Err(DirectoryPickerError::SelectionInvalid);
    }
    let canonical = selected
        .canonicalize()
        .map_err(|_| DirectoryPickerError::SelectionInvalid)?;
    if !canonical.is_absolute() || !canonical.is_dir() {
        return Err(DirectoryPickerError::SelectionInvalid);
    }
    Ok(DirectoryPickerOutcome::Selected(canonical))
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
            DirectoryPickerOutcome::Selected(root.path().canonicalize().expect("canonical root"))
        );
    }

    #[test]
    fn files_missing_paths_and_relative_paths_are_rejected_without_path_details() {
        let root = tempdir().expect("picker root");
        let file = root.path().join("model.txt");
        fs::write(&file, b"not a directory").expect("picker file");
        for selected in [file, root.path().join("missing"), PathBuf::from("relative")] {
            let error = validate_selected_directory(selected).expect_err("invalid selection");
            assert_eq!(error, DirectoryPickerError::SelectionInvalid);
            assert_eq!(error.to_string(), "directory_picker_selection_invalid");
        }
    }

    #[test]
    fn stable_error_codes_cover_every_picker_failure() {
        for (error, expected) in [
            (
                DirectoryPickerError::UnsupportedPlatform,
                "directory_picker_unsupported_platform",
            ),
            (
                DirectoryPickerError::WrongThread,
                "directory_picker_wrong_thread",
            ),
            (
                DirectoryPickerError::BackendUnavailable,
                "directory_picker_backend_unavailable",
            ),
            (
                DirectoryPickerError::SelectionUnavailable,
                "directory_picker_selection_unavailable",
            ),
            (
                DirectoryPickerError::SelectionInvalid,
                "directory_picker_selection_invalid",
            ),
        ] {
            assert_eq!(error.as_str(), expected);
            assert_eq!(error.to_string(), expected);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn picker_rejects_background_threads_before_touching_appkit() {
        let error = std::thread::spawn(|| pick_model_directory(|_| {}))
            .join()
            .expect("picker test thread")
            .expect_err("background picker");
        assert_eq!(error, DirectoryPickerError::WrongThread);
    }
}
