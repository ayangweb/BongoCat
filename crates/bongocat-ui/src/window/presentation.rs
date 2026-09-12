use super::*;
use crate::SettingsInputMonitoringPermission;

pub(super) fn is_activation_key(event: &KeyDownEvent) -> bool {
    !event.keystroke.modifiers.control
        && !event.keystroke.modifiers.platform
        && !event.keystroke.modifiers.alt
        && (matches!(event.keystroke.key.as_str(), "enter" | "space")
            || event.keystroke.key_char.as_deref() == Some(" "))
}
pub(super) fn capture_key(key: &str) -> Option<String> {
    if is_capture_modifier(key) {
        return None;
    }
    canonical_capture_key(key).or_else(|| {
        let key = key.trim();
        (!key.is_empty()).then(|| key.to_owned())
    })
}

pub(super) fn shortcut_capture_preview(
    modifiers: &Modifiers,
    keys: &BTreeSet<String>,
) -> Option<String> {
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
    parts.extend(keys.iter().cloned());
    (!parts.is_empty()).then(|| parts.join("+"))
}

/// Format a canonical shortcut for the platform's familiar keyboard labels.
/// The persisted/configuration form remains unchanged so platform adapters can
/// continue to match the typed modifier and HID key identities.
pub(super) fn shortcut_display(shortcut: &str) -> String {
    format_shortcut_display(shortcut, cfg!(target_os = "macos"))
}

pub(super) fn format_shortcut_display(shortcut: &str, macos: bool) -> String {
    if !macos {
        return shortcut.to_owned();
    }

    shortcut
        .split('+')
        .map(macos_shortcut_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn macos_shortcut_token(token: &str) -> &str {
    match token.trim().to_ascii_lowercase().as_str() {
        "control" | "ctrl" => "⌃",
        "alt" | "option" => "⌥",
        "shift" => "⇧",
        "meta" | "command" | "cmd" | "win" | "windows" => "⌘",
        "escape" | "esc" => "⎋",
        "backspace" => "⌫",
        "tab" => "⇥",
        "enter" | "return" => "↩︎",
        "space" => "␣",
        "arrowup" => "↑",
        "arrowdown" => "↓",
        "arrowleft" => "←",
        "arrowright" => "→",
        "backquote" => "`",
        "minus" => "-",
        "equal" => "=",
        "bracketleft" => "[",
        "bracketright" => "]",
        "backslash" => "\\",
        "semicolon" => ";",
        "quote" => "'",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        _ => token.trim(),
    }
}

pub(super) fn shortcut_from_capture(
    modifiers: &Modifiers,
    keys: &BTreeSet<String>,
) -> Option<String> {
    let key = keys.first()?;
    if keys.len() != 1 {
        return None;
    }
    let has_modifier = modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform;
    if !has_modifier && !is_function_key(key) {
        return None;
    }
    let candidate = shortcut_capture_preview(modifiers, keys)?;
    ShortcutChord::parse(&candidate)
        .ok()
        .map(|chord| chord.canonical())
}

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=12).contains(&number))
}

fn is_capture_modifier(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "control"
            | "ctrl"
            | "alt"
            | "option"
            | "shift"
            | "meta"
            | "command"
            | "cmd"
            | "win"
            | "windows"
            | "platform"
    )
}

pub(super) struct ShortcutRow {
    pub(super) target: ShortcutCaptureTarget,
    pub(super) shortcut: Option<String>,
}

const WINDOW_SHORTCUT_COMMANDS: [&str; 5] = [
    "toggle_overlay",
    "open_settings",
    "toggle_mirror",
    "toggle_click_through",
    "toggle_always_on_top",
];

pub(super) fn window_shortcut_rows(shortcuts: &SettingsShortcuts) -> Vec<ShortcutRow> {
    WINDOW_SHORTCUT_COMMANDS
        .iter()
        .map(|command| ShortcutRow {
            target: ShortcutCaptureTarget::Command((*command).to_owned()),
            shortcut: shortcuts
                .commands
                .iter()
                .find(|binding| binding.command == *command)
                .map(|binding| binding.shortcut.clone()),
        })
        .collect()
}

pub(super) fn shortcut_rows(
    shortcuts: &SettingsShortcuts,
    active_model: Option<&SettingsModelKey>,
    entries: &[SettingsModelEntry],
) -> Vec<ShortcutRow> {
    let mut rows = window_shortcut_rows(shortcuts);
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
                .map(|shortcut| shortcut_display(&shortcut))
                .unwrap_or_else(|| {
                    bongocat_i18n::text(language.catalog_locale(), "shortcuts.state.not_set")
                        .to_owned()
                });
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
                    bongocat_i18n::text(language.catalog_locale(), "shortcuts.actions.clear"),
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
            if !WINDOW_SHORTCUT_COMMANDS.contains(&command.as_str()) {
                return false;
            }
            if let Some(binding) = shortcuts
                .commands
                .iter_mut()
                .find(|binding| binding.command == *command)
            {
                binding.shortcut = shortcut;
            } else {
                shortcuts.commands.push(SettingsShortcutBinding {
                    command: command.clone(),
                    shortcut,
                });
            }
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

pub(super) fn conflicting_shortcut(shortcuts: &SettingsShortcuts) -> Option<String> {
    let mut seen = BTreeSet::new();
    for chord in shortcuts
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
    {
        let canonical = chord.canonical();
        if !seen.insert(canonical.clone()) {
            return Some(canonical);
        }
    }
    None
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
        SettingsInputServiceStatus::NotStarted => ("status.not_started", false, false),
        SettingsInputServiceStatus::Running => ("status.running", true, false),
        SettingsInputServiceStatus::PermissionDenied => ("status.permission_required", false, true),
        SettingsInputServiceStatus::BackendUnavailable => {
            ("status.backend_unavailable", false, true)
        }
        SettingsInputServiceStatus::Failed => ("errors.runtime.startup_failed", false, true),
        SettingsInputServiceStatus::Stopped => ("status.stopped", false, false),
    };
    let permission = match diagnostics.input_monitoring_permission {
        SettingsInputMonitoringPermission::Unsupported => "status.unsupported",
        SettingsInputMonitoringPermission::Denied => "status.permission_required",
        SettingsInputMonitoringPermission::Granted => "status.granted",
    };
    let separator = match language {
        SettingsLanguage::ChineseSimplified => "：",
        SettingsLanguage::System | SettingsLanguage::EnglishUnitedStates => ": ",
    };
    InputServicePresentation {
        title: bongocat_i18n::text(language.catalog_locale(), key),
        detail: format!(
            "{}{}{}\n{}",
            bongocat_i18n::text(language.catalog_locale(), "diagnostics.input.monitoring"),
            separator,
            bongocat_i18n::text(language.catalog_locale(), permission),
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
        SettingsRuntimeErrorCode::GpuPreparationFailed => "errors.runtime.gpu_preparation_failed",
        SettingsRuntimeErrorCode::ModelLoadFailed => "errors.models.load_failed",
        SettingsRuntimeErrorCode::ModelEvaluationFailed => "errors.models.evaluation_failed",
        SettingsRuntimeErrorCode::MotionLoadFailed => "errors.models.motion_load_failed",
        SettingsRuntimeErrorCode::ExpressionLoadFailed => "errors.models.expression_load_failed",
        SettingsRuntimeErrorCode::PlatformUnsupported => "errors.runtime.platform_unsupported",
        SettingsRuntimeErrorCode::TransportClosed => "errors.runtime.transport_closed",
        SettingsRuntimeErrorCode::OverlaySettingsInvalid => "errors.settings.overlay_invalid",
        SettingsRuntimeErrorCode::MaximumFpsInvalid => "errors.settings.maximum_fps_invalid",
        SettingsRuntimeErrorCode::ReleaseFallbackTimeoutInvalid => {
            "errors.settings.release_fallback_timeout_invalid"
        }
    };
    bongocat_i18n::text(language.catalog_locale(), key)
}

pub(super) fn runtime_diagnostics_presentation(
    diagnostics: SettingsRuntimeDiagnostics,
    language: SettingsLanguage,
) -> RuntimeDiagnosticsPresentation {
    let (title, attention) = match diagnostics.render_error {
        Some(error) => (runtime_error_title(language, error), true),
        None => (
            bongocat_i18n::text(language.catalog_locale(), "errors.runtime.no_renderer"),
            false,
        ),
    };
    let detail = match diagnostics.last_command_failure {
        Some(failure) => runtime_command_failure(
            language,
            runtime_error_title(language, failure.code),
            failure.sequence,
        ),
        None => bongocat_i18n::text(
            language.catalog_locale(),
            "diagnostics.runtime.no_command_failures",
        )
        .to_owned(),
    };
    let shutdown_failures = diagnostics
        .shutdown_timed_out
        .saturating_add(diagnostics.shutdown_worker_panicked);
    let detail = if shutdown_failures > 0 {
        format!(
            "{} · {}",
            detail,
            runtime_shutdown_failures(language, shutdown_failures)
        )
    } else {
        detail
    };
    RuntimeDiagnosticsPresentation {
        title: title.to_owned(),
        detail,
        attention: attention || shutdown_failures > 0,
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
                title: bongocat_i18n::text(
                    language.catalog_locale(),
                    "errors.settings.configuration_unavailable",
                ),
                detail: backup_candidates_checked(language, checked_backups),
                recovered: false,
                attention: true,
                can_restore: true,
            }
        }
        SettingsConfigurationStatus::DefaultsRestoredRestartRequired => {
            ConfigRecoveryPresentation {
                title: bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.configuration.defaults_restored",
                ),
                detail: bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.configuration.restart_to_continue",
                )
                .to_owned(),
                recovered: true,
                attention: false,
                can_restore: false,
            }
        }
        SettingsConfigurationStatus::Ready if recovery.is_some() => {
            let recovery = recovery.expect("ready recovered configuration is present");
            ConfigRecoveryPresentation {
                title: bongocat_i18n::text(
                    language.catalog_locale(),
                    "diagnostics.configuration.recovered_from_backup",
                ),
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
            title: bongocat_i18n::text(
                language.catalog_locale(),
                "diagnostics.configuration.loaded_normally",
            ),
            detail: bongocat_i18n::text(
                language.catalog_locale(),
                "diagnostics.configuration.no_recovery",
            )
            .to_owned(),
            recovered: false,
            attention: false,
            can_restore: false,
        },
    }
}
