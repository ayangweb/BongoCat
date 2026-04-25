use tauri::command;

#[command]
#[cfg(target_os = "windows")]
pub fn is_running_as_administrator() -> Result<bool, String> {
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

#[command]
#[cfg(not(target_os = "windows"))]
pub fn is_running_as_administrator() -> Result<bool, String> {
    Ok(true)
}
