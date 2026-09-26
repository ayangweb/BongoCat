//! The display name a model is given when it is imported or renamed.
//!
//! A title is user-visible, editable metadata, so it degrades in named steps: the
//! typed hint, then the source folder, then the generated store id. It never
//! becomes an empty string the configuration schema would reject.

use bongocat_config::Language;
use bongocat_model_store::MverInputMode;
use std::path::Path;

pub(crate) const MODEL_TITLE_MAXIMUM_CHARS: usize = 128;

/// The editable display name for a newly imported model: the UI sends the
/// chosen title (defaulting to the source folder name). A blank hint degrades
/// to the source folder name and then to the stable model id. The id itself
/// is a service-generated UUID and never derived from any of these names.
pub(crate) fn installed_model_title(hint: &str, source_root: &Path, fallback: &str) -> String {
    let hint = hint.trim();
    if !hint.is_empty() {
        let clipped = clamp_model_title(hint);
        if !clipped.is_empty() {
            return clipped;
        }
    }
    installed_model_title_from_source(source_root, fallback)
}

/// The display name for one converted mode of a BongoCatMver source.
///
/// One legacy source becomes several models at once, so they need to be told
/// apart in the model list: the source's own name is kept and the mode is
/// appended, which is the naming the community already uses for exported
/// models. The mode is reserved out of the title limit before the source name is
/// clipped, because it is the only thing distinguishing the three models. The
/// label is localized here rather than stored as a stable token, because a title
/// is user-visible text the user can edit afterwards, exactly like the hint the
/// settings page sends.
pub(crate) fn legacy_model_title(
    hint: &str,
    source_root: &Path,
    fallback: &str,
    label: &str,
) -> String {
    let base = installed_model_title(hint, source_root, fallback);
    let suffix = format!(" · {label}");
    let available = MODEL_TITLE_MAXIMUM_CHARS.saturating_sub(suffix.chars().count());
    let base = base
        .chars()
        .take(available)
        .collect::<String>()
        .trim_end()
        .to_owned();
    if base.is_empty() {
        return clamp_model_title(label);
    }
    clamp_model_title(&format!("{base}{suffix}"))
}

/// The localized name of one BongoCatMver input mode.
pub(crate) fn legacy_mode_label(language: Language, mode: MverInputMode) -> &'static str {
    let key = match mode {
        MverInputMode::Standard => "models.mver.mode.standard",
        MverInputMode::Keyboard => "models.mver.mode.keyboard",
        MverInputMode::Gamepad => "models.mver.mode.gamepad",
    };
    bongocat_i18n::text(bongocat_i18n::locale_code(language.code()), key)
}

/// Trim a display name to the length the configuration schema accepts.
pub(crate) fn clamp_model_title(value: &str) -> String {
    value
        .chars()
        .take(MODEL_TITLE_MAXIMUM_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Normalize a user-typed title before it is written to the metadata record.
///
/// The settings page sanitizes the field as it is typed, and the configuration
/// re-validates the record when it loads; this is the service's own gate in
/// between, so a caller that bypasses the page cannot store a title the next
/// config load would reject.
pub(crate) fn normalize_model_title(value: &str) -> Option<String> {
    let filtered: String = value
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let title = clamp_model_title(filtered.trim());
    (!title.is_empty()).then_some(title)
}

/// The source-folder default title; over-long or missing folder names
/// degrade to the model id.
///
/// The name itself comes from `bongocat_ui_protocol::model_source_display_name` so the
/// service's fallback and the settings page's pre-filled title agree. The source
/// is the folder a user picked; the shared rule also knows how to drop an archive
/// extension, which is what the `名字.zip` exported from that folder carries.
pub(crate) fn installed_model_title_from_source(source_root: &Path, fallback: &str) -> String {
    bongocat_ui_protocol::model_source_display_name(source_root, source_root.is_dir())
        .map(|name| clamp_model_title(&name))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}
