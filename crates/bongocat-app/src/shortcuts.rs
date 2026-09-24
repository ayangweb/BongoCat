use bongocat_config::{ModelBehaviorAction, ShortcutCommand, ShortcutTarget};
use bongocat_platform::{ShortcutDispatch, ShortcutDispatchError, ShortcutDispatcher};
use bongocat_runtime::{
    ExpressionId, MotionId, MotionPriority, RuntimeClient, SendError, ShortcutAction,
};
use std::sync::mpsc::SyncSender;

/// Build the application-owned half of the global shortcut boundary.
///
/// The platform crate only owns OS registration and invokes this typed
/// callback. Runtime action construction and the application command sink stay
/// here so `bongocat-platform` does not depend on the business runtime.
pub fn application_shortcut_dispatcher(
    runtime: RuntimeClient,
    application_sink: SyncSender<ShortcutCommand>,
) -> ShortcutDispatcher {
    ShortcutDispatcher::new(move |target| match target {
        ShortcutTarget::Application(command) => application_sink
            .try_send(*command)
            .map(|()| ShortcutDispatch::ApplicationQueued)
            .map_err(|_| ShortcutDispatchError::ApplicationQueueFull),
        ShortcutTarget::ModelBehavior { model_id, action } => {
            let Some(active) = runtime.snapshot().active_model else {
                return Ok(ShortcutDispatch::IgnoredInactiveModel);
            };
            if active.id.as_str() != model_id {
                return Ok(ShortcutDispatch::IgnoredInactiveModel);
            }
            let action = match action {
                ModelBehaviorAction::Motion { group, index } => ShortcutAction::StartMotion {
                    motion: MotionId::new(group, *index).expect("validated motion group"),
                    priority: MotionPriority::Normal,
                },
                ModelBehaviorAction::Expression { name } => ShortcutAction::SetExpression(
                    ExpressionId::new(name).expect("validated expression name"),
                ),
            };
            runtime
                .trigger_shortcut(action)
                .map(|_| ShortcutDispatch::Triggered)
                .map_err(|error| match error {
                    SendError::QueueFull(_) => ShortcutDispatchError::RuntimeQueueFull,
                    SendError::RuntimeStopped(_) => ShortcutDispatchError::RuntimeStopped,
                })
        }
    })
}
