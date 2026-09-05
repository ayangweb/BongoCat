use super::*;
use crate::SettingsInputMonitoringPermission;

pub(super) fn is_activation_key(event: &KeyDownEvent) -> bool {
    !event.keystroke.modifiers.control
        && !event.keystroke.modifiers.platform
        && !event.keystroke.modifiers.alt
        && (matches!(event.keystroke.key.as_str(), "enter" | "space")
            || event.keystroke.key_char.as_deref() == Some(" "))
}
pub(super) fn shortcut_from_key_event(event: &KeyDownEvent) -> Option<String> {
    let key = canonical_capture_key(event.keystroke.key.as_str())?;
    let modifiers = &event.keystroke.modifiers;
    let mut parts: Vec<String> = Vec::with_capacity(5);
    if modifiers.control {
        parts.push("Control".to_owned());
    }
    if modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if modifiers.shift {
        parts.push("Shift".to_owned());
    }
    if modifiers.platform {
        parts.push("Meta".to_owned());
    }
    parts.push(key);
    let candidate = parts.join("+");
    ShortcutChord::parse(&candidate)
        .ok()
        .map(|chord| chord.canonical())
}

pub(super) fn is_capture_cancel(event: &KeyDownEvent) -> bool {
    event.keystroke.key.eq_ignore_ascii_case("escape")
        && !event.keystroke.modifiers.control
        && !event.keystroke.modifiers.platform
        && !event.keystroke.modifiers.alt
        && !event.keystroke.modifiers.shift
}

pub(super) struct ShortcutRow {
    pub(super) target: ShortcutCaptureTarget,
    pub(super) shortcut: Option<String>,
}

pub(super) fn shortcut_rows(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
) -> Vec<ShortcutRow> {
    let mut rows = shortcuts
        .commands
        .iter()
        .map(|binding| ShortcutRow {
            target: ShortcutCaptureTarget::Command(binding.command.clone()),
            shortcut: Some(binding.shortcut.clone()),
        })
        .collect::<Vec<_>>();
    let Some((model, behaviors)) = active_model.and_then(|model| {
        entries
            .iter()
            .find(|entry| entry.id == model.id && entry.origin == model.origin)
            .and_then(|entry| match &entry.availability {
                SettingsModelAvailability::Ready { behaviors, .. } => Some((model, behaviors)),
                SettingsModelAvailability::Invalid { .. } => None,
            })
    }) else {
        return rows;
    };
    rows.extend(behaviors.iter().map(|behavior| {
        let behavior_id = model_behavior_id(behavior);
        ShortcutRow {
            target: ShortcutCaptureTarget::ModelBehavior {
                model_id: model.id.clone(),
                behavior_id: behavior_id.clone(),
            },
            shortcut: shortcuts
                .model_behaviors
                .iter()
                .find(|binding| binding.model_id == model.id && binding.behavior_id == behavior_id)
                .map(|binding| binding.shortcut.clone()),
        }
    }));
    rows
}

pub(super) fn shortcut_behavior_rows(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
) -> Vec<ShortcutRow> {
    shortcut_rows(shortcuts, active_model, entries)
        .into_iter()
        .filter(|row| matches!(row.target, ShortcutCaptureTarget::ModelBehavior { .. }))
        .collect()
}

pub(super) fn shortcut_targets(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
) -> Vec<ShortcutCaptureTarget> {
    shortcut_rows(shortcuts, active_model, entries)
        .into_iter()
        .map(|row| row.target)
        .collect()
}

fn model_behavior_id(behavior: &SettingsModelBehavior) -> String {
    match behavior {
        SettingsModelBehavior::Motion { group, index } => format!("motion:{group}:{index}"),
        SettingsModelBehavior::Expression { name } => format!("expression:{name}"),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn shortcut_accessibility_rows(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
    language: SettingsLanguage,
) -> Vec<(ShortcutCaptureTarget, String, String)> {
    shortcut_rows(shortcuts, active_model, entries)
        .into_iter()
        .map(|row| {
            let label = shortcut_accessibility_label(language, &row.target);
            let value = row
                .shortcut
                .unwrap_or_else(|| ui_text(language, UiText::NotSet).to_owned());
            (row.target, label, value)
        })
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn shortcut_accessibility_node_id(index: usize) -> AccessibilityNodeId {
    AccessibilityNodeId::new(
        ACCESSIBILITY_SHORTCUT_CAPTURE_BASE
            .saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn shortcut_clear_accessibility_rows(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
    language: SettingsLanguage,
) -> Vec<(ShortcutCaptureTarget, String)> {
    shortcut_rows(shortcuts, active_model, entries)
        .into_iter()
        .filter_map(|row| {
            row.shortcut.map(|_| {
                let label = format!(
                    "{}: {}",
                    ui_text(language, UiText::Clear),
                    shortcut_accessibility_label(language, &row.target)
                );
                (row.target, label)
            })
        })
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn shortcut_clear_accessibility_node_id(index: usize) -> AccessibilityNodeId {
    AccessibilityNodeId::new(
        ACCESSIBILITY_SHORTCUT_CLEAR_BASE.saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn shortcut_target_for_accessibility_node(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
    node_id: AccessibilityNodeId,
) -> Option<ShortcutCaptureTarget> {
    let index = node_id
        .get()
        .checked_sub(ACCESSIBILITY_SHORTCUT_CAPTURE_BASE)
        .and_then(|index| usize::try_from(index).ok())?;
    shortcut_targets(shortcuts, active_model, entries)
        .get(index)
        .cloned()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn shortcut_clear_target_for_accessibility_node(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
    node_id: AccessibilityNodeId,
) -> Option<ShortcutCaptureTarget> {
    let index = node_id
        .get()
        .checked_sub(ACCESSIBILITY_SHORTCUT_CLEAR_BASE)
        .and_then(|index| usize::try_from(index).ok())?;
    shortcut_clear_accessibility_rows(
        shortcuts,
        active_model,
        entries,
        SettingsLanguage::EnglishUnitedStates,
    )
    .get(index)
    .map(|(target, _)| target.clone())
}

pub(super) fn shortcut_capture_tab_index(index: usize) -> isize {
    100_isize.saturating_add(
        isize::try_from(index)
            .unwrap_or(isize::MAX / 2)
            .saturating_mul(2),
    )
}

pub(super) fn shortcut_clear_tab_index(index: usize) -> isize {
    shortcut_capture_tab_index(index).saturating_add(1)
}

pub(super) fn replace_shortcut(
    shortcuts: &mut SettingsShortcuts,
    target: &ShortcutCaptureTarget,
    shortcut: String,
) -> bool {
    match target {
        ShortcutCaptureTarget::Command(command) => {
            let Some(binding) = shortcuts
                .commands
                .iter_mut()
                .find(|binding| binding.command == *command)
            else {
                return false;
            };
            binding.shortcut = shortcut;
        }
        ShortcutCaptureTarget::ModelBehavior {
            model_id,
            behavior_id,
        } => {
            if let Some(binding) = shortcuts.model_behaviors.iter_mut().find(|binding| {
                binding.model_id == *model_id && binding.behavior_id == *behavior_id
            }) {
                binding.shortcut = shortcut;
            } else {
                shortcuts
                    .model_behaviors
                    .push(SettingsModelBehaviorBinding {
                        model_id: model_id.clone(),
                        behavior_id: behavior_id.clone(),
                        shortcut,
                    });
            }
        }
    }
    true
}

pub(super) fn clear_shortcut(
    shortcuts: &mut SettingsShortcuts,
    target: &ShortcutCaptureTarget,
) -> bool {
    match target {
        ShortcutCaptureTarget::Command(command) => {
            let original_len = shortcuts.commands.len();
            shortcuts
                .commands
                .retain(|binding| binding.command != *command);
            shortcuts.commands.len() != original_len
        }
        ShortcutCaptureTarget::ModelBehavior {
            model_id,
            behavior_id,
        } => {
            let original_len = shortcuts.model_behaviors.len();
            shortcuts.model_behaviors.retain(|binding| {
                binding.model_id != *model_id || binding.behavior_id != *behavior_id
            });
            shortcuts.model_behaviors.len() != original_len
        }
    }
}

pub(super) fn canonical_capture_key(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    if lower.len() == 1 && lower.as_bytes()[0].is_ascii_alphanumeric() {
        return Some(lower.to_ascii_uppercase());
    }
    Some(
        match lower.as_str() {
            "minus" => "-",
            "equal" => "=",
            "space" => "Space",
            "enter" | "return" => "Enter",
            "escape" | "esc" => "Escape",
            "backspace" => "Backspace",
            "tab" => "Tab",
            "delete" | "forwarddelete" => "Delete",
            "insert" => "Insert",
            "home" => "Home",
            "end" => "End",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "left" | "arrowleft" => "ArrowLeft",
            "right" | "arrowright" => "ArrowRight",
            "up" | "arrowup" => "ArrowUp",
            "down" | "arrowdown" => "ArrowDown",
            "capslock" => "CapsLock",
            "printscreen" => "PrintScreen",
            "scrolllock" => "ScrollLock",
            "pause" => "Pause",
            "f1" => "F1",
            "f2" => "F2",
            "f3" => "F3",
            "f4" => "F4",
            "f5" => "F5",
            "f6" => "F6",
            "f7" => "F7",
            "f8" => "F8",
            "f9" => "F9",
            "f10" => "F10",
            "f11" => "F11",
            "f12" => "F12",
            _ => return None,
        }
        .to_owned(),
    )
}

pub(super) fn shortcut_conflicts(shortcuts: &SettingsShortcuts) -> bool {
    let mut seen = BTreeSet::new();
    shortcuts
        .commands
        .iter()
        .map(|binding| binding.shortcut.as_str())
        .chain(
            shortcuts
                .model_behaviors
                .iter()
                .map(|binding| binding.shortcut.as_str()),
        )
        .filter_map(|value| ShortcutChord::parse(value).ok())
        .any(|chord| !seen.insert(chord.canonical()))
}

pub(super) struct InputServicePresentation {
    pub(super) title: &'static str,
    pub(super) detail: String,
    pub(super) running: bool,
    pub(super) attention: bool,
}

pub(super) fn input_service_presentation(
    diagnostics: SettingsInputDiagnostics,
    language: SettingsLanguage,
) -> InputServicePresentation {
    let (key, running, attention) = match diagnostics.service_status {
        SettingsInputServiceStatus::NotStarted => (UiText::NotStarted, false, false),
        SettingsInputServiceStatus::Running => (UiText::Running, true, false),
        SettingsInputServiceStatus::PermissionDenied => (UiText::PermissionRequired, false, true),
        SettingsInputServiceStatus::BackendUnavailable => (UiText::BackendUnavailable, false, true),
        SettingsInputServiceStatus::Failed => (UiText::StartupFailed, false, true),
        SettingsInputServiceStatus::Stopped => (UiText::Stopped, false, false),
    };
    let permission = match diagnostics.input_monitoring_permission {
        SettingsInputMonitoringPermission::Unsupported => UiText::Unsupported,
        SettingsInputMonitoringPermission::Denied => UiText::PermissionRequired,
        SettingsInputMonitoringPermission::Granted => UiText::Granted,
    };
    let separator = match language {
        SettingsLanguage::ChineseSimplified => "：",
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => ": ",
    };
    InputServicePresentation {
        title: ui_text(language, key),
        detail: format!(
            "{}{}{}\n{}",
            ui_text(language, UiText::InputMonitoring),
            separator,
            ui_text(language, permission),
            input_service_attempts(language, diagnostics.service_start_attempts),
        ),
        running,
        attention,
    }
}

pub(super) struct RuntimeDiagnosticsPresentation {
    pub(super) title: String,
    pub(super) detail: String,
    pub(super) attention: bool,
}

fn runtime_error_title(
    language: SettingsLanguage,
    error: SettingsRuntimeErrorCode,
) -> &'static str {
    let key = match error {
        SettingsRuntimeErrorCode::GpuPreparationFailed => UiText::GpuPreparationFailed,
        SettingsRuntimeErrorCode::ModelLoadFailed => UiText::ModelLoadFailed,
        SettingsRuntimeErrorCode::ModelEvaluationFailed => UiText::ModelEvaluationFailed,
        SettingsRuntimeErrorCode::MotionLoadFailed => UiText::MotionLoadFailed,
        SettingsRuntimeErrorCode::ExpressionLoadFailed => UiText::ExpressionLoadFailed,
        SettingsRuntimeErrorCode::PlatformUnsupported => UiText::PlatformUnsupported,
        SettingsRuntimeErrorCode::TransportClosed => UiText::RuntimeTransportClosed,
        SettingsRuntimeErrorCode::OverlaySettingsInvalid => UiText::OverlaySettingsInvalid,
        SettingsRuntimeErrorCode::MaximumFpsInvalid => UiText::MaximumFpsInvalid,
        SettingsRuntimeErrorCode::ReleaseFallbackTimeoutInvalid => {
            UiText::ReleaseFallbackTimeoutInvalid
        }
    };
    ui_text(language, key)
}

pub(super) fn runtime_diagnostics_presentation(
    diagnostics: SettingsRuntimeDiagnostics,
    language: SettingsLanguage,
) -> RuntimeDiagnosticsPresentation {
    let (title, attention) = match diagnostics.render_error {
        Some(error) => (runtime_error_title(language, error), true),
        None => (ui_text(language, UiText::NoRendererError), false),
    };
    let detail = match diagnostics.last_command_failure {
        Some(failure) => runtime_command_failure(
            language,
            runtime_error_title(language, failure.code),
            failure.sequence,
        ),
        None => ui_text(language, UiText::NoCommandFailures).to_owned(),
    };
    RuntimeDiagnosticsPresentation {
        title: title.to_owned(),
        detail,
        attention,
    }
}

pub(super) struct ConfigRecoveryPresentation {
    pub(super) title: &'static str,
    pub(super) detail: String,
    pub(super) recovered: bool,
    pub(super) attention: bool,
    pub(super) can_restore: bool,
}

pub(super) fn config_recovery_presentation(
    status: SettingsConfigurationStatus,
    recovery: Option<SettingsConfigRecovery>,
    language: SettingsLanguage,
) -> ConfigRecoveryPresentation {
    match status {
        SettingsConfigurationStatus::RecoveryRequired { checked_backups } => {
            ConfigRecoveryPresentation {
                title: ui_text(language, UiText::ConfigurationUnavailable),
                detail: backup_candidates_checked(language, checked_backups),
                recovered: false,
                attention: true,
                can_restore: true,
            }
        }
        SettingsConfigurationStatus::DefaultsRestoredRestartRequired => {
            ConfigRecoveryPresentation {
                title: ui_text(language, UiText::DefaultsRestored),
                detail: ui_text(language, UiText::RestartToContinue).to_owned(),
                recovered: true,
                attention: false,
                can_restore: false,
            }
        }
        SettingsConfigurationStatus::Ready if recovery.is_some() => {
            let recovery = recovery.expect("ready recovered configuration is present");
            ConfigRecoveryPresentation {
                title: ui_text(language, UiText::RecoveredFromBackup),
                detail: recovered_backup_detail(
                    language,
                    recovery.source_schema_version,
                    recovery.skipped_newer_backups,
                ),
                recovered: true,
                attention: false,
                can_restore: false,
            }
        }
        SettingsConfigurationStatus::Ready => ConfigRecoveryPresentation {
            title: ui_text(language, UiText::LoadedNormally),
            detail: ui_text(language, UiText::NoRecovery).to_owned(),
            recovered: false,
            attention: false,
            can_restore: false,
        },
    }
}
