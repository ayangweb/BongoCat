//! Where this build's data lives.
//!
//! Development and Production share one shape and never one root: the layout is
//! derived from the bundle id and the environment, so a Development build cannot
//! read, write or lock Production data and a Production build cannot inherit a
//! Development preference.
//!
//! A portable build relocates that root rather than removing it. The
//! environment still names a directory under whatever root was chosen, so a
//! portable copy keeps the same separation an installed copy has.

use super::*;

/// File whose presence next to the executable makes this copy portable.
///
/// A marker file rather than a command-line flag, because a portable copy is
/// run by double-clicking its executable. A flag would make the user create a
/// shortcut with arguments, which is the one thing a copy-on-a-USB-stick
/// workflow cannot ask for. The file's contents are never read: its existence
/// is the whole signal, so a user can create it with any means.
pub const PORTABLE_MARKER_FILE_NAME: &str = "portable.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildEnvironment {
    Development,
    Production,
}

impl BuildEnvironment {
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageLayout {
    pub environment: BuildEnvironment,
    pub root: PathBuf,
    /// Whether the root was chosen because a [`PORTABLE_MARKER_FILE_NAME`] sits
    /// next to the executable.
    ///
    /// Reported rather than inferred from the path, because the two are not the
    /// same question: a diagnostic that shows a root under the executable's own
    /// directory says so, and a root the user can write to says the data can
    /// travel with the program.
    pub portable: bool,
    pub config: PathBuf,
    pub window_state: PathBuf,
    pub models: PathBuf,
    /// The user side of the models the build ships.
    ///
    /// A preset's package lives inside the application bundle — a signed
    /// `.app` on macOS, the installation directory on Windows — which the
    /// product may not write to. The artwork that replaces a preset's cover is
    /// kept here instead, in the same per-model shape a package uses
    /// (`<id>/resources/cover.png`), so the settings page reads a cover the
    /// same way whichever origin it came from.
    pub model_overrides: PathBuf,
    pub backups: PathBuf,
    pub logs: PathBuf,
    pub updates: PathBuf,
    pub locks: PathBuf,
}

impl StorageLayout {
    pub fn under_application_root(
        application_root: impl AsRef<Path>,
        environment: BuildEnvironment,
    ) -> Self {
        Self::build(application_root.as_ref(), environment, false)
    }

    /// The layout for a copy the user carries with them.
    ///
    /// The application root is the executable's own directory, so everything the
    /// product writes travels with the program. The environment still names a
    /// directory underneath, because a portable Production copy and a portable
    /// Development copy sharing one folder is exactly the collision the
    /// environment directory exists to prevent.
    pub fn portable(executable_directory: impl AsRef<Path>, environment: BuildEnvironment) -> Self {
        Self::build(executable_directory.as_ref(), environment, true)
    }

    fn build(application_root: &Path, environment: BuildEnvironment, portable: bool) -> Self {
        let root = application_root.join(environment.directory_name());
        Self {
            environment,
            portable,
            config: root.join("config.json"),
            window_state: root.join(WINDOW_STATE_FILE_NAME),
            models: root.join("models"),
            model_overrides: root.join("model-overrides"),
            backups: root.join("backups"),
            logs: root.join("logs"),
            updates: root.join("updates"),
            locks: root.join("locks"),
            root,
        }
    }

    pub fn under(base: impl AsRef<Path>, environment: BuildEnvironment) -> Self {
        Self::under_application_root(base.as_ref().join(BUNDLE_ID), environment)
    }

    pub(crate) fn create_directories(&self) -> io::Result<()> {
        create_private_dir_all(&self.root)?;
        for directory in [
            &self.models,
            &self.model_overrides,
            &self.backups,
            &self.logs,
            &self.updates,
            &self.locks,
        ] {
            create_private_dir_all(directory)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformStorageError {
    #[error("platform data directory is unavailable")]
    DataDirectoryUnavailable,
}

/// Whether the directory holding the executable asked to be portable.
///
/// Returns the directory to use as the application root, or `None` to use the
/// platform data directory. Portable is opt-in through
/// [`PORTABLE_MARKER_FILE_NAME`], so the default is always "not portable" and
/// an existing installed copy keeps its settings.
///
/// The marker has to be a regular file. A *directory* of that name is not a
/// request: a package that happens to ship an empty `portable.txt/` directory
/// would otherwise silently move every user's data next to their executable.
pub fn portable_application_root(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    directory
        .join(PORTABLE_MARKER_FILE_NAME)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file())
        .then(|| directory.to_path_buf())
}

/// The layout for the copy whose executable is at `executable`.
///
/// Portable when [`portable_application_root`] says so, and the platform data
/// directory otherwise. This is the whole of portable mode, and it takes the
/// executable as an argument rather than reading it itself so that the decision
/// can be tested against a real directory instead of only against whatever
/// binary happens to be running the test.
pub fn layout_for_executable(
    executable: &Path,
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    if let Some(directory) = portable_application_root(executable) {
        return Ok(StorageLayout::portable(directory, environment));
    }
    let root = dirs::data_dir()
        .ok_or(PlatformStorageError::DataDirectoryUnavailable)?
        .join(BUNDLE_ID);
    Ok(StorageLayout::under_application_root(root, environment))
}

#[cfg(target_os = "macos")]
pub fn platform_layout(
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    // No portable mode on macOS. The executable lives inside a signed bundle,
    // and writing `production/` next to it adds a file the signature does not
    // cover, so a portable marker there would produce a bundle that fails its
    // own `codesign --verify` on the next launch.
    let root = dirs::data_dir()
        .ok_or(PlatformStorageError::DataDirectoryUnavailable)?
        .join(BUNDLE_ID);
    Ok(StorageLayout::under_application_root(root, environment))
}

#[cfg(target_os = "windows")]
pub fn platform_layout(
    environment: BuildEnvironment,
) -> Result<StorageLayout, PlatformStorageError> {
    // The per-user installer puts the executable somewhere the user owns, so a
    // portable copy writes into a directory it is allowed to write to. The
    // marker rule is reached through [`layout_for_executable`] rather than in
    //lined, so both the rule and the choice between portable and installed are
    // covered by tests that run on every platform.
    //
    // A copy with no marker resolves exactly as it did before, which is what
    // keeps an existing installation's settings where they are.
    let executable =
        std::env::current_exe().map_err(|_| PlatformStorageError::DataDirectoryUnavailable)?;
    layout_for_executable(&executable, environment)
}
