use bongocat_config::{
    CompiledShortcuts, ModelBehaviorAction, ShortcutModifiers, ShortcutTable, ShortcutTarget,
};
use bongocat_runtime::{
    ExpressionId, InputEdge, MotionId, MotionPriority, PhysicalKey, RuntimeClient, SendError,
    ShortcutAction,
};
use std::{collections::BTreeSet, sync::mpsc::SyncSender};

/// Platform-neutral shortcut edge matcher used after native key codes have
/// already been mapped to USB HID usages. It owns only transient pressed state
/// and returns closed targets; registration and command execution stay with
/// their platform/application owners.
#[derive(Clone, Debug)]
pub struct ShortcutMatcher {
    shortcuts: CompiledShortcuts,
    pressed: BTreeSet<PhysicalKey>,
}

/// Dispatches matched model behavior shortcuts without exposing configuration
/// strings or platform key codes to the runtime. Application-level targets are
/// intentionally reported as ignored until the settings service owns their
/// persistence-aware command path.
#[derive(Clone)]
pub struct ShortcutDispatcher {
    table: ShortcutTable,
    matcher: ShortcutMatcher,
    runtime: RuntimeClient,
    application_sink: Option<SyncSender<bongocat_config::ShortcutCommand>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutDispatch {
    Triggered,
    ApplicationQueued,
    IgnoredApplicationCommand,
    IgnoredInactiveModel,
    NoMatch,
}

#[derive(Debug)]
pub enum ShortcutDispatchError {
    Runtime(SendError),
    ApplicationQueueFull,
}

impl PartialEq for ShortcutDispatchError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::ApplicationQueueFull, Self::ApplicationQueueFull)
                | (Self::Runtime(_), Self::Runtime(_))
        )
    }
}

impl Eq for ShortcutDispatchError {}

impl ShortcutDispatcher {
    pub fn new(shortcuts: CompiledShortcuts, runtime: RuntimeClient) -> Self {
        Self::with_table(ShortcutTable::new(shortcuts), runtime)
    }

    pub fn with_table(table: ShortcutTable, runtime: RuntimeClient) -> Self {
        Self {
            matcher: ShortcutMatcher::new(table.load()),
            table,
            runtime,
            application_sink: None,
        }
    }

    pub fn with_application_sink(
        table: ShortcutTable,
        runtime: RuntimeClient,
        application_sink: SyncSender<bongocat_config::ShortcutCommand>,
    ) -> Self {
        let mut dispatcher = Self::with_table(table, runtime);
        dispatcher.application_sink = Some(application_sink);
        dispatcher
    }

    pub fn apply(
        &mut self,
        key: PhysicalKey,
        edge: InputEdge,
    ) -> Result<ShortcutDispatch, ShortcutDispatchError> {
        let latest = self.table.load();
        if latest != self.matcher.shortcuts {
            self.matcher.replace(latest);
        }
        let Some(target) = self.matcher.apply(key, edge) else {
            return Ok(ShortcutDispatch::NoMatch);
        };
        let target = match target {
            ShortcutTarget::Application(command) => {
                return match self.application_sink.as_ref() {
                    Some(sender) => sender
                        .try_send(command)
                        .map(|()| ShortcutDispatch::ApplicationQueued)
                        .map_err(|_| ShortcutDispatchError::ApplicationQueueFull),
                    None => Ok(ShortcutDispatch::IgnoredApplicationCommand),
                };
            }
            ShortcutTarget::ModelBehavior { model_id, action } => (model_id, action),
        };
        let (model_id, action) = target;
        let Some(active) = self.runtime.snapshot().active_model else {
            return Ok(ShortcutDispatch::IgnoredInactiveModel);
        };
        if active.id.as_str() != model_id {
            return Ok(ShortcutDispatch::IgnoredInactiveModel);
        }
        let action = match action {
            ModelBehaviorAction::Motion { group, index } => ShortcutAction::StartMotion {
                motion: MotionId::new(group, index).expect("validated motion group"),
                priority: MotionPriority::Normal,
            },
            ModelBehaviorAction::Expression { name } => ShortcutAction::SetExpression(
                ExpressionId::new(name).expect("validated expression name"),
            ),
        };
        self.runtime
            .trigger_shortcut(action)
            .map(|_| ShortcutDispatch::Triggered)
            .map_err(ShortcutDispatchError::Runtime)
    }

    pub fn reset(&mut self) {
        self.matcher.reset();
    }

    /// Replaces transient key state with the platform's authoritative
    /// reconciliation snapshot. This never dispatches an action.
    pub fn reconcile(&mut self, pressed: impl IntoIterator<Item = PhysicalKey>) {
        let latest = self.table.load();
        if latest != self.matcher.shortcuts {
            self.matcher.replace(latest);
        }
        self.matcher.reconcile(pressed);
    }
}

impl ShortcutMatcher {
    pub fn new(shortcuts: CompiledShortcuts) -> Self {
        Self {
            shortcuts,
            pressed: BTreeSet::new(),
        }
    }

    pub fn replace(&mut self, shortcuts: CompiledShortcuts) {
        self.shortcuts = shortcuts;
    }

    pub fn apply(&mut self, key: PhysicalKey, edge: InputEdge) -> Option<ShortcutTarget> {
        match edge {
            InputEdge::Down => {
                if !self.pressed.insert(key) || modifier_bit(key).is_some() {
                    return None;
                }
                self.shortcuts
                    .resolve_hid_usage(self.modifiers(), key.hid_usage())
                    .map(|binding| binding.target().clone())
            }
            InputEdge::Up => {
                self.pressed.remove(&key);
                None
            }
        }
    }

    pub fn reconcile(&mut self, pressed: impl IntoIterator<Item = PhysicalKey>) {
        self.pressed = pressed.into_iter().collect();
    }

    pub fn reset(&mut self) {
        self.pressed.clear();
    }

    pub fn pressed_count(&self) -> usize {
        self.pressed.len()
    }

    fn modifiers(&self) -> ShortcutModifiers {
        let bits = self
            .pressed
            .iter()
            .filter_map(|key| modifier_bit(*key))
            .fold(0, |bits, bit| bits | bit);
        ShortcutModifiers::from_bits(bits).expect("modifier mapping uses only declared bits")
    }
}

fn modifier_bit(key: PhysicalKey) -> Option<u8> {
    match key.hid_usage() {
        0xe0 | 0xe4 => Some(ShortcutModifiers::CONTROL),
        0xe1 | 0xe5 => Some(ShortcutModifiers::SHIFT),
        0xe2 | 0xe6 => Some(ShortcutModifiers::ALT),
        0xe3 | 0xe7 => Some(ShortcutModifiers::META),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_config::{
        ModelBehaviorAction, ModelBehaviorBinding, ShortcutBinding, ShortcutCommand, ShortcutConfig,
    };

    fn matcher() -> ShortcutMatcher {
        ShortcutMatcher::new(
            ShortcutConfig {
                commands: vec![ShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+Shift+B".to_owned(),
                }],
                model_behaviors: vec![ModelBehaviorBinding {
                    model_id: "standard".to_owned(),
                    behavior_id: "expression:happy".to_owned(),
                    shortcut: "Alt+M".to_owned(),
                }],
            }
            .compile()
            .expect("compiled shortcuts"),
        )
    }

    #[test]
    fn dispatcher_keeps_application_targets_out_of_runtime_and_handles_no_active_model() {
        let runtime = bongocat_runtime::RuntimeOwner::start(true, 16);
        let client = runtime.client();
        client
            .wait_for_revision(1, std::time::Duration::from_secs(1))
            .expect("runtime ready");
        let compiled = ShortcutConfig {
            commands: vec![ShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+B".to_owned(),
            }],
            model_behaviors: vec![ModelBehaviorBinding {
                model_id: "standard".to_owned(),
                behavior_id: "expression:happy".to_owned(),
                shortcut: "Alt+M".to_owned(),
            }],
        }
        .compile()
        .expect("compiled shortcuts");
        let table = ShortcutTable::new(compiled);
        let mut dispatcher = ShortcutDispatcher::with_table(table.clone(), client.clone());
        let control = PhysicalKey::from_hid_usage(0xe0);
        let alt = PhysicalKey::from_hid_usage(0xe2);
        let b = PhysicalKey::from_hid_usage(0x05);
        let m = PhysicalKey::from_hid_usage(0x10);
        assert_eq!(
            dispatcher.apply(control, InputEdge::Down),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(b, InputEdge::Down),
            Ok(ShortcutDispatch::IgnoredApplicationCommand)
        );
        assert_eq!(
            dispatcher.apply(b, InputEdge::Up),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(control, InputEdge::Up),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(alt, InputEdge::Down),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(m, InputEdge::Down),
            Ok(ShortcutDispatch::IgnoredInactiveModel)
        );
        dispatcher.apply(m, InputEdge::Up).expect("release");
        table.replace(
            ShortcutConfig {
                commands: vec![ShortcutBinding {
                    command: "open_settings".to_owned(),
                    shortcut: "Shift+M".to_owned(),
                }],
                model_behaviors: Vec::new(),
            }
            .compile()
            .expect("replacement shortcuts"),
        );
        assert_eq!(
            dispatcher.apply(alt, InputEdge::Up),
            Ok(ShortcutDispatch::NoMatch)
        );
        let shift = PhysicalKey::from_hid_usage(0xe1);
        assert_eq!(
            dispatcher.apply(shift, InputEdge::Down),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(m, InputEdge::Down),
            Ok(ShortcutDispatch::IgnoredApplicationCommand)
        );
        runtime
            .shutdown(std::time::Duration::from_secs(1))
            .expect("runtime stop");
    }

    #[test]
    fn matches_hid_edges_once_and_aggregates_left_and_right_modifiers() {
        let mut matcher = matcher();
        let left_control = PhysicalKey::from_hid_usage(0xe0);
        let right_control = PhysicalKey::from_hid_usage(0xe4);
        let left_shift = PhysicalKey::from_hid_usage(0xe1);
        let b = PhysicalKey::from_hid_usage(0x05);

        assert_eq!(matcher.apply(left_control, InputEdge::Down), None);
        assert_eq!(matcher.apply(right_control, InputEdge::Down), None);
        assert_eq!(matcher.apply(left_control, InputEdge::Up), None);
        assert_eq!(matcher.apply(left_shift, InputEdge::Down), None);
        assert_eq!(
            matcher.apply(b, InputEdge::Down),
            Some(ShortcutTarget::Application(ShortcutCommand::ToggleOverlay))
        );
        assert_eq!(matcher.apply(b, InputEdge::Down), None);
        assert_eq!(matcher.apply(b, InputEdge::Up), None);
        assert_eq!(
            matcher.apply(b, InputEdge::Down),
            Some(ShortcutTarget::Application(ShortcutCommand::ToggleOverlay))
        );
    }

    #[test]
    fn extra_modifiers_do_not_match_and_reset_clears_repeat_state() {
        let mut matcher = matcher();
        let control = PhysicalKey::from_hid_usage(0xe0);
        let shift = PhysicalKey::from_hid_usage(0xe1);
        let alt = PhysicalKey::from_hid_usage(0xe2);
        let b = PhysicalKey::from_hid_usage(0x05);
        matcher.apply(control, InputEdge::Down);
        matcher.apply(shift, InputEdge::Down);
        matcher.apply(alt, InputEdge::Down);
        assert_eq!(matcher.apply(b, InputEdge::Down), None);
        assert_eq!(matcher.pressed_count(), 4);

        matcher.reset();
        matcher.apply(control, InputEdge::Down);
        matcher.apply(shift, InputEdge::Down);
        assert_eq!(
            matcher.apply(b, InputEdge::Down),
            Some(ShortcutTarget::Application(ShortcutCommand::ToggleOverlay))
        );
    }

    #[test]
    fn reconcile_and_replace_preserve_repeat_suppression_until_release() {
        let mut matcher = matcher();
        let alt = PhysicalKey::from_hid_usage(0xe2);
        let m = PhysicalKey::from_hid_usage(0x10);
        matcher.reconcile([alt, m]);
        assert_eq!(matcher.apply(m, InputEdge::Down), None);
        matcher.apply(m, InputEdge::Up);
        assert_eq!(
            matcher.apply(m, InputEdge::Down),
            Some(ShortcutTarget::ModelBehavior {
                model_id: "standard".to_owned(),
                action: ModelBehaviorAction::Expression {
                    name: "happy".to_owned(),
                },
            })
        );

        matcher.replace(CompiledShortcuts::default());
        assert_eq!(matcher.pressed_count(), 2);
        assert_eq!(matcher.apply(m, InputEdge::Down), None);
        matcher.apply(m, InputEdge::Up);
        assert_eq!(matcher.apply(m, InputEdge::Down), None);
    }

    #[test]
    fn dispatcher_reconciliation_releases_a_missing_shortcut_key() {
        let runtime = bongocat_runtime::RuntimeOwner::start(true, 16);
        let client = runtime.client();
        client
            .wait_for_revision(1, std::time::Duration::from_secs(1))
            .expect("runtime ready");
        let mut dispatcher = ShortcutDispatcher::new(
            ShortcutConfig {
                commands: vec![ShortcutBinding {
                    command: "toggle_overlay".to_owned(),
                    shortcut: "Control+B".to_owned(),
                }],
                model_behaviors: Vec::new(),
            }
            .compile()
            .expect("compiled shortcuts"),
            client.clone(),
        );
        let control = PhysicalKey::from_hid_usage(0xe0);
        let b = PhysicalKey::from_hid_usage(0x05);

        assert_eq!(
            dispatcher.apply(control, InputEdge::Down),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(b, InputEdge::Down),
            Ok(ShortcutDispatch::IgnoredApplicationCommand)
        );
        dispatcher.reconcile([]);
        assert_eq!(
            dispatcher.apply(control, InputEdge::Down),
            Ok(ShortcutDispatch::NoMatch)
        );
        assert_eq!(
            dispatcher.apply(b, InputEdge::Down),
            Ok(ShortcutDispatch::IgnoredApplicationCommand)
        );
        runtime
            .shutdown(std::time::Duration::from_secs(1))
            .expect("runtime stop");
    }
}
