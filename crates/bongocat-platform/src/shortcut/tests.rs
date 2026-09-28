//! The dispatcher's tests: what the application is handed, and what it is told
//! when the application cannot take it.
//!
//! The dispatcher is the boundary between the operating system and the
//! application, so these are about that boundary rather than about any shortcut.

use super::*;

use bongocat_config::{ShortcutCommand, ShortcutTarget};

#[test]
fn forwards_targets_to_the_application_handler() {
    let dispatcher = ShortcutDispatcher::new(|target| {
        assert_eq!(
            target,
            &ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)
        );
        Ok(ShortcutDispatch::ApplicationQueued)
    });
    assert_eq!(
        dispatcher.execute(&ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)),
        Ok(ShortcutDispatch::ApplicationQueued)
    );
}

#[test]
fn preserves_the_handler_error_for_platform_diagnostics() {
    let dispatcher = ShortcutDispatcher::new(|_| Err(ShortcutDispatchError::RuntimeQueueFull));
    assert_eq!(
        dispatcher.execute(&ShortcutTarget::Application(ShortcutCommand::ToggleOverlay)),
        Err(ShortcutDispatchError::RuntimeQueueFull)
    );
}
