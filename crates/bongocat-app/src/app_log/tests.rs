//! The application log's tests, split by what they cover.
//!
//! The `panic!` in the privacy test is the point of that test: it proves a value
//! that looks like a path never reaches the log, so the macro has to be in scope
//! for the child module that asserts it.

use super::*;

use tempfile::tempdir;

fn only_log(directory: &Path) -> PathBuf {
    let mut paths = fs::read_dir(directory)
        .expect("read logs")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("application-") && name.ends_with(".log"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.pop().expect("application log")
}

mod code;
mod context;
mod panic_hook;
mod run_marker;
mod write;
