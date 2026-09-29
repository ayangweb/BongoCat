//! Downloading one model package and unpacking it for the model store.
//!
//! Everything here is bounded twice over: the download refuses bodies past the
//! package size limit while streaming, and the unpack refuses archives whose
//! claimed or actual shapes exceed the same limits a local import enforces. The
//! model store re-validates the unpacked tree on import; these bounds only stop
//! a hostile archive from consuming the machine before the store ever sees it.

use std::{
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use bongocat_model::ModelPackageLimits;

/// Why one download session did not end in a package on disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DownloadFailure {
    /// Every URL the entry advertised was unreachable or refused the request.
    DownloadFailed,
    /// The download or the unpack exceeded the package size limit.
    DownloadTooLarge,
    /// The archive is not a package the unpack step accepts.
    InvalidPackage,
    /// The application is shutting down and the work was abandoned.
    Stopped,
}

impl DownloadFailure {
    /// The stable, anonymous reason the application log records. These are log
    /// vocabulary, not user-facing copy: the card maps the failure to its own
    /// localized message without going through this string.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::DownloadFailed => "download_failed",
            Self::DownloadTooLarge => "download_too_large",
            Self::InvalidPackage => "invalid_package",
            Self::Stopped => "stopped",
        }
    }
}

/// Streams one package to `destination`, trying each advertised URL in order.
///
/// Progress reports every chunk with the byte count so far and the advertised
/// total when the server sent one; publishing is the caller's decision. A stop
/// check runs between chunks, so shutdown waits at most one chunk.
pub(super) fn download_package(
    agent: &ureq::Agent,
    urls: &[String],
    destination: &Path,
    maximum_bytes: u64,
    mut progress: impl FnMut(u64, Option<u64>),
    is_stopped: impl Fn() -> bool,
) -> Result<(), DownloadFailure> {
    for url in urls {
        match try_download(
            agent,
            url,
            destination,
            maximum_bytes,
            &mut progress,
            &is_stopped,
        ) {
            Err(DownloadFailure::DownloadFailed) => continue,
            other => return other,
        }
    }
    Err(DownloadFailure::DownloadFailed)
}

fn try_download(
    agent: &ureq::Agent,
    url: &str,
    destination: &Path,
    maximum_bytes: u64,
    progress: &mut impl FnMut(u64, Option<u64>),
    is_stopped: &impl Fn() -> bool,
) -> Result<(), DownloadFailure> {
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(_) => return Err(DownloadFailure::DownloadFailed),
    };
    let total_bytes: Option<u64> = response
        .header("content-length")
        .and_then(|value| value.parse().ok());
    if total_bytes.is_some_and(|total| total > maximum_bytes) {
        return Err(DownloadFailure::DownloadTooLarge);
    }
    if let Some(parent) = destination.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut file = match fs::File::create(destination) {
        Ok(file) => file,
        Err(_) => return Err(DownloadFailure::DownloadFailed),
    };
    let mut reader = response.into_reader();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        if is_stopped() {
            return Err(DownloadFailure::Stopped);
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(_) => return Err(DownloadFailure::DownloadFailed),
        };
        downloaded = downloaded.saturating_add(read as u64);
        if downloaded > maximum_bytes {
            return Err(DownloadFailure::DownloadTooLarge);
        }
        if file.write_all(&buffer[..read]).is_err() {
            return Err(DownloadFailure::DownloadFailed);
        }
        progress(downloaded, total_bytes);
    }
}

/// Unpacks one downloaded archive into `destination`, refusing anything the
/// local import path would refuse on shape: too many entries, too much bytes,
/// paths that leave the destination, or nesting that is too deep.
pub(super) fn extract_package(
    archive_path: &Path,
    destination: &Path,
    limits: &ModelPackageLimits,
) -> Result<(), DownloadFailure> {
    let _ = fs::create_dir_all(destination);
    let file = fs::File::open(archive_path).map_err(|_| DownloadFailure::InvalidPackage)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| DownloadFailure::InvalidPackage)?;
    if archive.len() > limits.maximum_file_count {
        return Err(DownloadFailure::DownloadTooLarge);
    }
    let mut unpacked_bytes: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| DownloadFailure::InvalidPackage)?;
        // `enclosed_name` is the whole path story: it is `None` for absolute
        // paths, `..` components and Windows drive prefixes alike.
        let Some(relative) = entry.enclosed_name() else {
            return Err(DownloadFailure::InvalidPackage);
        };
        if relative.components().count() > limits.maximum_directory_depth {
            return Err(DownloadFailure::InvalidPackage);
        }
        if entry.is_dir() {
            fs::create_dir_all(destination.join(relative))
                .map_err(|_| DownloadFailure::InvalidPackage)?;
            continue;
        }
        let out_path = destination.join(relative);
        let parent = out_path.parent().ok_or(DownloadFailure::InvalidPackage)?;
        fs::create_dir_all(parent).map_err(|_| DownloadFailure::InvalidPackage)?;
        let mut out = fs::File::create(&out_path).map_err(|_| DownloadFailure::InvalidPackage)?;
        unpacked_bytes = unpacked_bytes.saturating_add(copy_bounded(&mut entry, &mut out, limits)?);
        if unpacked_bytes > limits.maximum_package_bytes {
            return Err(DownloadFailure::DownloadTooLarge);
        }
    }
    Ok(())
}

/// Copies one entry, refusing to write more than the per-file limit no matter
/// what the archive's directory claims its size is.
fn copy_bounded(
    entry: &mut impl Read,
    out: &mut impl Write,
    limits: &ModelPackageLimits,
) -> Result<u64, DownloadFailure> {
    let budget = limits.maximum_file_bytes;
    let mut limited = entry.take(budget.saturating_add(1));
    let copied = io::copy(&mut limited, out).map_err(|_| DownloadFailure::InvalidPackage)?;
    if copied > budget {
        return Err(DownloadFailure::DownloadTooLarge);
    }
    Ok(copied)
}

/// Locates the package root inside an unpacked archive.
///
/// Model archives usually ship inside one wrapper directory named after the
/// model, so the unpacked tree is `<root>/<model>/cat.model3.json` and the
/// import must address `<model>`, not the unpack root. The rule is the shape
/// itself: a root that carries no files and exactly one directory hands that
/// directory over, any other shape is already a package root and the import
/// reports its own, precise diagnostic about it.
pub(super) fn resolve_package_root(extracted: &Path) -> PathBuf {
    let Ok(entries) = fs::read_dir(extracted) else {
        return extracted.to_owned();
    };
    let mut directories: Vec<PathBuf> = Vec::new();
    let mut files = 0_usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            directories.push(path);
        } else {
            files += 1;
        }
        if files > 0 || directories.len() > 1 {
            return extracted.to_owned();
        }
    }
    directories
        .into_iter()
        .next()
        .unwrap_or_else(|| extracted.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("archive file");
        let mut writer = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("entry header");
            writer.write_all(bytes).expect("entry bytes");
        }
        writer.finish().expect("archive finish");
    }

    fn tight_limits() -> ModelPackageLimits {
        ModelPackageLimits {
            maximum_file_bytes: 16,
            maximum_package_bytes: 32,
            maximum_file_count: 4,
            maximum_directory_depth: 4,
            ..ModelPackageLimits::default()
        }
    }

    #[test]
    fn unpacks_an_ordinary_package() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(
            &archive,
            &[
                ("cat.model3.json", b"{}".as_slice()),
                ("textures/face.png", &[0_u8; 10]),
            ],
        );
        extract_package(&archive, &unpacked, &tight_limits()).expect("unpack");
        assert!(unpacked.join("cat.model3.json").is_file());
        assert!(unpacked.join("textures/face.png").is_file());
    }

    #[test]
    fn refuses_an_entry_that_leaves_the_destination() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(&archive, &[("../escape.txt", b"x".as_slice())]);
        assert!(extract_package(&archive, &unpacked, &tight_limits()).is_err());
    }

    #[test]
    fn refuses_an_entry_past_the_per_file_limit() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(&archive, &[("big.png", &[0_u8; 17])]);
        assert!(extract_package(&archive, &unpacked, &tight_limits()).is_err());
    }

    #[test]
    fn refuses_a_package_past_the_total_limit() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(
            &archive,
            &[
                ("one.png", &[0_u8; 16]),
                ("two.png", &[0_u8; 16]),
                ("three.png", &[0_u8; 16]),
            ],
        );
        assert!(extract_package(&archive, &unpacked, &tight_limits()).is_err());
    }

    #[test]
    fn refuses_more_entries_than_the_limit() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(
            &archive,
            &[
                ("a.png", &[0_u8]),
                ("b.png", &[0_u8]),
                ("c.png", &[0_u8]),
                ("d.png", &[0_u8]),
                ("e.png", &[0_u8]),
            ],
        );
        assert!(extract_package(&archive, &unpacked, &tight_limits()).is_err());
    }

    /// An archive whose entries live under one wrapper directory, the shape
    /// every catalog zip observed so far ships in.
    fn write_wrapped_zip(path: &Path, wrapper: &str) {
        let file = fs::File::create(path).expect("archive file");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .add_directory(wrapper, zip::write::SimpleFileOptions::default())
            .expect("directory entry");
        writer
            .start_file(
                format!("{wrapper}/cat.model3.json"),
                zip::write::SimpleFileOptions::default(),
            )
            .expect("entry header");
        writer.write_all(b"{}").expect("entry bytes");
        writer.finish().expect("archive finish");
    }

    #[test]
    fn resolves_a_single_wrapper_directory_to_the_package_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_wrapped_zip(&archive, "经典小键盘 · 标准模式");
        extract_package(&archive, &unpacked, &tight_limits()).expect("unpack");
        let resolved = resolve_package_root(&unpacked);
        assert!(resolved.join("cat.model3.json").is_file());
        assert_ne!(resolved, unpacked);
    }

    #[test]
    fn keeps_a_flat_package_at_the_unpack_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(&archive, &[("cat.model3.json", b"{}".as_slice())]);
        extract_package(&archive, &unpacked, &tight_limits()).expect("unpack");
        assert_eq!(resolve_package_root(&unpacked), unpacked);
    }

    #[test]
    fn keeps_several_siblings_at_the_unpack_root() {
        let root = tempfile::tempdir().expect("tempdir");
        let archive = root.path().join("package.zip");
        let unpacked = root.path().join("package");
        write_zip(
            &archive,
            &[
                ("one/cat.model3.json", b"{}".as_slice()),
                ("two/cat.model3.json", b"{}".as_slice()),
            ],
        );
        extract_package(&archive, &unpacked, &tight_limits()).expect("unpack");
        assert_eq!(resolve_package_root(&unpacked), unpacked);
    }

    /// One-shot end-to-end check against a real catalog zip. Run with
    /// `cargo test -p bongocat-app -- --ignored real_catalog_zip`; the archive
    /// path is machine-local, so this never runs in CI.
    #[test]
    #[ignore]
    fn real_catalog_zip_resolves_to_a_valid_package_root() {
        let archive = std::path::PathBuf::from(
            std::env::var("REAL_MODEL_ZIP")
                .unwrap_or_else(|_| r"C:\Users\hi\AppData\Local\Temp\model-test.zip".to_owned()),
        );
        let root = tempfile::tempdir().expect("tempdir");
        let unpacked = root.path().join("package");
        extract_package(&archive, &unpacked, &ModelPackageLimits::default())
            .expect("real zip unpacks");
        let resolved = resolve_package_root(&unpacked);
        assert!(resolved.join("cat.model3.json").is_file());
        let store = bongocat_model_store::ModelStore::new(
            root.path().join("models"),
            root.path().join("models.writer.lock"),
            ModelPackageLimits::default(),
        )
        .expect("store constructs");
        let content = store
            .inspect_source(&resolved)
            .expect("the resolved package root must be a valid source");
        assert!(matches!(
            content,
            bongocat_model_store::ModelSourceContent::Package
        ));
    }
}
