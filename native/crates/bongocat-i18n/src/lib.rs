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
        value
            .as_object()
            .expect("object locale file")
            .iter()
            .filter(|(key, _)| key.as_str() != "_version")
            .map(|(key, value)| {
                (
                    key.clone(),
                    value.as_str().expect("string translation").to_owned(),
                )
            })
            .collect()
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
    fn missing_locale_text_falls_back_to_english() {
        assert_eq!(text("zh-CN", "ui.settings"), "BongoCat 设置");
        assert_eq!(text("system", "ui.settings"), "BongoCat Settings");
        assert_eq!(text("de-DE", "ui.settings"), "BongoCat Settings");
    }

    #[test]
    fn count_interpolation_is_available() {
        assert_eq!(
            count_text("en-US", "ui.runtime_shutdown_failures", 3),
            "Shutdown failures: 3"
        );
    }
}
