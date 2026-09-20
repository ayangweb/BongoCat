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
/// as `settings.application.status_icon.label.macos`. Add a new value when a
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
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "unsupported"
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
mod tests {
    use super::{current_platform_id, format_text, platform_text, text};
    use std::collections::{BTreeMap, BTreeSet};

    fn messages(locale: &str) -> BTreeMap<String, String> {
        let value: serde_json::Value = serde_json::from_str(match locale {
            "en-US" => include_str!("../locales/en-US.json"),
            "zh-CN" => include_str!("../locales/zh-CN.json"),
            _ => panic!("unsupported test locale"),
        })
        .expect("valid locale JSON");
        fn flatten(value: &serde_json::Value, prefix: &str, out: &mut BTreeMap<String, String>) {
            let Some(object) = value.as_object() else {
                if !prefix.is_empty() {
                    out.insert(
                        prefix.to_owned(),
                        value.as_str().expect("string translation").to_owned(),
                    );
                }
                return;
            };
            for (key, child) in object {
                if key == "_version" {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(child, &path, out);
            }
        }
        let mut messages = BTreeMap::new();
        flatten(&value, "", &mut messages);
        messages
    }

    fn placeholders(value: &str) -> BTreeSet<&str> {
        value
            .split("%{")
            .skip(1)
            .filter_map(|part| part.split('}').next())
            .collect()
    }

    #[test]
    fn locale_keys_and_placeholders_match_default_language() {
        let english = messages("en-US");
        let chinese = messages("zh-CN");
        assert_eq!(
            english.keys().collect::<Vec<_>>(),
            chinese.keys().collect::<Vec<_>>()
        );
        for key in english.keys() {
            assert_eq!(
                placeholders(&english[key]),
                placeholders(&chinese[key]),
                "placeholder mismatch for {key}"
            );
        }
    }

    #[test]
    fn locale_source_uses_nested_snake_case_keys() {
        for locale in ["en-US", "zh-CN"] {
            let value: serde_json::Value = serde_json::from_str(match locale {
                "en-US" => include_str!("../locales/en-US.json"),
                "zh-CN" => include_str!("../locales/zh-CN.json"),
                _ => unreachable!(),
            })
            .expect("valid locale JSON");
            fn visit(value: &serde_json::Value, path: &str) {
                if let Some(object) = value.as_object() {
                    for (key, child) in object {
                        if key != "_version" {
                            assert!(!key.contains('.'), "flat key at {path}: {key}");
                            assert!(
                                key.chars().all(|c| c.is_ascii_lowercase()
                                    || c == '_'
                                    || c.is_ascii_digit()),
                                "non-snake-case key at {path}: {key}"
                            );
                        }
                        visit(child, &format!("{path}.{key}"));
                    }
                }
            }
            visit(&value, locale);
        }
    }

    #[test]
    fn missing_locale_text_falls_back_to_english() {
        assert_eq!(text("zh-CN", "navigation.settings.title"), "BongoCat 设置");
        assert_eq!(
            text("system", "navigation.settings.title"),
            "BongoCat Settings"
        );
        assert_eq!(
            text("de-DE", "navigation.settings.title"),
            "BongoCat Settings"
        );
    }

    #[test]
    fn format_text_interpolation_is_available() {
        assert_eq!(
            format_text(
                "en-US",
                "diagnostics.configuration.backup_candidates",
                &[
                    ("count", "3".to_string()),
                    ("plural_suffix", "s".to_string()),
                ],
            ),
            "3 backup candidates checked"
        );
        assert_eq!(
            format_text(
                "zh-CN",
                "diagnostics.configuration.backup_candidates",
                &[
                    ("count", "5".to_string()),
                    ("plural_suffix", "".to_string())
                ],
            ),
            "已检查 5 个备份候选"
        );
    }

    #[test]
    fn current_platform_id_is_one_of_the_supported_platforms() {
        assert!(
            matches!(current_platform_id(), "macos" | "windows" | "unsupported"),
            "platform id must be a known suffix, got {:?}",
            current_platform_id()
        );
    }

    #[test]
    fn platform_text_picks_the_current_platform_override() {
        // The catalog always carries every supported override, so resolving
        // the platform-relative key returns the platform-specific copy on the
        // build host and the fallback copy on every other platform.
        let expected_override = match current_platform_id() {
            "macos" => text("en-US", "settings.application.status_icon.label.macos"),
            "windows" => text("en-US", "settings.application.status_icon.label.windows"),
            _ => text("en-US", "settings.application.status_icon.label"),
        };
        assert_eq!(
            platform_text("en-US", "settings.application.status_icon.label"),
            expected_override
        );
        assert_eq!(
            platform_text("zh-CN", "settings.application.status_icon.description"),
            match current_platform_id() {
                "macos" => {
                    text(
                        "zh-CN",
                        "settings.application.status_icon.description.macos",
                    )
                }
                "windows" => {
                    text(
                        "zh-CN",
                        "settings.application.status_icon.description.windows",
                    )
                }
                _ => text("zh-CN", "settings.application.status_icon.description"),
            }
        );
    }

    #[test]
    fn platform_text_falls_back_to_the_base_key_when_no_override_exists() {
        // `navigation.settings.title` carries no `.macos` or `.windows`
        // override, so the helper must always return the shared string for
        // every supported platform id.
        let base_key = "navigation.settings.title";
        let platform_key = format!("{base_key}.{}", current_platform_id());
        assert_eq!(
            text("en-US", &platform_key),
            platform_key,
            "test premise: no platform override exists for {base_key}"
        );
        assert_eq!(platform_text("en-US", base_key), text("en-US", base_key));
        assert_eq!(platform_text("zh-CN", base_key), text("zh-CN", base_key));
    }

    #[test]
    fn platform_text_is_safe_for_keys_without_a_platform_override_present() {
        // The fallback path is reachable even on supported platforms, so the
        // helper must keep returning the shared string rather than the
        // augmented key. This guards against a regression where the lookup
        // would start to return the suffixed key by accident.
        let base_key = "navigation.about.title";
        let resolved = platform_text("zh-CN", base_key);
        assert!(!resolved.ends_with(&format!(".{}", current_platform_id())));
    }
}
