use crate::{
    DirectoryPickerError, DirectoryPickerOutcome, directory_picker::validate_selected_directory,
};
use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::NSApplication;
use rfd::AsyncFileDialog;

type PickerResult = Result<DirectoryPickerOutcome, DirectoryPickerError>;

fn asynchronous_sheet_is_available(mtm: MainThreadMarker) -> bool {
    let application = NSApplication::sharedApplication(mtm);
    let has_window = application.mainWindow().is_some() || !application.windows().is_empty();
    application.isRunning() && has_window
}

pub(crate) fn pick_model_directory<F>(on_complete: F) -> Result<(), DirectoryPickerError>
where
    F: FnOnce(PickerResult) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(DirectoryPickerError::WrongThread)?;
    if !asynchronous_sheet_is_available(mtm) {
        // `rfd` falls back to a synchronous `runModal` when no sheet parent exists. That reenters
        // GPUI's event loop and previously caused `RefCell already borrowed`, so reject the call.
        return Err(DirectoryPickerError::BackendUnavailable);
    }

    let task = autoreleasepool(|_| {
        AsyncFileDialog::new()
            .set_can_create_directories(false)
            .pick_folder()
    });
    std::thread::Builder::new()
        .name("bongocat-directory-validation".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // `rfd` maps both cancellation and backend failure to `None`; cancellation is the
                // only outcome the public API can represent without inventing information.
                match async_io::block_on(task) {
                    Some(handle) => validate_selected_directory(handle.path().to_path_buf()),
                    None => Ok(DirectoryPickerOutcome::Cancelled),
                }
            }))
            .unwrap_or(Err(DirectoryPickerError::BackendUnavailable));
            on_complete(result);
        })
        .map(|_| ())
        .map_err(|_| DirectoryPickerError::BackendUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_rejects_background_threads_before_touching_appkit() {
        let error = std::thread::spawn(|| pick_model_directory(|_| {}))
            .join()
            .expect("picker test thread")
            .expect_err("background picker");
        assert_eq!(error, DirectoryPickerError::WrongThread);
    }
}
