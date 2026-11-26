//! Persistent cache for catalog artwork.
//!
//! A release icon is small but still crosses the same GitHub proxy boundary as
//! the plugin package. Keeping the last valid copy on disk means a later launch
//! can paint the card before that network request starts, and a failed proxy
//! cannot turn a previously usable icon into an empty square.
use crate::{catalog, sha256_of};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

const ICON_CACHE_DIRECTORY: &str = "cache/icons";
const MAX_CACHED_ICONS: usize = 64;
const MAX_ICON_EDGE: u32 = 4096;
/// Fetch one remote catalog icon once and return its persistent local copy.
///
/// A valid cache entry is used without touching the network. Otherwise the
/// download follows the catalog's proxy order, is validated before it can
/// replace the cache file, and is committed atomically.
pub(crate) fn cache_remote_icon(
    plugin_dir: &Path,
    plugin_id: &str,
    version: &str,
    url: &str,
    stop: &AtomicBool,
) -> Result<PathBuf, String> {
    let path = cache_path(plugin_dir, plugin_id, version);
    if valid_cached_icon(&path) {
        return Ok(path);
    }
    if stop.load(Ordering::Acquire) {
        return Err("icon cache cancelled".into());
    }
    let bytes = catalog::download_icon_blocking(url, stop).map_err(|error| error.to_string())?;
    write_icon(&path, &bytes)?;
    prune_icon_cache(plugin_dir);
    Ok(path)
}

fn cache_path(plugin_dir: &Path, plugin_id: &str, version: &str) -> PathBuf {
    let key = sha256_of(format!("{plugin_id}\0{version}").as_bytes());
    plugin_dir
        .join(ICON_CACHE_DIRECTORY)
        .join(format!("{}.png", &key[..32]))
}

fn valid_cached_icon(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > catalog::MAX_ICON_BYTES as u64
    {
        return false;
    }
    fs::read(path)
        .ok()
        .is_some_and(|bytes| decodes_as_square_rgba(&bytes))
}

fn write_icon(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if !decodes_as_square_rgba(bytes) {
        return Err("catalog icon is not a square RGBA PNG".into());
    }
    let parent = path.parent().ok_or("icon cache path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = atomic_write_file::AtomicWriteFile::options()
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.commit().map_err(|error| error.to_string())
}

fn decodes_as_square_rgba(bytes: &[u8]) -> bool {
    use image::ImageDecoder as _;

    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    reader.limits(icon_decode_limits());
    let Ok(decoder) = reader.into_decoder() else {
        return false;
    };
    let (width, height) = decoder.dimensions();
    if width != height
        || !(1..=MAX_ICON_EDGE).contains(&width)
        || decoder.original_color_type() != image::ExtendedColorType::Rgba8
    {
        return false;
    }
    let mut pixels = vec![0; decoder.total_bytes() as usize];
    decoder.read_image(&mut pixels).is_ok()
}

fn icon_decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_ICON_EDGE);
    limits.max_image_height = Some(MAX_ICON_EDGE);
    limits.max_alloc = Some(catalog::MAX_ICON_BYTES as u64 * 16);
    limits
}

fn prune_icon_cache(plugin_dir: &Path) {
    let directory = plugin_dir.join(ICON_CACHE_DIRECTORY);
    let Ok(entries) = fs::read_dir(&directory) else {
        return;
    };
    let mut files = entries
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    entry.path(),
                )
            })
        })
        .collect::<Vec<_>>();
    if files.len() <= MAX_CACHED_ICONS {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let remove = files.len() - MAX_CACHED_ICONS;
    for (_, path) in files.into_iter().take(remove) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon_png() -> Vec<u8> {
        include_bytes!("../../../resources/icons/tray-macos.png").to_vec()
    }

    #[test]
    fn a_valid_icon_is_cached_atomically_and_reused() {
        let root = tempfile::tempdir().unwrap();
        let path = cache_path(root.path(), "com.example.plugin", "1.0.0");
        let bytes = icon_png();
        write_icon(&path, &bytes).unwrap();
        assert!(valid_cached_icon(&path));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            cache_remote_icon(
                root.path(),
                "com.example.plugin",
                "1.0.0",
                "https://example.com/plugin.png",
                &AtomicBool::new(false),
            )
            .unwrap(),
            path
        );

        // The same plugin version keeps one key even when a later catalog read
        // arrives through another proxy prefix.
        assert_eq!(cache_path(root.path(), "com.example.plugin", "1.0.0"), path);
        assert_ne!(cache_path(root.path(), "com.example.plugin", "1.0.1"), path);
    }

    #[test]
    fn malformed_or_non_square_icons_never_replace_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let path = cache_path(root.path(), "com.example.plugin", "1.0.0");
        let bytes = icon_png();
        write_icon(&path, &bytes).unwrap();
        let original = fs::read(&path).unwrap();

        let mut not_png = bytes.clone();
        not_png[1] = b'X';
        let mut not_rgba = bytes.clone();
        not_rgba[25] = 2;
        let mut not_square = bytes;
        not_square[16..20].copy_from_slice(&1u32.to_be_bytes());

        for invalid in [not_png, not_rgba, not_square] {
            assert!(write_icon(&path, &invalid).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }
}
