use std::{fmt, path::PathBuf};

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use std::fs;

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
}
