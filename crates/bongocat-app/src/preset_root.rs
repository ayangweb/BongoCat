//! Where the model packages this build ships live.
//!
//! A packaged build carries them inside its own bundle; a Development run finds
//! the repository's own resources. Both are resolved from the executable rather
//! than from the working directory, so the product behaves the same however it
//! was launched.

use super::*;

pub(crate) fn preset_root() -> PathBuf {
    if let Ok(executable) = env::current_exe()
        && let Some(root) = bundled_preset_root(&executable)
        && root.is_dir()
    {
        return root;
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

#[cfg(any(target_os = "windows", test))]
pub(crate) fn executable_relative_preset_root(executable: &Path) -> Option<PathBuf> {
    Some(executable.parent()?.join("resources/models"))
}
