const COMMANDS: &[&str] = &["is_running_as_administrator"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
