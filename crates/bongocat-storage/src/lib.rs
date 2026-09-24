//! User-private storage primitives shared by every crate that writes product
//! state to disk.
//!
//! Configuration, installed models, logs and diagnostics exports are all
//! user-private data. Each of them used to carry its own copy of the same
//! `chmod` helpers, so a change to the permission model had to be applied in
//! five places or it silently drifted between them. This crate is the single
//! definition of what "written privately" means.
//!
//! Unix modes are applied after every directory creation and every atomic
//! replacement, because a file that replaces another can otherwise inherit the
//! mode of the file it replaced instead of the mode it was created with.
//! Windows has nothing to do here: the profile directory ACL is already the
//! platform's user-private boundary, so every helper is a no-op there.
//!
//! This crate owns the permission and replacement primitives only. Deciding
//! *where* product state lives stays in `bongocat-config`, and log retention
//! stays in `bongocat-log`; neither is duplicated across crates.

#![forbid(unsafe_code)]

use atomic_write_file::AtomicWriteFile;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

/// Restrict a directory to the current user (`0o700` on Unix).
pub fn set_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Restrict an already-open file to the current user (`0o600` on Unix).
pub fn set_private_file(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

/// Restrict a file to the current user by path (`0o600` on Unix).
pub fn set_private_path(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Create `path` and every missing parent, then restrict `path` itself.
///
/// `create_dir_all` applies the process umask to the directories it creates, so
/// pairing it with [`set_private_directory`] is what actually makes the leaf
/// directory private. The two calls were written next to each other in seven
/// places before this helper existed.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    set_private_directory(path)
}

/// Atomically replace `path` with `bytes`, leaving the result owner-only.
///
/// The replacement happens in the same directory, so the rename is atomic on
/// every supported platform and a reader never observes a partial file. The
/// mode is requested when the replacement file is created and applied again
/// after the commit, which covers the case where the commit preserved the mode
/// of the file it replaced.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    let mut options = AtomicWriteFile::options();
    #[cfg(not(unix))]
    let options = AtomicWriteFile::options();
    #[cfg(unix)]
    {
        use atomic_write_file::unix::OpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as _;
        options.preserve_mode(false).mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.commit()?;
    set_private_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    #[test]
    fn write_private_atomic_replaces_the_file_contents() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("payload.json");
        write_private_atomic(&path, b"first").expect("first write");
        write_private_atomic(&path, b"second").expect("second write");
        let mut contents = String::new();
        File::open(&path)
            .expect("open")
            .read_to_string(&mut contents)
            .expect("read");
        assert_eq!(contents, "second");
    }

    #[test]
    #[cfg(unix)]
    fn write_private_atomic_leaves_the_file_owner_only() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("payload.json");
        write_private_atomic(&path, b"private").expect("write");
        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    fn create_private_dir_all_creates_missing_parents() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("nested").join("deeper");
        create_private_dir_all(&path).expect("create");
        assert!(path.is_dir());
    }

    #[test]
    #[cfg(unix)]
    fn create_private_dir_all_ignores_the_process_umask() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("private");
        fs::create_dir_all(&path).expect("create");
        assert_ne!(mode_of(&path), 0o700, "umask already produced 0o700");
        create_private_dir_all(&path).expect("create private");
        assert_eq!(mode_of(&path), 0o700);
    }

    /// The dangerous case is not a fresh write but a replacement: a file that
    /// already exists with a looser mode must not keep it, because an earlier
    /// version or a manual edit could have left the state readable by others.
    #[test]
    #[cfg(unix)]
    fn write_private_atomic_tightens_a_previously_loose_file() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("payload.json");
        fs::write(&path, b"loose").expect("seed");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("loosen");
        write_private_atomic(&path, b"private").expect("write");
        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn set_private_file_restricts_an_open_file() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("log.jsonl");
        let file = File::create(&path).expect("create");
        set_private_file(&file).expect("restrict");
        drop(file);
        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn set_private_path_restricts_a_file_by_path() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("log.jsonl");
        File::create(&path).expect("create");
        set_private_path(&path).expect("restrict");
        assert_eq!(mode_of(&path), 0o600);
    }
}
