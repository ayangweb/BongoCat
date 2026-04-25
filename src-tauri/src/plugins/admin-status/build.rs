const COMMANDS: &[&str] = &["is_elevated", "relaunch_as_administrator"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
