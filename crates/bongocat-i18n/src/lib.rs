#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

#[macro_use]
extern crate rust_i18n;

rust_i18n::i18n!("locales", fallback = "en-US");

// This value is injected from the locale file contents by build.rs. Keeping it
// in the crate's rustc inputs ensures catalog-only edits rebuild consumers.
#[doc(hidden)]
pub const CATALOG_REVISION: &str = env!("BONGOCAT_I18N_CATALOG_REVISION");

/// The locale used for the application fallback and the initial UI.
pub const DEFAULT_LOCALE: &str = "en-US";

/// Every catalog this crate ships, in the order the language selector shows.
///
/// The names are region-qualified the way a single-catalog-per-language release
/// is conventionally named: the subtag nominates the primary variety the copy
/// was written in, and it does not claim a regional specialisation the catalog
/// does not carry. `en-US` is the same kind of name — it serves `en-GB` just as
/// `ar-SA` serves `ar-EG`.
pub const SHIPPED_LOCALES: [&str; 7] = [
    "en-US", "zh-CN", "zh-TW", "ar-SA", "vi-VN", "pt-BR", "ko-KR",
];

/// Resolve any locale tag onto the catalog that serves it.
///
/// This is the RFC 4647 language-subtag fallback every other runtime already
/// does — browsers' `Intl`, CLDR, and the operating system itself all truncate
/// right-to-left until a known tag is hit, so `ar-EG` falls back to `ar` rather
/// than to English. Matching the primary subtag against the shipped catalogs is
/// that same rule, and it is what keeps a machine reporting `vi` or `vi-VN` or
/// `ar` or `ar-EG` or `pt` or `pt-PT` or `ko` on one catalog instead of seven.
///
/// `zh` is the one case that cannot be decided by the primary subtag alone.
/// Simplified and Traditional are different written forms rather than regional
/// variants, and both ship their own catalog. Every `zh` tag therefore resolves
/// from inside the `zh` branch by script subtag, which is the one thing the
/// region subtag cannot express on its own. Letting it fall through to the
/// subtag loop would hand Simplified-tagged machines Traditional copy, or the
/// other way round.
pub fn locale_code(code: &str) -> &str {
    let normalized = code.replace('_', "-").to_ascii_lowercase();
    let language = normalized.split('-').next().unwrap_or("");
    if language == "zh" {
        return if is_simplified_chinese(&normalized) {
            "zh-CN"
        } else {
            "zh-TW"
        };
    }
    SHIPPED_LOCALES
        .iter()
        .copied()
        .find(|shipped| {
            shipped
                .split_once('-')
                .is_some_and(|(shipped_language, _)| shipped_language == language)
        })
        .unwrap_or(DEFAULT_LOCALE)
}

/// Whether a `zh` tag names the Simplified script rather than the Traditional one.
///
/// The script subtag (`hant`/`hans`) is authoritative when it is present; the
/// `TW`/`HK`/`MO` region subtags are the spellings platforms use when it is not.
fn is_simplified_chinese(normalized: &str) -> bool {
    !normalized
        .split('-')
        .any(|subtag| matches!(subtag, "hant" | "tw" | "hk" | "mo"))
}

/// Translate a stable key for the requested locale.
pub fn text(locale: &str, key: &str) -> &'static str {
    static CACHE: OnceLock<RwLock<HashMap<String, &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let cache_key = format!("{}\0{key}", locale_code(locale));
    if let Some(value) = cache
        .read()
        .expect("translation cache read lock")
        .get(&cache_key)
    {
        return value;
    }
    let value = Box::leak(
        t!(key, locale = locale_code(locale))
            .to_string()
            .into_boxed_str(),
    );
    cache
        .write()
        .expect("translation cache write lock")
        .insert(cache_key, value);
    value
}

/// Interpolate named `%{name}` values in a translated message.
///
/// Keeping this tiny formatter in the i18n crate lets UI code remain free of
/// locale-specific branches while retaining rust-i18n's compile-time catalog.
pub fn format_text(locale: &str, key: &str, values: &[(&str, String)]) -> String {
    let mut message = text(locale, key).to_owned();
    for (name, value) in values {
        message = message.replace(&format!("%{{{name}}}"), value);
    }
    message
}

/// Stable identifier for the platform the catalog text targets.
///
/// Returned as the lower-case snake-case suffix used in catalog overrides such
/// as `settings.app_system.status_icon.label.macos`. Add a new value when a
/// newly supported platform needs its own copy of an otherwise-shared string.
pub const fn current_platform_id() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
}

/// Resolve a translation that can vary per supported platform.
///
/// Lookup order:
///   1. `base_key` suffixed with [`current_platform_id`]
///      (e.g. `..status_icon.label.macos`).
///   2. `base_key` as a shared fallback for every platform that has no
///      explicit override.
///
/// Adding a new platform override is a locale-only change: drop
/// `..label.<platform>` next to the existing `..label` and the override is
/// picked up automatically. Items that have no platform variation keep using
/// [`text`]; items that need a platform-specific copy swap the call site to
/// this helper without any new branching at the UI layer.
///
/// `rust-i18n` returns the key itself when the lookup misses, so the helper
/// compares the resolved string against the requested platform key to decide
/// whether to fall back. This keeps the contract identical to [`text`] for
/// every caller: missing keys still surface visibly in development rather
/// than silently returning an empty string.
pub fn platform_text(locale: &str, base_key: &str) -> &'static str {
    let platform_key = format!("{base_key}.{}", current_platform_id());
    let candidate = text(locale, &platform_key);
    if candidate != platform_key.as_str() {
        return candidate;
    }
    text(locale, base_key)
}

#[cfg(test)]
mod tests;
