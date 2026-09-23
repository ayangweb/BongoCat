use std::{fmt, fs, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryOpenError {
    InvalidPath,
    DirectoryUnavailable,
    LaunchFailed,
}

impl DirectoryOpenError {
    pub const ALL: [Self; 3] = [
        Self::InvalidPath,
        Self::DirectoryUnavailable,
        Self::LaunchFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidPath => "directory_open_invalid_path",
            Self::DirectoryUnavailable => "directory_open_unavailable",
            Self::LaunchFailed => "directory_open_launch_failed",
        }
    }
}

impl fmt::Display for DirectoryOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for DirectoryOpenError {}

pub fn open_directory(path: &Path) -> Result<(), DirectoryOpenError> {
    open_directory_with(path, launch_directory)
}

fn open_directory_with(
    path: &Path,
    launch: impl FnOnce(&Path) -> Result<(), DirectoryOpenError>,
) -> Result<(), DirectoryOpenError> {
    if !path.is_absolute() {
        return Err(DirectoryOpenError::InvalidPath);
    }
    let canonical = fs::canonicalize(path).map_err(|_| DirectoryOpenError::DirectoryUnavailable)?;
    if !canonical.is_dir() {
        return Err(DirectoryOpenError::DirectoryUnavailable);
    }

    launch(&canonical)
}

fn launch_directory(path: &Path) -> Result<(), DirectoryOpenError> {
    opener::open(path).map_err(|_| DirectoryOpenError::LaunchFailed)
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

        for (error, expected) in [
            (
                DirectoryOpenError::InvalidPath,
                "directory_open_invalid_path",
            ),
            (
                DirectoryOpenError::DirectoryUnavailable,
                "directory_open_unavailable",
            ),
            (
                DirectoryOpenError::LaunchFailed,
                "directory_open_launch_failed",
            ),
        ] {
            assert_eq!(error.as_str(), expected);
            assert_eq!(error.to_string(), expected);
        }
        assert_eq!(DirectoryOpenError::ALL.len(), 3);
    }

    #[test]
    fn adapter_delegates_the_canonical_directory_to_the_system_opener() {
        let base = tempdir().expect("temporary directory");
        let canonical = base.path().canonicalize().expect("canonical directory");
        let mut launched = None;

        let result = open_directory_with(base.path(), |path| {
            launched = Some(path.to_owned());
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(launched, Some(canonical));
    }
}
