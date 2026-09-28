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

/// Resolve the persisted application language into a locale understood by the
/// translation backend. `system` is resolved by the platform/config layer.
pub fn locale_code(code: &str) -> &str {
    match code {
        "zh-CN" => "zh-CN",
        "en-US" | "system" => DEFAULT_LOCALE,
        _ => DEFAULT_LOCALE,
    }
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
