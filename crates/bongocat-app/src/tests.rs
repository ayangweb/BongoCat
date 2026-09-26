//! Test fixtures shared by the application test modules.
//!
//! The tests live in this directory rather than beside the code they cover, so a
//! production module holds only production code. Everything here is a fixture a
//! sibling module reaches through `use super::*`: the repository's own model
//! packages, a legacy BongoCatMver source, and the frame and commit waits the
//! render-driven tests share.

use crate::*;

// The test prelude. These were one module's imports when every test in the crate
// shared a single `mod tests`; re-exporting them here lets a test module reach
// them through `use super::*` instead of repeating the list.
pub(crate) use crate::import_progress::ImportProgressAccumulator;
pub(crate) use crate::model_input::input_bindings_for_model;
pub(crate) use crate::model_titles::legacy_model_title;
pub(crate) use crate::shortcut_config::without_removed_model_targets;

pub(crate) use bongocat_config::{
    ConfigStore, GamepadAutoSwitchConfig, ImportedModelMetadata, Language, ModelIdentity,
    ModelInputMode, ModelSource, NativeConfig, StorageLayout, Theme as ConfigTheme,
};
pub(crate) use bongocat_input::{GamepadAxisSettings, GamepadButton, HandSide, PhysicalKey};
pub(crate) use bongocat_live2d_render::KeyImageInventory;
pub(crate) use bongocat_model::{
    InstalledModel, ModelCatalogEntry, ModelOrigin, ModelPackageLimits,
};
pub(crate) use bongocat_model_store::{ModelImportProgress, ModelImportStage, MverInputMode};
pub(crate) use bongocat_render::{
    KeyIdentity, KeySide, ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome,
    ModelCommitToken, RenderConsumer,
};
pub(crate) use bongocat_runtime::{
    GamepadAxis, GamepadAxisKey, GamepadAxisSample, GamepadButtonKey, InputControl, InputEdge,
    InputEvent, InputSource, ModelSettings, MonotonicMillis, OverlaySettings,
    RandomBehaviorSettings, RuntimeState,
};
pub(crate) use std::{fs, path::Path, time::Instant};
pub(crate) use tempfile::tempdir;

mod diagnostics;
mod gamepad;
mod model_catalog;
mod model_import;
mod model_input;
mod shortcuts;
mod shutdown;
mod startup;

/// Import a single-model package and return the one model it installed.
///
/// The product's import entry point handles sources that describe several
/// models at once. Every source in this module is a BongoCat package, so the
/// count is asserted here instead of at each call site.
fn import_one(
    application: &mut Application,
    title_hint: &str,
    source: impl AsRef<Path>,
) -> InstalledModel {
    let mut models = application
        .import_models(title_hint, source)
        .expect("import a single model package");
    assert_eq!(
        models.len(),
        1,
        "a package source installs exactly one model"
    );
    models.pop().expect("one installed model")
}

/// One opaque key cap and one semi-transparent paw, as exact PNG bytes so
/// the fixture does not need a bitmap decoder in the test build.
const LEGACY_KEY_CAP_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xa9, 0xf1, 0x9e,
    0x7e, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60, 0x60, 0xf8, 0xff,
    0x1f, 0x15, 0x93, 0x2c, 0x00, 0x00, 0x1c, 0x60, 0x1f, 0xe1, 0xcb, 0x7f, 0x73, 0xf9, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];
const LEGACY_SECOND_KEY_CAP_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xa9, 0xf1, 0x9e,
    0x7e, 0x00, 0x00, 0x00, 0x0f, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60, 0xf8, 0x8f, 0x06,
    0x49, 0x17, 0x00, 0x00, 0x2c, 0x50, 0x1f, 0xe1, 0x45, 0xaf, 0x33, 0x10, 0x00, 0x00, 0x00, 0x00,
    0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];
const LEGACY_PAW_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x04, 0x08, 0x06, 0x00, 0x00, 0x00, 0xa9, 0xf1, 0x9e,
    0x7e, 0x00, 0x00, 0x00, 0x12, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8, 0xcf, 0xc0, 0xd0,
    0x80, 0x8c, 0x19, 0x48, 0x17, 0x00, 0x00, 0x3a, 0x39, 0x17, 0xf1, 0x3b, 0x56, 0x2b, 0xf5, 0x00,
    0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn write_fixture_file(root: &Path, reference: &str, bytes: &[u8]) {
    let path = root.join(reference);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture directory");
    fs::write(path, bytes).expect("write fixture file");
}

/// Write a minimal BongoCatMver application folder.
///
/// The layout is the legacy application's: the mode key table at the root,
/// and one resource folder per mode holding the Live2D package, the paw and
/// key-cap layers a conversion composes, and the mode's background and
/// cover. The gamepad section addresses buttons with XInput indices, the way
/// the legacy config does.
fn legacy_source_fixture(root: &Path) {
    let mut config = serde_json::Map::new();
    for (mode, section) in [
        ("standard", r#"{"hand":[[65],[66]],"keyboard":[[65],[66]]}"#),
        (
            "keyboard",
            r#"{"lefthand":[[65]],"righthand":[[37]],"keyboard":[[65],[37]]}"#,
        ),
        (
            "gamepad",
            // XInput button indices: 12 is D-pad up, 6 the left analog
            // trigger, 0 the face button the chart calls A and 9 Start.
            r#"{"lefthand":[[12],[6]],"righthand":[[0],[9]],"keyboard":[[12],[6],[0],[9]]}"#,
        ),
    ] {
        config.insert(
            mode.to_owned(),
            serde_json::from_str(section).expect("legacy mode section"),
        );
    }
    write_fixture_file(
        root,
        "config.json",
        &serde_json::to_vec(&serde_json::Value::Object(config)).expect("legacy config"),
    );

    for mode in ["standard", "keyboard", "gamepad"] {
        let base = format!("img/{mode}");
        write_fixture_file(
            root,
            &format!("{base}/cat_model/cat.model3.json"),
            br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        );
        write_fixture_file(root, &format!("{base}/cat_model/model.moc3"), b"moc");
        // Four key caps, because the two split modes read one shared atlas
        // whose right-hand half continues where the left-hand half stopped:
        // two left-hand bindings plus two right-hand bindings need indices
        // 0 … 3.
        for (index, key_cap) in [
            LEGACY_KEY_CAP_PNG,
            LEGACY_SECOND_KEY_CAP_PNG,
            LEGACY_SECOND_KEY_CAP_PNG,
            LEGACY_KEY_CAP_PNG,
        ]
        .into_iter()
        .enumerate()
        {
            write_fixture_file(root, &format!("{base}/keyboard/{index}.png"), key_cap);
        }
        for hand in ["hand", "lefthand", "righthand"] {
            write_fixture_file(root, &format!("{base}/{hand}/0.png"), LEGACY_PAW_PNG);
            write_fixture_file(root, &format!("{base}/{hand}/1.png"), LEGACY_PAW_PNG);
        }
        for asset in ["mousebg.png", "bg.png", "cat.png"] {
            write_fixture_file(root, &format!("{base}/{asset}"), LEGACY_KEY_CAP_PNG);
        }
    }
}

/// The key images a shipped preset model carries. An imported model brings
/// its own artwork, so the presets stand in for "a package that ships this
/// side" and "a package that ships nothing".
fn shipped_key_images(id: &str) -> KeyImageInventory {
    let model = bongocat_model::PresetModelCatalog::open(
        repository_preset_root(),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse(id).expect("model id"))
    .expect("preset model");
    KeyImageInventory::read(model.root())
}

/// Every HID usage the key vocabulary covers, which is the set the binding
/// table has to stay consistent with: the main block (punctuation,
/// PrintScreen and the navigation cluster included), keypad `=`, F13 … F24,
/// the eight modifier usages, and the Apple Fn / globe key. `0x66` (Power) is
/// the only gap, because neither adapter maps it.
///
/// This is a **superset** of what either adapter can produce, not a list of
/// what they do produce. Naming and binding deliberately cover more than the
/// hardware delivers, so a model shipping artwork for a key no keyboard can
/// press is still honoured; each adapter's own tests pin what it can really
/// report (`bongocat-platform`'s
/// `this_adapter_reports_exactly_the_keycodes_the_platform_defines` for
/// macOS, the scan-code matrix for Windows). Do not read a usage being listed
/// here as proof that the key is reachable — `Apps` (`0x65`) sat in this list
/// for as long as the vocabulary existed while no adapter mapped it, which is
/// why `Apps.png` could never be drawn.
///
/// The modifier block is written out instead of relying on the ranges the
/// implementation uses: it sits above `0x04..=0x65` and below nothing else,
/// so a rewrite of the binding loops dropped Shift, Control, Alt and Meta
/// without any range looking wrong. The globe key is written out for the
/// same reason and one more: it is not on the Keyboard/Keypad page at all,
/// so no range over that page can ever be made to include it.
fn adapter_keyboard_usages() -> Vec<u16> {
    (0x04..=0x65)
        .chain([0x67])
        .chain(0x68..=0x73)
        .chain(0xe0..=0xe7)
        .chain([bongocat_render::GLOBE_KEY_USAGE])
        .collect()
}

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_owned()
}

/// The platform's command modifier, spelled the way the canonical chord
/// strings spell it.
fn behavior_shortcut_primary_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Meta"
    } else {
        "Control"
    }
}

/// The same modifier as a config bit set, for the one test that drives the
/// runtime instead of reading a chord string back.
fn behavior_shortcut_primary_modifiers() -> bongocat_config::ShortcutModifiers {
    let bits = if cfg!(target_os = "macos") {
        bongocat_config::ShortcutModifiers::META
    } else {
        bongocat_config::ShortcutModifiers::CONTROL
    };
    bongocat_config::ShortcutModifiers::from_bits(bits).expect("valid modifiers")
}

/// Seed the environment model store with a package stored under an exact
/// id. Imports always generate UUID ids, so a store entry whose id collides
/// with a preset id can only be produced through direct seeding; the merged
/// catalog must still keep both identities.
fn seed_installed_model(models_root: &Path, id: &str) {
    let destination = models_root.join(id);
    let fixture = repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型");
    copy_fixture_tree(&fixture, &destination);
}

fn copy_fixture_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("seeded model directory");
    for entry in std::fs::read_dir(source).expect("fixture entries") {
        let entry = entry.expect("fixture entry");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("fixture file type").is_dir() {
            copy_fixture_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("seeded package file");
        }
    }
}

fn installed_catalog_ids(application: &Application) -> Vec<String> {
    application
        .model_catalog()
        .expect("model catalog")
        .into_iter()
        .filter(|entry| entry.origin() == bongocat_model::ModelOrigin::Installed)
        .map(|entry| entry.id().as_str().to_owned())
        .collect()
}

/// Every catalog id in the order the Model library page shows them.
fn catalog_ids(application: &Application) -> Vec<String> {
    application
        .model_catalog()
        .expect("model catalog")
        .into_iter()
        .map(|entry| entry.id().as_str().to_owned())
        .collect()
}

fn wait_for_model_commit_frame(
    consumer: &RenderConsumer,
    token: ModelCommitToken,
) -> bongocat_render::RenderFrame {
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    loop {
        if let Some(frame) = consumer.take_latest()
            && frame.model_commit == Some(token)
        {
            return frame;
        }
        assert!(Instant::now() < deadline, "model frame timed out");
        std::thread::yield_now();
    }
}

fn wait_for_any_model_commit_frame(consumer: &RenderConsumer) -> bongocat_render::RenderFrame {
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    loop {
        if let Some(frame) = consumer.take_latest()
            && frame.model_commit.is_some()
        {
            return frame;
        }
        assert!(Instant::now() < deadline, "model frame timed out");
        std::thread::yield_now();
    }
}

/// The next frame whose overlays satisfy `predicate`.
fn wait_for_render_frame(
    consumer: &RenderConsumer,
    predicate: impl Fn(&bongocat_render::RenderSnapshot) -> bool,
) -> bongocat_render::RenderFrame {
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    loop {
        if let Some(frame) = consumer.take_latest()
            && predicate(&frame.snapshot)
        {
            return frame;
        }
        assert!(Instant::now() < deadline, "render frame timed out");
        std::thread::yield_now();
    }
}
/// The model packages this repository ships, which every render-driven test
/// activates instead of building a package of its own.
pub(crate) fn repository_preset_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .join("resources/models")
}
