//! The owner thread registers for real, and stopping it unregisters.

use super::*;

/// End-to-end proof that the owner thread really registers with the
/// OS: a rare, harmless chord (Ctrl+Alt+0) is registered for the
/// duration of the test and unregistered afterwards. Ignored by
/// default because it touches process-global OS registration state;
/// run it explicitly with `cargo test -p bongocat-platform --
/// owner_thread_registers -- --ignored`.
#[test]
#[ignore = "registers a real OS-wide hotkey for the test duration"]
fn owner_thread_registers_and_unregisters_real_hotkeys() {
    let compiled = ShortcutConfig {
        commands_enabled: true,
        command_bindings: vec![ShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "Control+Alt+0".to_owned(),
        }],
        ..ShortcutConfig::default()
    }
    .compile()
    .expect("compiled shortcuts");
    let service = GlobalShortcutService::start(
        ShortcutTable::new(compiled),
        ShortcutDispatcher::new(|_| Ok(ShortcutDispatch::IgnoredApplicationCommand)),
    )
    .expect("global shortcut service starts");
    // The owner thread must have registered the binding with the OS
    // (no startup error, no registration failure).
    assert!(service.registration_failures().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(200));
    service.stop().expect("service stops and unregisters");
}
