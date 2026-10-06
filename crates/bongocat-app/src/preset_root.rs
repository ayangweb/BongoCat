//! Where the model packages this build ships live.
//!
//! A packaged build carries them inside its own bundle; a Development run finds
//! the repository's own resources. Both are resolved from the executable rather
//! than from the working directory, so the product behaves the same however it
//! was launched.

use super::*;

#[cfg(target_os = "linux")]
pub(crate) const SYSTEM_PRESET_ROOT: &str = "/usr/share/bongocat/models";

pub(crate) fn preset_root() -> PathBuf {
    if let Ok(executable) = env::current_exe()
        && let Some(root) = bundled_preset_root(&executable)
        && root.is_dir()
    {
        return root;
    }
    #[cfg(target_os = "linux")]
    if Path::new(SYSTEM_PRESET_ROOT).is_dir() {
        return PathBuf::from(SYSTEM_PRESET_ROOT);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .join("resources/models")
}

#[cfg(target_os = "macos")]
pub(crate) fn bundled_preset_root(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    Some(contents.join("Resources/models"))
}

#[cfg(target_os = "windows")]
pub(crate) fn bundled_preset_root(executable: &Path) -> Option<PathBuf> {
    executable_relative_preset_root(executable)
}

#[cfg(target_os = "linux")]
pub(crate) fn bundled_preset_root(executable: &Path) -> Option<PathBuf> {
    executable_relative_preset_root(executable)
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub(crate) fn executable_relative_preset_root(executable: &Path) -> Option<PathBuf> {
    Some(executable.parent()?.join("resources/models"))
}
