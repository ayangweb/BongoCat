use tauri::command;

#[command]
pub fn is_elevated() -> Result<bool, String> {
    is_elevated_inner()
}

#[command]
pub fn relaunch_as_administrator() -> Result<(), String> {
    relaunch_as_administrator_inner()
}

#[cfg(target_os = "windows")]
fn is_elevated_inner() -> Result<bool, String> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    unsafe {
        let mut token = HANDLE::default();

        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|error| error.to_string())?;

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned_size = 0;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned_size,
        );

        let _ = CloseHandle(token);

        result.map_err(|error| error.to_string())?;

        Ok(elevation.TokenIsElevated != 0)
    }
}

#[cfg(target_os = "windows")]
fn relaunch_as_administrator_inner() -> Result<(), String> {
    use std::{env::current_exe, iter::once, os::windows::ffi::OsStrExt, process::exit};
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };

    let operation = "runas".encode_utf16().chain(once(0)).collect::<Vec<_>>();
    let executable = current_exe()
        .map_err(|error| error.to_string())?
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(executable.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    if result.0 as usize <= 32 {
        return Err(format!(
            "failed to relaunch with elevation: {}",
            result.0 as usize
        ));
    }

    exit(0);
}

#[cfg(not(target_os = "windows"))]
fn is_elevated_inner() -> Result<bool, String> {
    Ok(true)
}

#[cfg(not(target_os = "windows"))]
fn relaunch_as_administrator_inner() -> Result<(), String> {
    Ok(())
}
