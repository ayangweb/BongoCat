//! The catalog's tests, split by what they are about.
//!
//! The fixtures are here rather than in one of the files because every question
//! needs them: a key check, a placeholder check and a coverage check all start
//! from the same flattened catalog and the same scan of the source.

use super::{current_platform_id, format_text, locale_code, platform_text, text};
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

/// Every catalog the crate ships, in a fixed order.
///
/// The tests that compare catalogs read this instead of repeating the list. A
/// locale added to `locales/` and to the `messages` match below but not here
/// would leave every comparison silently covering one catalog fewer, which is
/// exactly the failure this constant exists to make impossible.
pub(super) const LOCALES: [&str; 5] = ["en-US", "zh-CN", "zh-TW", "ar-SA", "vi-VN"];

/// The locale every other catalog is compared against.
pub(super) const DEFAULT: &str = "en-US";

/// The catalog file a locale is compiled from, embedded at build time.
///
/// Every test that reads a catalog goes through here, so `LOCALES` and this
/// match cannot disagree: a locale listed in one and absent from the other is a
/// compile error in `messages` rather than a silently uncovered catalog.
fn source(locale: &str) -> &'static str {
    match locale {
        "en-US" => include_str!("../../locales/en-US.json"),
        "zh-CN" => include_str!("../../locales/zh-CN.json"),
        "zh-TW" => include_str!("../../locales/zh-TW.json"),
        "ar-SA" => include_str!("../../locales/ar-SA.json"),
        "vi-VN" => include_str!("../../locales/vi-VN.json"),
        _ => panic!("unsupported test locale"),
    }
}

fn messages(locale: &str) -> BTreeMap<String, String> {
    let value: serde_json::Value = serde_json::from_str(source(locale)).expect("valid locale JSON");
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

mod catalog;
mod coverage;
mod format;
mod platform;
