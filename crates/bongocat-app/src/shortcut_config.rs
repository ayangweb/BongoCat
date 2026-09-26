//! Building the shortcut bindings the platform should have registered, and the
//! legacy default chords a newly activated model receives.
//!
//! Model behaviors are scoped to exactly one model on purpose. The configuration
//! keeps a binding for every model the user has activated, and each model counts
//! its defaults from the primary modifier, so two models can legitimately hold
//! the same chord. Only the live model's half may reach the platform: registering
//! every model's half would occupy chords that can never fire and would leave the
//! platform's per-chord target map pointing at whichever model registered first.

use crate::model_identity::{config_source_from_model, config_source_from_settings};
use bongocat_config::{
    CompiledShortcuts, ConfigError, GamepadAutoSwitchConfig, ModelBehaviorAction,
    ModelBehaviorBinding, ModelIdentity, ModelSource, NativeConfig, ShortcutBinding,
    ShortcutConfig, ShortcutModifiers,
};
use bongocat_model::{CommittedModel, ModelBehaviorSnapshot, ModelId};

/// The shortcut bindings the platform should have registered right now: every
/// application command, plus the behaviors of the live model when the model
/// behavior switch is on.
///
/// Model behaviors are scoped to exactly one model on purpose. The
/// configuration keeps a binding for every model the user has activated, and
/// each model counts its defaults from the first digit of the primary modifier
/// — so two models can legitimately hold the same chord. Only the live model's
/// half may reach the platform: registering every model's half would occupy
/// chords that can never fire (the dispatcher drops a target whose model is not
/// active) and would leave the platform's per-chord target map pointing at
/// whichever model registered first.
pub(crate) fn active_shortcuts(
    config: &NativeConfig,
    active_model: Option<&ModelIdentity>,
) -> Result<CompiledShortcuts, ConfigError> {
    config.shortcuts.active_bindings(active_model).compile()
}

/// Drop every gamepad auto switch target that names a model which no longer
/// exists, and report whether anything changed.
///
/// Only an imported model can be removed, so a build-shipped target that happens
/// to share the id names a different model and is kept. The gate and the
/// surviving target are left exactly as they were: removing a model must not
/// switch the feature off.
pub(crate) fn without_removed_model_targets(
    switch: &GamepadAutoSwitchConfig,
    removed: &ModelId,
) -> (GamepadAutoSwitchConfig, bool) {
    let mut next = switch.clone();
    let mut changed = false;
    for target in [&mut next.connected_model, &mut next.disconnected_model] {
        if target.as_ref().is_some_and(|target| {
            target.source == ModelSource::Imported && target.id == removed.as_str()
        }) {
            *target = None;
            changed = true;
        }
    }
    (next, changed)
}

/// The platform's command modifier, which the legacy auto-assignment used as the
/// base of every model behaviour chord: Command on macOS, Control everywhere
/// else. `bongocat-config` takes it as a parameter so that crate stays
/// platform-free.
pub(crate) const fn behavior_shortcut_primary() -> u8 {
    if cfg!(target_os = "macos") {
        ShortcutModifiers::META
    } else {
        ShortcutModifiers::CONTROL
    }
}

/// The behavior ids of one committed model, in the order the model declares
/// them: every motion group in declaration order, then every expression. The
/// legacy auto-assignment walked them in this order, and so does the Shortcuts
/// page.
pub(crate) fn behavior_ids(model: &CommittedModel) -> Vec<String> {
    model
        .snapshot()
        .behaviors
        .into_iter()
        .map(|behavior| {
            let action = match behavior {
                ModelBehaviorSnapshot::Motion { group, index } => {
                    ModelBehaviorAction::Motion { group, index }
                }
                ModelBehaviorSnapshot::Expression { name } => {
                    ModelBehaviorAction::Expression { name }
                }
            };
            action.behavior_id()
        })
        .collect()
}

/// Fill in the legacy default chords for one model's motions and expressions,
/// leaving every binding the user already has untouched. Returns how many
/// bindings were added.
pub(crate) fn assign_default_behavior_shortcuts(
    config: &mut NativeConfig,
    model: &CommittedModel,
) -> usize {
    let model_identity = ModelIdentity {
        id: model.id().as_str().to_owned(),
        source: config_source_from_model(model.origin()),
    };
    bongocat_config::assign_default_behavior_shortcuts(
        &mut config.shortcuts,
        &model_identity,
        &behavior_ids(model),
        behavior_shortcut_primary(),
    )
}

/// Rebuild the shortcut section from what the settings window sent.
///
/// The payload carries bindings only, so the command gate is passed in
/// separately: recording or clearing a chord must never switch the window
/// shortcuts back on behind the user's back.
pub(crate) fn shortcut_config_from_settings(
    shortcuts: bongocat_ui_protocol::SettingsShortcuts,
    commands_enabled: bool,
    model_behaviors_enabled: bool,
) -> ShortcutConfig {
    ShortcutConfig {
        commands_enabled,
        model_behaviors_enabled,
        command_bindings: shortcuts
            .commands
            .into_iter()
            .map(|binding| ShortcutBinding {
                command: binding.command,
                shortcut: binding.shortcut,
            })
            .collect(),
        model_behavior_bindings: shortcuts
            .model_behaviors
            .into_iter()
            .map(|binding| ModelBehaviorBinding {
                model: ModelIdentity {
                    id: binding.model.id,
                    source: config_source_from_settings(binding.model.origin),
                },
                behavior_id: binding.behavior_id,
                shortcut: binding.shortcut,
            })
            .collect(),
    }
}
