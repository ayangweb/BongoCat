#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

#[macro_use]
extern crate rust_i18n;

rust_i18n::i18n!("locales", fallback = "en-US");

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

/// Translate a message containing a `count` interpolation.
pub fn count_text(locale: &str, key: &str, count: impl std::fmt::Display) -> String {
    t!(key, locale = locale_code(locale), count = count).to_string()
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

#[cfg(test)]
mod tests {
    use super::{count_text, text};
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
    fn count_interpolation_is_available() {
        assert_eq!(
            count_text("en-US", "diagnostics.runtime.shutdown_failures", 3),
            "Shutdown failures: 3"
        );
    }
}
