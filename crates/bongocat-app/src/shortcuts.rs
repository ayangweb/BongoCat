use bongocat_config::{ModelBehaviorAction, ModelSource, ShortcutCommand, ShortcutTarget};
use bongocat_model::ModelOrigin;
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
fn model_origin(source: ModelSource) -> ModelOrigin {
    match source {
        ModelSource::BuiltIn => ModelOrigin::Preset,
        ModelSource::Imported => ModelOrigin::Installed,
    }
}

pub fn application_shortcut_dispatcher(
    runtime: RuntimeClient,
    application_sink: SyncSender<ShortcutCommand>,
) -> ShortcutDispatcher {
    ShortcutDispatcher::new(move |target| match target {
        ShortcutTarget::Application(command) => application_sink
            .try_send(*command)
            .map(|()| ShortcutDispatch::ApplicationQueued)
            .map_err(|_| ShortcutDispatchError::ApplicationQueueFull),
        ShortcutTarget::ModelBehavior { model, action } => {
            let snapshot = runtime.snapshot();
            let Some(active) = snapshot.active_model else {
                return Ok(ShortcutDispatch::IgnoredInactiveModel);
            };
            if active.id.as_str() != model.id
                || snapshot.active_model_origin != Some(model_origin(model.source))
            {
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
