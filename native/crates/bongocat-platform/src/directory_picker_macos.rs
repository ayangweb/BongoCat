use crate::{
    DirectoryPickerError, DirectoryPickerOutcome, directory_picker::validate_selected_directory,
};
use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSOpenPanel};
use std::path::PathBuf;

pub(crate) fn pick_model_directory() -> Result<DirectoryPickerOutcome, DirectoryPickerError> {
    let mtm = MainThreadMarker::new().ok_or(DirectoryPickerError::WrongThread)?;
    autoreleasepool(|_| {
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setCanCreateDirectories(false);
        panel.setResolvesAliases(true);
        panel.setShowsHiddenFiles(false);

        let response = panel.runModal();
        if response == NSModalResponseCancel {
            return Ok(DirectoryPickerOutcome::Cancelled);
        }
        if response != NSModalResponseOK {
            return Err(DirectoryPickerError::BackendUnavailable);
        }
        let urls = panel.URLs();
        if urls.count() != 1 {
            return Err(DirectoryPickerError::SelectionUnavailable);
        }
        let url = urls
            .firstObject()
            .ok_or(DirectoryPickerError::SelectionUnavailable)?;
        let path = url
            .path()
            .ok_or(DirectoryPickerError::SelectionUnavailable)?;
        validate_selected_directory(PathBuf::from(path.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_rejects_background_threads_before_touching_appkit() {
        let error = std::thread::spawn(pick_model_directory)
            .join()
            .expect("picker test thread")
            .expect_err("background picker");
        assert_eq!(error, DirectoryPickerError::WrongThread);
    }
}
