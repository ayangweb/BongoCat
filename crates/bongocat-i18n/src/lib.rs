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
    use std::path::{Path, PathBuf};

    /// Flatten a nested catalog object into dotted leaf keys.
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

    fn messages(locale: &str) -> BTreeMap<String, String> {
        let value: serde_json::Value = serde_json::from_str(match locale {
            "en-US" => include_str!("../locales/en-US.json"),
            "zh-CN" => include_str!("../locales/zh-CN.json"),
            _ => panic!("unsupported test locale"),
        })
        .expect("valid locale JSON");
        let mut messages = BTreeMap::new();
        flatten(&value, "", &mut messages);
        messages
    }

    /// The same flattening, read from the file a rebuild would read.
    ///
    /// Deliberately not `include_str!`: a copy embedded in this test binary would go stale
    /// together with the catalog it exists to check.
    fn messages_on_disk(locale: &str) -> BTreeMap<String, String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("locales/{locale}.json"));
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let value: serde_json::Value = serde_json::from_str(&source).expect("valid locale JSON");
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

    /// Platform override suffixes the catalog is expected to carry (ADR-0028).
    const PLATFORM_OVERRIDES: [&str; 2] = ["macos", "windows"];

    /// A catalog reduced to the two questions a source scan asks of it.
    struct Catalog {
        /// Leaf keys: what a lookup can resolve to.
        leaves: BTreeSet<String>,
        /// Every object and leaf path: what makes a dotted literal look like a key.
        paths: BTreeSet<String>,
    }

    impl Catalog {
        fn load(locale: &str) -> Self {
            let leaves = messages(locale).into_keys().collect::<BTreeSet<_>>();
            let mut paths = BTreeSet::new();
            for leaf in &leaves {
                let mut prefix = String::new();
                for segment in leaf.split('.') {
                    if !prefix.is_empty() {
                        prefix.push('.');
                    }
                    prefix.push_str(segment);
                    paths.insert(prefix.clone());
                }
            }
            Self { leaves, paths }
        }

        /// Whether a lookup resolves, mirroring the order `text` and `platform_text` use.
        ///
        /// A platform-relative lookup tries the suffixed key first and the base key second, so
        /// the base key only has to exist for a platform that carries no override of its own.
        fn resolves(&self, key: &str, platform_relative: bool) -> bool {
            if self.leaves.contains(key) {
                return true;
            }
            platform_relative
                && PLATFORM_OVERRIDES
                    .iter()
                    .all(|platform| self.leaves.contains(&format!("{key}.{platform}")))
        }

        /// Whether a dotted literal means to be a key: it is one, or its first two segments
        /// are. The second test is what keeps an unrelated dotted string whose first segment
        /// happens to match a namespace out of the scan.
        fn intends_a_key(&self, literal: &str) -> bool {
            if self.paths.contains(literal) {
                return true;
            }
            let mut segments = literal.split('.');
            match (segments.next(), segments.next()) {
                (Some(head), Some(second)) => self.paths.contains(&format!("{head}.{second}")),
                _ => false,
            }
        }
    }

    /// Every `.rs` file under `directory`, skipping build output.
    fn rust_sources(directory: &Path, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.expect("readable directory entry");
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "target" {
                    rust_sources(&path, out);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    /// The literal that is the second argument of the call whose `(` is at `open`.
    ///
    /// `None` when that argument is not a literal, which is how every key assembled at runtime
    /// is skipped. Parens are tracked so a first argument such as `language.catalog_locale()`
    /// does not end the search early.
    fn literal_argument(source: &str, open: usize) -> Option<(usize, &str)> {
        let bytes = source.as_bytes();
        let mut depth = 1_usize;
        let mut index = open + 1;
        while index < bytes.len() {
            match bytes[index] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return None;
                    }
                }
                b',' if depth == 1 => break,
                _ => {}
            }
            index += 1;
        }
        let mut start = index + 1;
        while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
            start += 1;
        }
        if bytes.get(start) != Some(&b'"') {
            return None;
        }
        let rest = &source[start + 1..];
        let length = rest.find('"')?;
        Some((start + 1, &rest[..length]))
    }

    /// `settings.overlay.behavior.title`: lower-case snake-case segments, at least two of them.
    fn is_catalog_key_shape(literal: &str) -> bool {
        let mut segments = literal.split('.');
        let Some(first) = segments.next() else {
            return false;
        };
        if !is_snake_segment(first) {
            return false;
        }
        let mut count = 1_usize;
        for segment in segments {
            if !is_snake_segment(segment) {
                return false;
            }
            count += 1;
        }
        count >= 2
    }

    fn is_snake_segment(segment: &str) -> bool {
        !segment.is_empty()
            && segment
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
    }

    /// Every dotted literal in `source` as `(offset of its first character, literal)`.
    fn dotted_literals(source: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        let mut index = 0;
        while let Some(open) = source[index..].find('"') {
            let start = index + open + 1;
            let Some(length) = source[start..].find('"') else {
                break;
            };
            let literal = &source[start..start + length];
            if is_catalog_key_shape(literal) {
                found.push((start, literal.to_owned()));
            }
            index = start + length + 1;
        }
        found
    }

    fn line_of(source: &str, offset: usize) -> usize {
        source[..offset].matches('\n').count() + 1
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
        for locale in ["en-US", "zh-CN"] {
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
                "update.current_version",
                &[("version", "1.2.3".to_string())],
            ),
            "Current version 1.2.3"
        );
        assert_eq!(
            format_text(
                "zh-CN",
                "update.current_version",
                &[("version", "1.2.3".to_string())],
            ),
            "当前版本 1.2.3"
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
            platform_text("zh-CN", "settings.application.status_icon.label"),
            match current_platform_id() {
                "macos" => text("zh-CN", "settings.application.status_icon.label.macos"),
                "windows" => text("zh-CN", "settings.application.status_icon.label.windows"),
                _ => text("zh-CN", "settings.application.status_icon.label"),
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

    /// Every catalog key the source asks for must exist in the catalog.
    ///
    /// `rust_i18n` answers an unknown key with the key itself, so a key that was renamed or
    /// dropped never fails a build, a unit test or a smoke run: the window quietly renders the
    /// raw key. The assertions that do cover catalog copy compare two lookups of the *same*
    /// key, so they stay equal even when the key is gone.
    ///
    /// Two scans run over every `.rs` file under `crates/`:
    ///
    /// 1. A literal handed straight to a catalog lookup must resolve. The lookup paths are
    ///    spelled with `concat!` so the scan cannot match this test's own source.
    /// 2. A dotted literal that already sits under a catalog path must resolve to a leaf, which
    ///    is what covers keys held in a `const` table, read through a local closure, or picked
    ///    by a `match` arm such as the settings error table.
    ///
    /// The one composition that survives is `platform_text`, which builds the platform-relative
    /// key from a literal base key; scan 1 covers it through the platform rule. Everything else
    /// is spelled out, which is why `bongocat-ui` names every settings error key in full rather
    /// than prefixing a suffix. A key whose *namespace* is misspelled stays uncovered, because
    /// scan 2 only recognises a literal once its first two segments are a real catalog path.
    #[test]
    fn source_referenced_keys_exist_in_the_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the crate lives at <workspace>/crates/bongocat-i18n")
            .join("crates");
        let mut sources = Vec::new();
        rust_sources(&root, &mut sources);
        assert!(
            sources
                .iter()
                .any(|path| path.ends_with("bongocat-ui/src/window/render.rs")),
            "expected the workspace source tree under {}, found {} files",
            root.display(),
            sources.len()
        );

        let mut catalogs = Vec::new();
        for locale in ["en-US", "zh-CN"] {
            catalogs.push((locale, Catalog::load(locale)));
        }
        let lookups = [
            (concat!("bongocat_i18n::", "text("), false),
            (concat!("bongocat_i18n::", "format_text("), false),
            (concat!("bongocat_i18n::", "platform_text("), true),
        ];
        let mut missing = BTreeSet::new();
        for source_path in &sources {
            let source = std::fs::read_to_string(source_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", source_path.display()));
            let mut looked_up = BTreeSet::new();
            for (lookup, platform_relative) in lookups {
                for (open, _) in source.match_indices(lookup) {
                    let Some((offset, key)) = literal_argument(&source, open + lookup.len()) else {
                        continue;
                    };
                    looked_up.insert(offset);
                    for (locale, catalog) in &catalogs {
                        if !catalog.resolves(key, platform_relative) {
                            missing.insert(format!(
                                "{}:{}: `{key}` is looked up but is missing from {locale}",
                                source_path.display(),
                                line_of(&source, offset)
                            ));
                        }
                    }
                }
            }
            for (offset, literal) in dotted_literals(&source) {
                if looked_up.contains(&offset) {
                    continue;
                }
                for (locale, catalog) in &catalogs {
                    // A literal with no visible lookup may still be a platform-relative base
                    // key, so both lookup shapes have to fail before it is reported.
                    if catalog.intends_a_key(&literal) && !catalog.resolves(&literal, true) {
                        missing.insert(format!(
                            "{}:{}: `{literal}` looks like a key but is missing from {locale}",
                            source_path.display(),
                            line_of(&source, offset)
                        ));
                    }
                }
            }
        }

        assert!(
            missing.is_empty(),
            "catalog keys referenced by the source do not exist:\n{}\n\
             Add the key to both locale files, or drop the reference.",
            missing.into_iter().collect::<Vec<_>>().join("\n")
        );
    }

    /// Every catalog key must be reachable from the source.
    ///
    /// The mirror of `source_referenced_keys_exist_in_the_catalog`. A key that no call site asks
    /// for is copy that ships without ever being shown: the settings window collected 22 of them
    /// before they were removed on 2026-09-20, and a cleanup commit then deleted a key that *was*
    /// still used. Both directions are checked so neither can come back.
    ///
    /// A leaf is reachable when its text appears as a literal anywhere under `crates/`, or when
    /// it is `<base>.<platform>` and `<base>` does. That second rule is how `platform_text`
    /// composes the key it looks up, and it is the only composition left in the workspace.
    #[test]
    fn catalog_keys_are_referenced_by_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the crate lives at <workspace>/crates/bongocat-i18n")
            .join("crates");
        let mut sources = Vec::new();
        rust_sources(&root, &mut sources);

        let mut literals = BTreeSet::new();
        for source_path in &sources {
            let source = std::fs::read_to_string(source_path)
                .unwrap_or_else(|error| panic!("read {}: {error}", source_path.display()));
            for (_, literal) in dotted_literals(&source) {
                literals.insert(literal);
            }
        }

        let mut unreferenced = BTreeSet::new();
        for locale in ["en-US", "zh-CN"] {
            for key in Catalog::load(locale).leaves {
                if literals.contains(&key) {
                    continue;
                }
                let platform_relative = key.rsplit_once('.').is_some_and(|(base, platform)| {
                    PLATFORM_OVERRIDES.contains(&platform) && literals.contains(base)
                });
                if !platform_relative {
                    unreferenced.insert(format!("{locale}: `{key}` is never looked up"));
                }
            }
        }

        assert!(
            unreferenced.is_empty(),
            "catalog keys that no source file asks for:\n{}\n\
             Remove the key from both locale files, or add the call site that uses it.",
            unreferenced.into_iter().collect::<Vec<_>>().join("\n")
        );
    }
}
