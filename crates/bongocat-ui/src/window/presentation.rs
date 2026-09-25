use super::*;
use bongocat_config::ModelBehaviorAction;

pub(super) fn logging_level_display_name(
    level: SettingsLogLevel,
    display_language: SettingsLanguage,
) -> &'static str {
    let key = match level {
        SettingsLogLevel::Error => "settings.app_system.logging.level.options.error",
        SettingsLogLevel::Warn => "settings.app_system.logging.level.options.warn",
        SettingsLogLevel::Info => "settings.app_system.logging.level.options.info",
        SettingsLogLevel::Debug => "settings.app_system.logging.level.options.debug",
        SettingsLogLevel::Trace => "settings.app_system.logging.level.options.trace",
    };
    bongocat_i18n::text(display_language.catalog_locale(), key)
}

pub(super) fn logging_level_from_display_name(
    name: &str,
    display_language: SettingsLanguage,
) -> Option<SettingsLogLevel> {
    SettingsLogLevel::ALL
        .into_iter()
        .find(|level| logging_level_display_name(*level, display_language) == name)
}

pub(super) fn logging_level_options(
    display_language: SettingsLanguage,
) -> [&'static str; SettingsLogLevel::ALL.len()] {
    SettingsLogLevel::ALL.map(|level| logging_level_display_name(level, display_language))
}

pub(super) fn logging_retention_number_field_options() -> NumberFieldOptions {
    NumberFieldOptions {
        min: 1.0,
        max: f64::from(bongocat_config::MAXIMUM_LOG_RETENTION_DAYS),
        step: 1.0,
    }
}

pub(super) fn check_for_updates_interval_number_field_options() -> NumberFieldOptions {
    NumberFieldOptions {
        min: 1.0,
        max: f64::from(bongocat_config::MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS),
        step: 1.0,
    }
}

pub(super) fn normalize_check_for_updates_interval_hours(raw: f64) -> u16 {
    let maximum = f64::from(bongocat_config::MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS);
    let value = if raw.is_nan() {
        f64::from(bongocat_config::DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS)
    } else {
        raw.round()
    };
    value.clamp(1.0, maximum) as u16
}

pub(super) fn normalize_logging_retention_days(raw: f64) -> u8 {
    let maximum = f64::from(bongocat_config::MAXIMUM_LOG_RETENTION_DAYS);
    let value = if raw.is_nan() {
        f64::from(bongocat_config::DEFAULT_LOG_RETENTION_DAYS)
    } else {
        raw.round()
    };
    value.clamp(1.0, maximum) as u8
}

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

/// Which counter a model behavior row draws its displayed number from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BehaviorKind {
    Motion,
    Expression,
}

/// A behavior's flattened position inside the active model's behavior list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BehaviorOrdinal {
    pub(super) kind: BehaviorKind,
    pub(super) number: usize,
}

/// What a model behavior row's play control asks for: one behavior of the model
/// the row belongs to.
///
/// The row keeps the behavior whole instead of re-parsing it out of the
/// binding's `behavior_id`, and carries the model key the row was built from
/// rather than looking the active model up again at click time — so the control
/// can only ever play what the row is actually showing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PlayableBehavior {
    pub(super) model: SettingsModelKey,
    pub(super) behavior: SettingsModelBehavior,
}

pub(super) struct ShortcutRow {
    pub(super) target: ShortcutCaptureTarget,
    /// Where this row sits in the active model's behavior list, for the rows
    /// that are model behaviors. `None` on an application command, which is
    /// named by the command itself.
    pub(super) behavior: Option<BehaviorOrdinal>,
    /// The behavior this row's binding fires, for the rows that are model
    /// behaviors. `None` on an application command.
    pub(super) playable: Option<PlayableBehavior>,
    pub(super) shortcut: Option<String>,
}

impl ShortcutRow {
    /// The label this row renders.
    pub(super) fn name(&self, language: SettingsLanguage) -> String {
        match &self.target {
            ShortcutCaptureTarget::Command(command) => {
                shortcut_command_name(language, command.as_str())
            }
            // Every model behavior row is built from the active model's ordered
            // behavior list, so it carries a position and this arm is the one
            // users see. The fallback keeps the row self-describing if one is
            // ever built outside that list; it prints the raw identity the way
            // the page used to, rather than panicking in a render path.
            ShortcutCaptureTarget::ModelBehavior { behavior_id, .. } => self.behavior.map_or_else(
                || behavior_id.clone(),
                |ordinal| shortcut_behavior_name(language, ordinal),
            ),
        }
    }
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
            behavior: None,
            playable: None,
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
    // Motions are numbered across every group before the expressions start,
    // which is the order the rows are listed in: a model's behaviors are "every
    // motion group in declaration order, then every expression" (see
    // `bongocat-app`'s `behavior_ids`). The two counters are what make the labels
    // continuous and unique per kind, so `motion:CAT_motion:1` and
    // `motion:CAT_motion_lock:0` read as "Motion 2" and "Motion 3" instead of
    // restarting at the group boundary.
    let mut motions = 0usize;
    let mut expressions = 0usize;
    rows.extend(behaviors.iter().map(|behavior| {
        let (kind, number) = match behavior {
            SettingsModelBehavior::Motion { .. } => {
                motions += 1;
                (BehaviorKind::Motion, motions)
            }
            SettingsModelBehavior::Expression { .. } => {
                expressions += 1;
                (BehaviorKind::Expression, expressions)
            }
        };
        let behavior_id = model_behavior_id(behavior);
        ShortcutRow {
            target: ShortcutCaptureTarget::ModelBehavior {
                model: model.clone(),
                behavior_id: behavior_id.clone(),
            },
            behavior: Some(BehaviorOrdinal { kind, number }),
            playable: Some(PlayableBehavior {
                model: model.clone(),
                behavior: behavior.clone(),
            }),
            shortcut: shortcuts
                .model_behaviors
                .iter()
                .find(|binding| binding.model == *model && binding.behavior_id == behavior_id)
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

#[cfg(test)]
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

/// The canonical `behavior_id` spelling, taken from `bongocat-config` rather
/// than re-implemented here.
///
/// The row lookup below is a plain string match between a binding's
/// `behavior_id` and this value, so a second copy of the format that drifted
/// would silently render every row without its chord. `bongocat-config` is the
/// single owner of the spelling; the auto-assignment and the import
/// normalization both go through it too.
fn model_behavior_id(behavior: &SettingsModelBehavior) -> String {
    match behavior {
        SettingsModelBehavior::Motion { group, index } => ModelBehaviorAction::Motion {
            group: group.clone(),
            index: *index,
        },
        SettingsModelBehavior::Expression { name } => {
            ModelBehaviorAction::Expression { name: name.clone() }
        }
    }
    .behavior_id()
}

/// The tab index of a row's capture control.
///
/// A row's controls are numbered as one group of three consecutive indices —
/// capture, play, clear — in the order they read from left to right. The stride
/// is fixed rather than derived from which controls a row actually renders, so a
/// row that leaves the play slot empty (every application command) keeps the
/// rows around it numbered where they were.
pub(super) fn shortcut_capture_tab_index(index: usize) -> isize {
    100_isize.saturating_add(
        isize::try_from(index)
            .unwrap_or(isize::MAX / 3)
            .saturating_mul(3),
    )
}

/// The tab index of a row's play control, between the capture and clear ones.
pub(super) fn shortcut_play_tab_index(index: usize) -> isize {
    shortcut_capture_tab_index(index).saturating_add(1)
}

pub(super) fn shortcut_clear_tab_index(index: usize) -> isize {
    shortcut_capture_tab_index(index).saturating_add(2)
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
        ShortcutCaptureTarget::ModelBehavior { model, behavior_id } => {
            if let Some(binding) = shortcuts
                .model_behaviors
                .iter_mut()
                .find(|binding| binding.model == *model && binding.behavior_id == *behavior_id)
            {
                binding.shortcut = shortcut;
            } else {
                shortcuts
                    .model_behaviors
                    .push(SettingsModelBehaviorBinding {
                        model: model.clone(),
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
        ShortcutCaptureTarget::ModelBehavior { model, behavior_id } => {
            let original_len = shortcuts.model_behaviors.len();
            shortcuts
                .model_behaviors
                .retain(|binding| binding.model != *model || binding.behavior_id != *behavior_id);
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

/// The first chord two *simultaneously live* bindings would fight over, if any.
///
/// The scopes mirror `NativeConfig::validate`: the application command list is
/// one scope, each model's own behavior list is another, and a model behavior
/// may not shadow a command either. Two models may hold the same chord — only
/// one model's behaviors are live at a time, and every model counts its
/// defaults from the first digit of the primary modifier — so a chord recorded
/// for the model the user is not on is not a conflict.
pub(super) fn conflicting_shortcut(shortcuts: &SettingsShortcuts) -> Option<String> {
    let mut command_chords = BTreeSet::new();
    for chord in shortcuts
        .commands
        .iter()
        .map(|binding| binding.shortcut.as_str())
        .filter_map(|value| ShortcutChord::parse(value).ok())
    {
        let canonical = chord.canonical();
        if !command_chords.insert(canonical.clone()) {
            return Some(canonical);
        }
    }
    let mut model_chords: BTreeMap<SettingsModelKey, BTreeSet<String>> = BTreeMap::new();
    for binding in &shortcuts.model_behaviors {
        let Some(chord) = ShortcutChord::parse(&binding.shortcut).ok() else {
            continue;
        };
        let canonical = chord.canonical();
        if command_chords.contains(&canonical)
            || !model_chords
                .entry(binding.model.clone())
                .or_default()
                .insert(canonical.clone())
        {
            return Some(canonical);
        }
    }
    None
}
