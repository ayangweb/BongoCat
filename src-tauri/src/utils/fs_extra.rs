use fs_extra::dir::{CopyOptions, copy};
use std::fs::create_dir_all;
use tauri::command;

#[command]
pub async fn copy_dir(from_path: String, to_path: String) -> Result<(), String> {
    let mut options = CopyOptions::new();
    options.content_only = true;

    create_dir_all(&to_path).map_err(|err| err.to_string())?;

    copy(from_path, to_path, &options).map_err(|err| err.to_string())?;

    Ok(())
}
