//! Every key the source names is in the catalog, and the reverse.

use super::*;

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
