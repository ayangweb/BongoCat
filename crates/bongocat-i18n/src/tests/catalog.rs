//! The catalogs hold what the default language holds.

use super::*;

#[test]
fn locale_keys_and_placeholders_match_default_language() {
    let reference = messages(DEFAULT);
    for locale in LOCALES.into_iter().filter(|locale| *locale != DEFAULT) {
        let current = messages(locale);
        assert_eq!(
            reference.keys().collect::<Vec<_>>(),
            current.keys().collect::<Vec<_>>(),
            "{locale} does not hold the keys the default language holds"
        );
        for key in reference.keys() {
            assert_eq!(
                placeholders(&reference[key]),
                placeholders(&current[key]),
                "placeholder mismatch for {key} in {locale}"
            );
        }
    }
}

/// The catalog that ships must agree with the file it was built from.
///
/// `rust_i18n::i18n!` resolves through a proc macro that reads the locale files while it
/// expands, so the compiler records no dependency on them: `dep-lib-bongocat_i18n` lists
/// `src/lib.rs` and nothing else. Freshness therefore rests entirely on `build.rs`'s
/// `rerun-if-changed` plus the revision it injects. Miss that path — a build that only
/// relinks the UI, a stale fingerprint, an editor that leaves the mtime behind — and the
/// crate keeps serving the previous copy while every other check stays green, because
/// `validate-locales.py` and both key scans above read the JSON rather than the catalog that
/// actually ships. The window then renders retired copy: on 2026-09-21 the model delete
/// confirmation displayed the pre-rename template `%{status} · %{confirm_deletion}` with
/// every gate passing.
#[test]
fn compiled_catalog_matches_the_files_on_disk() {
    for locale in LOCALES {
        for (key, expected) in messages_on_disk(locale) {
            assert_eq!(
                text(locale, &key),
                expected,
                "{locale}: `{key}` differs from locales/{locale}.json — this build is serving \
                 a stale catalog; rebuild with `cargo build -p bongocat-i18n`"
            );
        }
    }
}

#[test]
fn locale_source_uses_nested_snake_case_keys() {
    for locale in LOCALES {
        let value: serde_json::Value =
            serde_json::from_str(source(locale)).expect("valid locale JSON");
        fn visit(value: &serde_json::Value, path: &str) {
            if let Some(object) = value.as_object() {
                for (key, child) in object {
                    if key != "_version" {
                        assert!(!key.contains('.'), "flat key at {path}: {key}");
                        assert!(
                            key.chars()
                                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
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
    assert_eq!(text("ar", "navigation.settings.title"), "إعدادات BongoCat");
    assert_eq!(text("vi", "navigation.settings.title"), "Cài đặt BongoCat");
    assert_eq!(
        text("system", "navigation.settings.title"),
        "BongoCat Settings"
    );
    assert_eq!(
        text("de-DE", "navigation.settings.title"),
        "BongoCat Settings"
    );
}

/// A locale the product never asks for must resolve to the default catalog.
///
/// `text` takes a free-form locale string, so a code the crate does not ship —
/// a regional variant, or a typo in a `code()` arm — silently falls through to
/// the default. That is the intended fallback, and the two assertions above
/// already cover it; this one pins the answer for the regional codes a machine
/// may report, so a future `ar-SA` or `vi-VN` catalog cannot appear without
/// this test being updated to say which one the product intends.
#[test]
fn a_regional_locale_the_product_does_not_ship_falls_back_to_the_default() {
    for locale in ["de-DE", "ar-SA", "ar-EG", "vi-VN"] {
        assert_eq!(
            text(locale, "navigation.settings.title"),
            text(DEFAULT, "navigation.settings.title"),
            "{locale} is not a catalog this product ships"
        );
    }
}
