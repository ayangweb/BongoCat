use crate::{
    DirectoryPickerError, DirectoryPickerOutcome, directory_picker::validate_selected_directory,
};
use block2::RcBlock;
use objc2::{MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::{NSModalResponse, NSModalResponseCancel, NSModalResponseOK, NSOpenPanel};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

type PickerResult = Result<DirectoryPickerOutcome, DirectoryPickerError>;

enum RawPickerOutcome {
    Selected(PathBuf),
    Cancelled,
}

fn take_completion<F>(completion: &Arc<Mutex<Option<F>>>) -> Option<F> {
    completion
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

fn complete<F>(completion: &Arc<Mutex<Option<F>>>, result: PickerResult)
where
    F: FnOnce(PickerResult),
{
    if let Some(completion) = take_completion(completion) {
        completion(result);
    }
}

fn read_panel_outcome(
    panel: &NSOpenPanel,
    response: NSModalResponse,
) -> Result<RawPickerOutcome, DirectoryPickerError> {
    if response == NSModalResponseCancel {
        return Ok(RawPickerOutcome::Cancelled);
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
    Ok(RawPickerOutcome::Selected(PathBuf::from(path.to_string())))
}

fn finish_panel<F>(
    panel: &NSOpenPanel,
    response: NSModalResponse,
    completion: &Arc<Mutex<Option<F>>>,
) where
    F: FnOnce(PickerResult) + Send + 'static,
{
    match read_panel_outcome(panel, response) {
        Ok(RawPickerOutcome::Cancelled) => {
            complete(completion, Ok(DirectoryPickerOutcome::Cancelled));
        }
        Ok(RawPickerOutcome::Selected(selected)) => {
            let completion_for_thread = Arc::clone(completion);
            let completion_for_failure = Arc::clone(completion);
            if std::thread::Builder::new()
                .name("bongocat-directory-validation".to_owned())
                .spawn(move || {
                    complete(
                        &completion_for_thread,
                        validate_selected_directory(selected),
                    );
                })
                .is_err()
            {
                complete(
                    &completion_for_failure,
                    Err(DirectoryPickerError::BackendUnavailable),
                );
            }
        }
        Err(error) => complete(completion, Err(error)),
    }
}

pub(crate) fn pick_model_directory<F>(on_complete: F) -> Result<(), DirectoryPickerError>
where
    F: FnOnce(PickerResult) + Send + 'static,
{
    let mtm = MainThreadMarker::new().ok_or(DirectoryPickerError::WrongThread)?;
    autoreleasepool(|_| {
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setCanCreateDirectories(false);
        panel.setResolvesAliases(true);
        panel.setShowsHiddenFiles(false);

        let completion = Arc::new(Mutex::new(Some(on_complete)));
        let completion_for_block = Arc::clone(&completion);
        let panel_for_block = panel.clone();
        let handler: RcBlock<dyn Fn(NSModalResponse)> = RcBlock::new(move |response| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                finish_panel(&panel_for_block, response, &completion_for_block);
            }));
        });
        panel.beginWithCompletionHandler(&handler);
        Ok(())
    })
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
