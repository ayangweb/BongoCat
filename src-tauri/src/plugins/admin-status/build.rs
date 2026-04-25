const COMMANDS: &[&str] = &["is_elevated"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
