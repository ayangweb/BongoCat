use crate::{
    DirectoryPickerError, DirectoryPickerOutcome, directory_picker::validate_selected_directory,
};
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, RPC_E_CHANGED_MODE},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOS_DONTADDTORECENT, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST,
            FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::{HRESULT, PWSTR},
};

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, DirectoryPickerError> {
        // SAFETY: COM is initialized for the current thread with the required null reserved pointer.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Err(DirectoryPickerError::WrongThread);
        }
        result
            .ok()
            .map_err(|_| DirectoryPickerError::BackendUnavailable)?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: This guard exists only after a successful CoInitializeEx on the current thread.
        unsafe { CoUninitialize() };
    }
}

struct TaskMemWide(PWSTR);

impl TaskMemWide {
    fn to_path_buf(&self) -> Result<PathBuf, DirectoryPickerError> {
        if self.0.is_null() {
            return Err(DirectoryPickerError::SelectionUnavailable);
        }
        // SAFETY: IShellItem::GetDisplayName returned a valid CoTaskMem-allocated, null-terminated
        // string that remains owned by this guard for the duration of the copy.
        let wide = unsafe { self.0.as_wide() };
        Ok(PathBuf::from(OsString::from_wide(wide)))
    }
}

impl Drop for TaskMemWide {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetDisplayName transfers this allocation to the caller for CoTaskMemFree.
            unsafe { CoTaskMemFree(Some(self.0.as_ptr().cast())) };
        }
    }
}

pub(crate) fn pick_model_directory() -> Result<DirectoryPickerOutcome, DirectoryPickerError> {
    let _apartment = ComApartment::initialize()?;
    // SAFETY: FileOpenDialog is an in-process COM class requested after STA initialization.
    let dialog: IFileOpenDialog = unsafe {
        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| DirectoryPickerError::BackendUnavailable)?
    };
    // SAFETY: The COM interface is valid for this apartment and all flags are documented options.
    let options = unsafe {
        dialog
            .GetOptions()
            .map_err(|_| DirectoryPickerError::BackendUnavailable)?
    };
    // SAFETY: The COM interface is valid and the option bitset preserves its existing flags.
    unsafe {
        dialog
            .SetOptions(
                options
                    | FOS_PICKFOLDERS
                    | FOS_FORCEFILESYSTEM
                    | FOS_PATHMUSTEXIST
                    | FOS_NOCHANGEDIR
                    | FOS_DONTADDTORECENT,
            )
            .map_err(|_| DirectoryPickerError::BackendUnavailable)?;
    }
    // SAFETY: Show runs the modal dialog on this STA; no owner HWND is borrowed or retained.
    if let Err(error) = unsafe { dialog.Show(None) } {
        if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            return Ok(DirectoryPickerOutcome::Cancelled);
        }
        return Err(DirectoryPickerError::BackendUnavailable);
    }
    // SAFETY: A successful Show makes one result available on the same COM interface.
    let item = unsafe {
        dialog
            .GetResult()
            .map_err(|_| DirectoryPickerError::SelectionUnavailable)?
    };
    // SAFETY: SIGDN_FILESYSPATH requests an owned filesystem path from a valid shell item.
    let selected = TaskMemWide(unsafe {
        item.GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|_| DirectoryPickerError::SelectionUnavailable)?
    });
    validate_selected_directory(selected.to_path_buf()?)
}
