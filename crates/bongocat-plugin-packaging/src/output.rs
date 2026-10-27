use crate::{Error, Result};
use std::{fs, io::Write as _, path::Path};

/// Write generated output atomically and allow a repeat build to replace it.
///
/// Development catalogs and signed catalogs are derived outputs, not immutable
/// release history. Identical bytes are left untouched; different bytes are
/// staged beside the destination so Windows does not have to rename over an
/// existing file. The old file is restored if the replacement cannot commit.
pub fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.is_file() && fs::read(path)? == bytes {
        return Ok(());
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;

    if !path.exists() {
        staged.persist(path)?;
        return Ok(());
    }

    let backup = tempfile::Builder::new()
        .prefix(".previous-")
        .tempfile_in(parent)?
        .into_temp_path();
    let backup_path = backup.to_path_buf();
    fs::remove_file(&backup_path)?;
    fs::rename(path, &backup_path)?;
    if let Err(error) = staged.persist(path) {
        let _ = fs::rename(&backup_path, path);
        return Err(error.into());
    }
    Ok(())
}

/// Keep immutable release artifacts from being silently rewritten.
pub fn new_or_same(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        if fs::read(path)? == bytes {
            return Ok(());
        }
        return Err(Error::Invalid(format!(
            "refusing to overwrite different bytes at {}",
            path.display()
        )));
    }
    replace(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_is_idempotent_and_restores_on_demand() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.json");
        replace(&path, b"first").unwrap();
        replace(&path, b"first").unwrap();
        replace(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(new_or_same(&path, b"second").is_ok());
        assert!(new_or_same(&path, b"third").is_err());
    }
}
