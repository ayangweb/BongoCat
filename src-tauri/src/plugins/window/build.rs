const COMMANDS: &[&str] = &["show_window", "hide_window", "set_always_on_top"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
