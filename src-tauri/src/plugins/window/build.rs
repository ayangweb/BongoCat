const COMMANDS: &[&str] = &[
    "show_window",
    "hide_window",
    "set_always_on_top",
    "set_taskbar_visibility",
    "set_multi_screen_follow",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
