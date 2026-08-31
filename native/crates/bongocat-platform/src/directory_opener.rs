use std::{fmt, fs, path::Path};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{
    process::{Command, Stdio},
    thread,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryOpenError {
    UnsupportedPlatform,
    InvalidPath,
    DirectoryUnavailable,
    LaunchFailed,
}

impl fmt::Display for DirectoryOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedPlatform => "directory opening is unsupported on this platform",
            Self::InvalidPath => "directory path is invalid",
            Self::DirectoryUnavailable => "directory is unavailable",
            Self::LaunchFailed => "directory opener could not be launched",
        })
    }
}

impl std::error::Error for DirectoryOpenError {}

pub fn open_directory(path: &Path) -> Result<(), DirectoryOpenError> {
    if !path.is_absolute() {
        return Err(DirectoryOpenError::InvalidPath);
    }
    let canonical = fs::canonicalize(path).map_err(|_| DirectoryOpenError::DirectoryUnavailable)?;
    if !canonical.is_dir() {
        return Err(DirectoryOpenError::DirectoryUnavailable);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return Err(DirectoryOpenError::UnsupportedPlatform);

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut child = directory_open_command(&canonical)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| DirectoryOpenError::LaunchFailed)?;
        let reaper = thread::Builder::new()
            .name("bongocat-directory-opener-reaper".to_owned())
            .spawn(move || {
                let _ = child.wait();
            });
        if reaper.is_err() {
            return Err(DirectoryOpenError::LaunchFailed);
        }
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn directory_open_command(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    command.arg(path);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn invalid_and_unavailable_paths_have_stable_anonymous_errors() {
        assert_eq!(
            open_directory(Path::new("relative")),
            Err(DirectoryOpenError::InvalidPath)
        );

        let base = tempdir().expect("temporary directory");
        let missing = base.path().join("missing");
        assert_eq!(
            open_directory(&missing),
            Err(DirectoryOpenError::DirectoryUnavailable)
        );
        let file = base.path().join("file.txt");
        fs::write(&file, b"file").expect("test file");
        assert_eq!(
            open_directory(&file),
            Err(DirectoryOpenError::DirectoryUnavailable)
        );

        for error in [
            DirectoryOpenError::UnsupportedPlatform,
            DirectoryOpenError::InvalidPath,
            DirectoryOpenError::DirectoryUnavailable,
            DirectoryOpenError::LaunchFailed,
        ] {
            let message = error.to_string();
            assert!(!message.contains('/') && !message.contains('\\'));
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn platform_opener_receives_the_directory_as_one_argument_without_a_shell() {
        #[cfg(target_os = "macos")]
        let expected_program = std::ffi::OsStr::new("/usr/bin/open");
        #[cfg(target_os = "windows")]
        let expected_program = std::ffi::OsStr::new("explorer.exe");
        let path = Path::new("directory with spaces and & metacharacter");
        let command = directory_open_command(path);

        assert_eq!(command.get_program(), expected_program);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![path.as_os_str()]
        );
    }
}
