use crate::{StartupItemEnvironment, StartupItemError, StartupItemState};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW,
            RegOpenKeyExW, RegSetValueExW,
        },
    },
    core::{PCWSTR, w},
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

pub(super) fn state(
    environment: StartupItemEnvironment,
) -> Result<StartupItemState, StartupItemError> {
    let command = expected_command_for_current_executable()?;
    state_for_command(environment, &command)
}

pub(super) fn set_enabled(
    environment: StartupItemEnvironment,
    enabled: bool,
) -> Result<StartupItemState, StartupItemError> {
    if enabled {
        let command = expected_command_for_current_executable()?;
        write_value(environment, &command)?;
        state_for_command(environment, &command)
    } else {
        delete_value(environment)?;
        Ok(StartupItemState::Disabled)
    }
}

fn expected_command_for_current_executable() -> Result<Vec<u16>, StartupItemError> {
    let executable =
        std::env::current_exe().map_err(|_| StartupItemError::CurrentExecutableUnavailable)?;
    expected_command(&executable)
}

fn expected_command(executable: &Path) -> Result<Vec<u16>, StartupItemError> {
    if !executable.is_absolute() {
        return Err(StartupItemError::InvalidExecutablePath);
    }
    let executable = executable.as_os_str().encode_wide().collect::<Vec<_>>();
    if executable.contains(&u16::from(b'"')) || executable.contains(&0) {
        return Err(StartupItemError::InvalidExecutablePath);
    }
    let mut command = Vec::with_capacity(executable.len() + 18);
    command.push(u16::from(b'"'));
    command.extend(executable);
    command.extend(OsStr::new("\" --run-seconds 0").encode_wide());
    Ok(command)
}

const fn value_name(environment: StartupItemEnvironment) -> &'static str {
    match environment {
        StartupItemEnvironment::Development => "BongoCat Development",
        StartupItemEnvironment::Production => "BongoCat Production",
    }
}

fn state_for_command(
    environment: StartupItemEnvironment,
    expected: &[u16],
) -> Result<StartupItemState, StartupItemError> {
    match read_value(environment)? {
        None => Ok(StartupItemState::Disabled),
        Some(actual) if actual == expected => Ok(StartupItemState::Enabled),
        Some(_) => Ok(StartupItemState::Stale),
    }
}

fn read_value(environment: StartupItemEnvironment) -> Result<Option<Vec<u16>>, StartupItemError> {
    let value_name = wide_null(value_name(environment));
    let mut byte_count = 0_u32;
    // SAFETY: Both strings are NUL-terminated for the duration of the call. The first call only
    // queries the byte count and does not receive a data pointer.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut byte_count),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS && result != ERROR_MORE_DATA {
        return Err(StartupItemError::StateReadFailed);
    }
    if byte_count == 0 {
        return Ok(Some(Vec::new()));
    }

    let mut bytes = vec![0_u8; byte_count as usize];
    // SAFETY: The allocated buffer is byte_count bytes long, pcbdata points to its current length,
    // and all string pointers remain valid and NUL-terminated during the call.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut byte_count),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(StartupItemError::StateReadFailed);
    }
    bytes.truncate(byte_count as usize);
    if !bytes.len().is_multiple_of(2) {
        return Ok(Some(Vec::new()));
    }
    let mut value = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .collect::<Vec<_>>();
    while value.last() == Some(&0) {
        value.pop();
    }
    Ok(Some(value))
}

fn write_value(
    environment: StartupItemEnvironment,
    command: &[u16],
) -> Result<(), StartupItemError> {
    let key = RegistryKey::create()?;
    let value_name = wide_null(value_name(environment));
    let data = command
        .iter()
        .copied()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    // SAFETY: key is an owned open registry handle, value_name is NUL-terminated, and data is a
    // complete NUL-terminated UTF-16LE REG_SZ value valid for the duration of the call.
    let result = unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(value_name.as_ptr()),
            None,
            REG_SZ,
            Some(&data),
        )
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(StartupItemError::EnableFailed)
    }
}

fn delete_value(environment: StartupItemEnvironment) -> Result<(), StartupItemError> {
    let Some(key) = RegistryKey::open()? else {
        return Ok(());
    };
    let value_name = wide_null(value_name(environment));
    // SAFETY: key is an owned open registry handle and value_name remains NUL-terminated and valid
    // for the duration of the call.
    let result = unsafe { RegDeleteValueW(key.0, PCWSTR(value_name.as_ptr())) };
    if result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(StartupItemError::DisableFailed)
    }
}

struct RegistryKey(HKEY);

impl RegistryKey {
    fn create() -> Result<Self, StartupItemError> {
        let mut key = HKEY::default();
        // SAFETY: phkresult points to initialized storage, the subkey is a static NUL-terminated
        // string, and no security descriptor or disposition output is requested.
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        if result == ERROR_SUCCESS {
            Ok(Self(key))
        } else {
            Err(StartupItemError::EnableFailed)
        }
    }

    fn open() -> Result<Option<Self>, StartupItemError> {
        let mut key = HKEY::default();
        // SAFETY: phkresult points to initialized storage and the subkey is a static
        // NUL-terminated string.
        let result = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                None,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut key,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else if result == ERROR_SUCCESS {
            Ok(Some(Self(key)))
        } else {
            Err(StartupItemError::DisableFailed)
        }
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns the open key handle and closes it exactly once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

    struct ValueRestore {
        environment: StartupItemEnvironment,
        original: Option<Vec<u16>>,
        restored: bool,
    }

    impl ValueRestore {
        fn restore(&mut self) -> Result<(), StartupItemError> {
            match &self.original {
                Some(value) => write_value(self.environment, value)?,
                None => delete_value(self.environment)?,
            }
            self.restored = true;
            Ok(())
        }
    }

    impl Drop for ValueRestore {
        fn drop(&mut self) {
            if self.restored {
                return;
            }
            let _ = match &self.original {
                Some(value) => write_value(self.environment, value),
                None => delete_value(self.environment),
            };
        }
    }

    #[test]
    fn environments_use_distinct_value_names() {
        assert_ne!(
            value_name(StartupItemEnvironment::Development),
            value_name(StartupItemEnvironment::Production)
        );
    }

    #[test]
    fn command_quotes_the_absolute_executable_and_has_stable_arguments() {
        assert_eq!(
            expected_command(Path::new(r"C:\Program Files\BongoCat\bongocat-app.exe")),
            Ok(
                OsStr::new(r#""C:\Program Files\BongoCat\bongocat-app.exe" --run-seconds 0"#)
                    .encode_wide()
                    .collect()
            )
        );
    }

    #[test]
    fn command_rejects_relative_and_quoted_paths_but_accepts_native_utf16() {
        assert_eq!(
            expected_command(Path::new("bongocat-app.exe")),
            Err(StartupItemError::InvalidExecutablePath)
        );
        assert_eq!(
            expected_command(Path::new(r#"C:\Bad"Name\bongocat-app.exe"#)),
            Err(StartupItemError::InvalidExecutablePath)
        );

        let invalid = PathBuf::from(OsString::from_wide(&[0xD800]));
        assert!(expected_command(&Path::new(r"C:\").join(invalid)).is_ok());
    }

    #[test]
    #[ignore = "changes and restores the current user's Development startup value"]
    fn windows_startup_item_registry_smoke_restores_original_value() {
        let environment = StartupItemEnvironment::Development;
        let production_before = read_value(StartupItemEnvironment::Production).unwrap();
        let mut restore = ValueRestore {
            environment,
            original: read_value(environment).unwrap(),
            restored: false,
        };

        delete_value(environment).unwrap();
        assert_eq!(state(environment), Ok(StartupItemState::Disabled));

        assert_eq!(
            set_enabled(environment, true),
            Ok(StartupItemState::Enabled)
        );
        write_value(
            environment,
            &OsStr::new(r#""C:\stale\bongocat-app.exe" --run-seconds 0"#)
                .encode_wide()
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(state(environment), Ok(StartupItemState::Stale));
        assert_eq!(
            set_enabled(environment, true),
            Ok(StartupItemState::Enabled)
        );
        assert_eq!(
            set_enabled(environment, false),
            Ok(StartupItemState::Disabled)
        );

        restore.restore().unwrap();
        assert_eq!(read_value(environment).unwrap(), restore.original);
        assert_eq!(
            read_value(StartupItemEnvironment::Production).unwrap(),
            production_before
        );
    }
}
