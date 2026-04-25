use tauri::command;

#[command]
pub fn is_elevated() -> Result<bool, String> {
    is_elevated_inner()
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

#[cfg(not(target_os = "windows"))]
fn is_elevated_inner() -> Result<bool, String> {
    Ok(true)
}
