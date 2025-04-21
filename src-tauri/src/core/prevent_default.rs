pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    #[cfg(debug_assertions)]
    {
        use tauri_plugin_prevent_default::Flags;

        tauri_plugin_prevent_default::Builder::new()
            .with_flags(Flags::all().difference(Flags::CONTEXT_MENU))
            .build()
    }

    #[cfg(not(debug_assertions))]
    tauri_plugin_prevent_default::init()
}
