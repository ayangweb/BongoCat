//! When today's log becomes yesterday's.
//!
//! Rotation is numbered rather than overwritten, so the file a support thread is
//! reading is never the file that is being written. A number that is already
//! taken moves to the next one instead of replacing it, which is what happens
//! when the process restarts several times in a day.

use super::*;

pub(crate) fn rotate_path(
    directory: &Path,
    stream: LogStream,
    active: &Path,
) -> io::Result<PathBuf> {
    let file_name = active
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log path has no file name"))?;
    let stem = file_name
        .strip_suffix(".log")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log path is not .log"))?;
    let prefix = format!("{}-", stream.prefix());
    let date_text = stem.strip_prefix(&prefix).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "active log path belongs to another stream",
        )
    })?;
    let date = UtcDate::parse(date_text).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "active log path has no valid date",
        )
    })?;
    let destination = next_rotation_path(directory, stream, date)?;
    fs::rename(active, &destination)?;
    Ok(destination)
}

pub(crate) fn next_rotation_path(
    directory: &Path,
    stream: LogStream,
    date: UtcDate,
) -> io::Result<PathBuf> {
    let highest = collect_log_files(directory, date)
        .into_iter()
        .filter(|file| file.stream == stream && file.date == date)
        .filter_map(|file| file.generation)
        .max()
        .unwrap_or(0);
    let generation = highest
        .checked_add(1)
        .ok_or_else(|| io::Error::other("log rotation generation overflow"))?;
    let path = directory.join(stream.rotated_file_name(date, generation));
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "next log rotation path already exists",
        ));
    }
    Ok(path)
}
