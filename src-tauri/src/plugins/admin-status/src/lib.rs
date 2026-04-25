use tauri::{
    Runtime, generate_handler,
    plugin::{Builder, TauriPlugin},
};

mod commands;

pub use commands::*;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("admin-status")
        .invoke_handler(generate_handler![commands::is_running_as_administrator])
        .build()
}
