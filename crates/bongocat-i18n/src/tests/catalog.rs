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
    assert_eq!(text("zh-TW", "navigation.settings.title"), "BongoCat 設定");
    assert_eq!(
        text("ar-SA", "navigation.settings.title"),
        "إعدادات BongoCat"
    );
    assert_eq!(
        text("vi-VN", "navigation.settings.title"),
        "Cài đặt BongoCat"
    );
    assert_eq!(
        text("pt-BR", "navigation.settings.title"),
        "Configurações do BongoCat"
    );
    assert_eq!(
        text("system", "navigation.settings.title"),
        "BongoCat Settings"
    );
    assert_eq!(
        text("de-DE", "navigation.settings.title"),
        "BongoCat Settings"
    );
}

/// Every region variant of a shipped language reaches that language's catalog.
///
/// The catalogs are named after one primary variety each — `ar-SA`, `vi-VN`,
/// `en-US` — so the codes a machine actually reports are almost never the
/// shipped name itself. This is the RFC 4647 language-subtag fallback that
/// browsers' `Intl`, CLDR and the operating system all perform, and it is what
/// keeps an Egyptian or Saudi machine, a machine reporting a bare `ar` and a
/// machine reporting `vi` all landing on the same catalog instead of silently
/// dropping to English.
#[test]
fn a_region_variant_reaches_the_catalog_that_serves_its_language() {
    for (locale, shipped) in [
        ("ar", "ar-SA"),
        ("ar-EG", "ar-SA"),
        ("ar_EG", "ar-SA"),
        ("AR-sa", "ar-SA"),
        ("vi", "vi-VN"),
        ("vi-VN", "vi-VN"),
        // Brazilian Portuguese is the only Portuguese catalog, so a European
        // machine reaches it on the primary subtag exactly as an Egyptian one
        // reaches `ar-SA`.
        ("pt", "pt-BR"),
        ("pt-BR", "pt-BR"),
        ("pt-PT", "pt-BR"),
        ("pt_BR", "pt-BR"),
        ("en", "en-US"),
        ("en-GB", "en-US"),
        ("en_AU", "en-US"),
    ] {
        assert_eq!(
            locale_code(locale),
            shipped,
            "{locale} should resolve to the {shipped} catalog"
        );
        assert_eq!(
            text(locale, "navigation.settings.title"),
            text(shipped, "navigation.settings.title"),
            "{locale} rendered different copy from {shipped}"
        );
    }
}

/// A language the product does not ship falls back to the default catalog.
///
/// `locale_code` takes a free-form tag, so a language we ship no catalog for —
/// or a typo in a `code()` arm — lands on the default. That is the intended
/// fallback, and it is the only outcome for a tag whose primary subtag matches
/// nothing the product ships.
#[test]
fn a_language_the_product_does_not_ship_falls_back_to_the_default() {
    for locale in ["de-DE", "fr", "ja-JP", "ko-KR", "ru-RU"] {
        assert_eq!(
            text(locale, "navigation.settings.title"),
            text(DEFAULT, "navigation.settings.title"),
            "{locale} is not a language this product ships"
        );
    }
}

/// Simplified and Traditional are different scripts, not regional variants.
///
/// The primary subtag cannot separate them — both are `zh` — so the `zh` branch
/// decides by script subtag instead, and each form reaches its own catalog. A
/// Traditional tag must never read the Simplified catalog: the two are different
/// written forms, and the region subtag is the only thing a platform reports
/// when it omits `hant`/`hans`, so both spellings have to land correctly.
#[test]
fn each_chinese_script_reaches_its_own_catalog() {
    for locale in [
        "zh-TW",
        "zh-HK",
        "zh-MO",
        "zh-Hant",
        "zh-Hant-HK",
        "zh_Hant_TW",
    ] {
        assert_eq!(
            locale_code(locale),
            "zh-TW",
            "{locale} is Traditional Chinese"
        );
        assert_eq!(
            text(locale, "navigation.settings.title"),
            text("zh-TW", "navigation.settings.title"),
            "{locale} rendered different copy from zh-TW"
        );
    }
    for locale in ["zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh_CN"] {
        assert_eq!(
            locale_code(locale),
            "zh-CN",
            "{locale} is Simplified Chinese"
        );
    }
    // The two scripts are different catalogs, not one catalog read two ways.
    assert_ne!(
        text("zh-CN", "navigation.settings.title"),
        text("zh-TW", "navigation.settings.title")
    );
}
