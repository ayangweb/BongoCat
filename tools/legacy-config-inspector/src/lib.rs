#![forbid(unsafe_code)]

use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};

use serde::Serialize;
use serde_json::Value;

const STORE_NAMES: [&str; 5] = ["app", "general", "cat", "model", "shortcut"];
const TRANSIENT_MODEL_FIELDS: [&str; 7] = [
    "pressedKeys",
    "modelReady",
    "supportKeys",
    "motions",
    "expressions",
    "currentMotions",
    "currentExpressions",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionStatus {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectionReport {
    pub report_version: u32,
    pub status: InspectionStatus,
    pub stores: Vec<StoreReport>,
    pub settings: NormalizedSettings,
    pub inventory: Inventory,
    pub diagnostics: Vec<Diagnostic>,
}

impl InspectionReport {
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.status == InspectionStatus::Blocked
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreReport {
    pub name: &'static str,
    pub state: StoreState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreState {
    Valid,
    Missing,
    InvalidJson,
    WrongTopLevel,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub store: &'static str,
    pub field: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizedSettings {
    pub general: GeneralSettings,
    pub cat: CatSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneralSettings {
    pub autostart: bool,
    pub taskbar_visible: bool,
    pub tray_visible: bool,
    pub theme: String,
    pub language: String,
    pub auto_check_update: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            autostart: false,
            taskbar_visible: false,
            tray_visible: true,
            theme: "auto".to_owned(),
            language: "en-US".to_owned(),
            auto_check_update: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatSettings {
    pub mirror: bool,
    pub mouse_mirror: bool,
    pub motion_sound: bool,
    pub behavior: bool,
    pub auto_release_delay_seconds: u64,
    pub max_fps: u64,
    pub ignore_mouse: bool,
    pub visible: bool,
    pub pass_through: bool,
    pub always_on_top: bool,
    pub scale_percent: u64,
    pub opacity_percent: u64,
    pub radius_percent: u64,
    pub hide_on_hover: bool,
    pub hide_on_hover_delay_seconds: u64,
    pub keep_in_screen: bool,
}

impl Default for CatSettings {
    fn default() -> Self {
        Self {
            mirror: false,
            mouse_mirror: false,
            motion_sound: true,
            behavior: true,
            auto_release_delay_seconds: 3,
            max_fps: 60,
            ignore_mouse: false,
            visible: true,
            pass_through: false,
            always_on_top: false,
            scale_percent: 100,
            opacity_percent: 100,
            radius_percent: 0,
            hide_on_hover: false,
            hide_on_hover_delay_seconds: 0,
            keep_in_screen: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub window_layout_count: usize,
    pub model_count: usize,
    pub preset_model_count: usize,
    pub custom_model_count: usize,
    pub current_model_kind: CurrentModelKind,
    pub model_shortcut_count: usize,
    pub global_shortcut_count: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            window_layout_count: 0,
            model_count: 0,
            preset_model_count: 0,
            custom_model_count: 0,
            current_model_kind: CurrentModelKind::None,
            model_shortcut_count: 0,
            global_shortcut_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentModelKind {
    None,
    Preset,
    Custom,
    Unknown,
}

#[must_use]
pub fn inspect_dir(input_dir: &Path) -> InspectionReport {
    let mut stores = Vec::with_capacity(STORE_NAMES.len());
    let mut values = BTreeMap::new();
    let mut diagnostics = Vec::new();
    let mut blocked = false;

    for name in STORE_NAMES {
        let path = input_dir.join(format!("{name}.json"));
        let (state, value) = read_store(&path);
        if state != StoreState::Valid {
            blocked = true;
            diagnostics.push(Diagnostic {
                severity: Severity::Error,
                code: state.diagnostic_code(),
                store: name,
                field: "",
            });
        }
        stores.push(StoreReport { name, state });
        if let Some(value) = value {
            values.insert(name, value);
        }
    }

    let mut settings = NormalizedSettings {
        general: GeneralSettings::default(),
        cat: CatSettings::default(),
    };
    let mut inventory = Inventory::default();

    if let Some(app) = values.get("app") {
        inventory.window_layout_count = object_len_at(app, &["windowState"]);
    }
    if let Some(general) = values.get("general") {
        inspect_general(general, &mut settings.general, &mut diagnostics);
    }
    if let Some(cat) = values.get("cat") {
        inspect_cat(cat, &mut settings.cat, &mut diagnostics);
    }
    if let Some(model) = values.get("model") {
        inspect_models(model, &mut inventory, &mut diagnostics);
    }
    if let Some(shortcut) = values.get("shortcut") {
        inventory.global_shortcut_count = shortcut
            .as_object()
            .map(|values| {
                values
                    .values()
                    .filter(|value| value.as_str().is_some_and(|text| !text.is_empty()))
                    .count()
            })
            .unwrap_or_default();
    }

    InspectionReport {
        report_version: 1,
        status: if blocked {
            InspectionStatus::Blocked
        } else {
            InspectionStatus::Ready
        },
        stores,
        settings,
        inventory,
        diagnostics,
    }
}

fn read_store(path: &Path) -> (StoreState, Option<Value>) {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (StoreState::Missing, None);
        }
        Err(_) => return (StoreState::Unreadable, None),
    };
    let value: Value = match serde_json::from_reader(BufReader::new(file)) {
        Ok(value) => value,
        Err(_) => return (StoreState::InvalidJson, None),
    };
    if value.is_object() {
        (StoreState::Valid, Some(value))
    } else {
        (StoreState::WrongTopLevel, None)
    }
}

impl StoreState {
    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::Valid => "",
            Self::Missing => "store_missing",
            Self::InvalidJson => "store_invalid_json",
            Self::WrongTopLevel => "store_wrong_top_level",
            Self::Unreadable => "store_unreadable",
        }
    }
}

fn inspect_general(
    root: &Value,
    settings: &mut GeneralSettings,
    diagnostics: &mut Vec<Diagnostic>,
) {
    settings.autostart = resolve_bool(
        root,
        "general",
        &["app", "autostart"],
        &["autostart"],
        settings.autostart,
        diagnostics,
    );
    settings.taskbar_visible = resolve_bool(
        root,
        "general",
        &["app", "taskbarVisible"],
        &["taskbarVisibility", "taskbarVisible"],
        settings.taskbar_visible,
        diagnostics,
    );
    settings.tray_visible = resolve_bool(
        root,
        "general",
        &["app", "trayVisible"],
        &[],
        settings.tray_visible,
        diagnostics,
    );
    settings.theme = resolve_theme(root, diagnostics);
    settings.language = resolve_string(
        root,
        "general",
        &["appearance", "language"],
        &[],
        &settings.language,
        diagnostics,
    );
    settings.auto_check_update = resolve_bool(
        root,
        "general",
        &["update", "autoCheck"],
        &["autoCheckUpdate"],
        settings.auto_check_update,
        diagnostics,
    );

    for field in ["appearance.isDark", "isDark", "migrated"] {
        if value_at(root, &field.split('.').collect::<Vec<_>>()).is_some() {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: "ignored_derived_state",
                store: "general",
                field,
            });
        }
    }
}

fn inspect_cat(root: &Value, settings: &mut CatSettings, diagnostics: &mut Vec<Diagnostic>) {
    settings.mirror = resolve_bool(
        root,
        "cat",
        &["model", "mirror"],
        &["mirrorMode"],
        settings.mirror,
        diagnostics,
    );
    settings.mouse_mirror = resolve_bool(
        root,
        "cat",
        &["model", "mouseMirror"],
        &["mouseMirror"],
        settings.mouse_mirror,
        diagnostics,
    );
    settings.motion_sound = resolve_bool(
        root,
        "cat",
        &["model", "motionSound"],
        &[],
        settings.motion_sound,
        diagnostics,
    );
    settings.behavior = resolve_bool(
        root,
        "cat",
        &["model", "behavior"],
        &[],
        settings.behavior,
        diagnostics,
    );
    settings.auto_release_delay_seconds = resolve_u64(
        root,
        "cat",
        &["model", "autoReleaseDelay"],
        &[],
        settings.auto_release_delay_seconds,
        0..=60,
        diagnostics,
    );
    settings.max_fps = resolve_u64(
        root,
        "cat",
        &["model", "maxFPS"],
        &[],
        settings.max_fps,
        1..=240,
        diagnostics,
    );
    settings.ignore_mouse = resolve_bool(
        root,
        "cat",
        &["model", "ignoreMouse"],
        &[],
        settings.ignore_mouse,
        diagnostics,
    );
    settings.visible = resolve_bool(
        root,
        "cat",
        &["window", "visible"],
        &["visible"],
        settings.visible,
        diagnostics,
    );
    settings.pass_through = resolve_bool(
        root,
        "cat",
        &["window", "passThrough"],
        &["penetrable"],
        settings.pass_through,
        diagnostics,
    );
    settings.always_on_top = resolve_bool(
        root,
        "cat",
        &["window", "alwaysOnTop"],
        &["alwaysOnTop"],
        settings.always_on_top,
        diagnostics,
    );
    settings.scale_percent = resolve_u64(
        root,
        "cat",
        &["window", "scale"],
        &["scale"],
        settings.scale_percent,
        10..=400,
        diagnostics,
    );
    settings.opacity_percent = resolve_u64(
        root,
        "cat",
        &["window", "opacity"],
        &["opacity"],
        settings.opacity_percent,
        0..=100,
        diagnostics,
    );
    settings.radius_percent = resolve_u64(
        root,
        "cat",
        &["window", "radius"],
        &[],
        settings.radius_percent,
        0..=100,
        diagnostics,
    );
    settings.hide_on_hover = resolve_bool(
        root,
        "cat",
        &["window", "hideOnHover"],
        &[],
        settings.hide_on_hover,
        diagnostics,
    );
    settings.hide_on_hover_delay_seconds = resolve_u64(
        root,
        "cat",
        &["window", "hideOnHoverDelay"],
        &[],
        settings.hide_on_hover_delay_seconds,
        0..=60,
        diagnostics,
    );
    settings.keep_in_screen = resolve_bool(
        root,
        "cat",
        &["window", "keepInScreen"],
        &[],
        settings.keep_in_screen,
        diagnostics,
    );

    for field in ["model.single", "singleMode", "window.position"] {
        if value_at(root, &field.split('.').collect::<Vec<_>>()).is_some() {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: "historical_field_unclassified",
                store: "cat",
                field,
            });
        }
    }
    if value_at(root, &["migrated"]).is_some() {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "ignored_derived_state",
            store: "cat",
            field: "migrated",
        });
    }
}

fn inspect_models(root: &Value, inventory: &mut Inventory, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(models) = value_at(root, &["models"]).and_then(Value::as_array) {
        inventory.model_count = models.len();
        for model in models {
            match model.get("isPreset").and_then(Value::as_bool) {
                Some(true) => inventory.preset_model_count += 1,
                Some(false) => inventory.custom_model_count += 1,
                None => {}
            }
        }
    }

    inventory.current_model_kind = match value_at(root, &["currentModel"]) {
        None | Some(Value::Null) => CurrentModelKind::None,
        Some(value) => match value.get("isPreset").and_then(Value::as_bool) {
            Some(true) => CurrentModelKind::Preset,
            Some(false) => CurrentModelKind::Custom,
            None => CurrentModelKind::Unknown,
        },
    };
    inventory.model_shortcut_count = object_len_at(root, &["shortcuts"]);

    for field in TRANSIENT_MODEL_FIELDS {
        if value_at(root, &[field]).is_some() {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: "ignored_transient_state",
                store: "model",
                field,
            });
        }
    }
    if inventory.custom_model_count > 0 || inventory.current_model_kind == CurrentModelKind::Custom
    {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "custom_model_requires_validation",
            store: "model",
            field: "models",
        });
    }
}

fn resolve_bool(
    root: &Value,
    store: &'static str,
    nested: &[&str],
    legacy: &[&'static str],
    default: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    resolve_value(
        root,
        store,
        nested,
        legacy,
        default,
        Value::as_bool,
        diagnostics,
    )
}

fn resolve_string(
    root: &Value,
    store: &'static str,
    nested: &[&str],
    legacy: &[&'static str],
    default: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    resolve_value(
        root,
        store,
        nested,
        legacy,
        default.to_owned(),
        |value| {
            value
                .as_str()
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        },
        diagnostics,
    )
}

fn resolve_theme(root: &Value, diagnostics: &mut Vec<Diagnostic>) -> String {
    let nested = value_at(root, &["appearance", "theme"]);
    if let Some(theme) = nested.and_then(valid_theme) {
        if value_at(root, &["theme"]).is_some() {
            diagnostics.push(Diagnostic {
                severity: Severity::Info,
                code: "legacy_field_shadowed",
                store: "general",
                field: "theme",
            });
        }
        return theme.to_owned();
    }
    if nested.is_some() {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "field_invalid_value",
            store: "general",
            field: "appearance.theme",
        });
    }
    if let Some(theme) = value_at(root, &["theme"]).and_then(valid_theme) {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "legacy_field_fallback",
            store: "general",
            field: "theme",
        });
        return theme.to_owned();
    }
    "auto".to_owned()
}

fn valid_theme(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|theme| ["auto", "light", "dark"].contains(theme))
}

fn resolve_u64(
    root: &Value,
    store: &'static str,
    nested: &[&str],
    legacy: &[&'static str],
    default: u64,
    range: std::ops::RangeInclusive<u64>,
    diagnostics: &mut Vec<Diagnostic>,
) -> u64 {
    let value = resolve_value(
        root,
        store,
        nested,
        legacy,
        default,
        Value::as_u64,
        diagnostics,
    );
    let clamped = value.clamp(*range.start(), *range.end());
    if clamped != value {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "value_clamped",
            store,
            field: canonical_field(nested),
        });
    }
    clamped
}

fn resolve_value<T: Clone>(
    root: &Value,
    store: &'static str,
    nested: &[&str],
    legacy: &[&'static str],
    default: T,
    parse: impl Fn(&Value) -> Option<T>,
    diagnostics: &mut Vec<Diagnostic>,
) -> T {
    let nested_value = value_at(root, nested);
    if let Some(value) = nested_value.and_then(&parse) {
        for field in legacy {
            if value_at(root, &[*field]).is_some() {
                diagnostics.push(Diagnostic {
                    severity: Severity::Info,
                    code: "legacy_field_shadowed",
                    store,
                    field,
                });
            }
        }
        return value;
    }
    if nested_value.is_some() {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "field_invalid_type",
            store,
            field: canonical_field(nested),
        });
    }
    for field in legacy {
        if let Some(value) = value_at(root, &[*field]).and_then(&parse) {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "legacy_field_fallback",
                store,
                field,
            });
            return value;
        }
    }
    default
}

fn value_at<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(root, |value, key| value.get(key))
}

fn object_len_at(root: &Value, path: &[&str]) -> usize {
    value_at(root, path)
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len)
}

fn canonical_field(path: &[&str]) -> &'static str {
    match path {
        ["model", "autoReleaseDelay"] => "model.autoReleaseDelay",
        ["model", "maxFPS"] => "model.maxFPS",
        ["window", "scale"] => "window.scale",
        ["window", "opacity"] => "window.opacity",
        ["window", "radius"] => "window.radius",
        ["window", "hideOnHoverDelay"] => "window.hideOnHoverDelay",
        ["appearance", "theme"] => "appearance.theme",
        ["appearance", "language"] => "appearance.language",
        ["app", "autostart"] => "app.autostart",
        ["app", "taskbarVisible"] => "app.taskbarVisible",
        ["app", "trayVisible"] => "app.trayVisible",
        ["update", "autoCheck"] => "update.autoCheck",
        ["model", "mirror"] => "model.mirror",
        ["model", "mouseMirror"] => "model.mouseMirror",
        ["model", "motionSound"] => "model.motionSound",
        ["model", "behavior"] => "model.behavior",
        ["model", "ignoreMouse"] => "model.ignoreMouse",
        ["window", "visible"] => "window.visible",
        ["window", "passThrough"] => "window.passThrough",
        ["window", "alwaysOnTop"] => "window.alwaysOnTop",
        ["window", "hideOnHover"] => "window.hideOnHover",
        ["window", "keepInScreen"] => "window.keepInScreen",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "bongocat-legacy-inspector-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create isolated test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove isolated test directory");
        }
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../shared/config/legacy-pinia")
            .join(name)
    }

    fn copy_stores(source: &Path, destination: &Path) {
        for name in STORE_NAMES {
            let source_path = source.join(format!("{name}.json"));
            if source_path.exists() {
                fs::copy(source_path, destination.join(format!("{name}.json")))
                    .expect("copy fixture store");
            }
        }
    }

    fn serialized(report: &InspectionReport) -> String {
        serde_json::to_string_pretty(report).expect("serialize report")
    }

    #[test]
    fn default_fixture_is_ready() {
        let report = inspect_dir(&fixture("default"));

        assert!(!report.is_blocked());
        assert_eq!(report.inventory.model_count, 3);
        assert_eq!(report.inventory.preset_model_count, 3);
        assert_eq!(report.inventory.custom_model_count, 0);
        assert_eq!(report.inventory.global_shortcut_count, 0);
        assert_eq!(report.settings.cat.max_fps, 60);
    }

    #[test]
    fn nested_fields_shadow_legacy_values() {
        let report = inspect_dir(&fixture("upgraded-with-custom-model"));

        assert!(!report.is_blocked());
        assert!(!report.settings.general.autostart);
        assert_eq!(report.settings.general.theme, "light");
        assert!(!report.settings.cat.mirror);
        assert!(!report.settings.cat.pass_through);
        assert_eq!(report.settings.cat.scale_percent, 125);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "legacy_field_shadowed" && diagnostic.field == "mirrorMode"
        }));
    }

    #[test]
    fn reports_counts_without_leaking_sensitive_values() {
        let report = inspect_dir(&fixture("upgraded-with-custom-model"));
        let json = serialized(&report);

        assert_eq!(report.inventory.custom_model_count, 1);
        assert_eq!(report.inventory.model_shortcut_count, 2);
        assert_eq!(report.inventory.global_shortcut_count, 5);
        assert_eq!(
            report.inventory.current_model_kind,
            CurrentModelKind::Custom
        );
        for sensitive in [
            "$LEGACY_APP_DATA",
            "custom-demo",
            "Control+Alt+B",
            "Control+1",
            "KeyA",
            "示例 model",
        ] {
            assert!(!json.contains(sensitive), "report leaked {sensitive}");
        }
    }

    #[test]
    fn damaged_json_blocks_without_panicking() {
        let temp = TestDirectory::new("damaged");
        let source = fixture("damaged");
        copy_stores(&source, temp.path());
        fs::copy(
            source.join("cat.json.invalid"),
            temp.path().join("cat.json"),
        )
        .expect("copy damaged cat store");

        let report = inspect_dir(temp.path());

        assert!(report.is_blocked());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "store_invalid_json" && diagnostic.store == "cat"
        }));
    }

    #[test]
    fn missing_store_blocks() {
        let temp = TestDirectory::new("missing");
        copy_stores(&fixture("default"), temp.path());
        fs::remove_file(temp.path().join("shortcut.json")).expect("remove copied store");

        let report = inspect_dir(temp.path());

        assert!(report.is_blocked());
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "store_missing" && diagnostic.store == "shortcut"
        }));
    }

    #[test]
    fn repeated_inspection_is_byte_stable() {
        let expected = serialized(&inspect_dir(&fixture("upgraded-with-custom-model")));

        for _ in 0..10 {
            assert_eq!(
                serialized(&inspect_dir(&fixture("upgraded-with-custom-model"))),
                expected
            );
        }
    }

    #[test]
    fn inspection_preserves_source_files() {
        let source = fixture("upgraded-with-custom-model");
        let before: Vec<_> = STORE_NAMES
            .iter()
            .map(|name| fs::read(source.join(format!("{name}.json"))).expect("read fixture"))
            .collect();

        let _ = inspect_dir(&source);

        let after: Vec<_> = STORE_NAMES
            .iter()
            .map(|name| fs::read(source.join(format!("{name}.json"))).expect("read fixture"))
            .collect();
        assert_eq!(after, before);
    }

    #[test]
    fn out_of_range_values_are_clamped_with_diagnostics() {
        let temp = TestDirectory::new("clamp");
        copy_stores(&fixture("default"), temp.path());
        let cat_path = temp.path().join("cat.json");
        let mut cat: Value =
            serde_json::from_slice(&fs::read(&cat_path).expect("read cat")).expect("parse cat");
        cat["model"]["maxFPS"] = Value::from(10_000);
        cat["window"]["opacity"] = Value::from(200);
        fs::write(
            cat_path,
            serde_json::to_vec_pretty(&cat).expect("serialize cat"),
        )
        .expect("write isolated cat fixture");

        let report = inspect_dir(temp.path());

        assert_eq!(report.settings.cat.max_fps, 240);
        assert_eq!(report.settings.cat.opacity_percent, 100);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "value_clamped")
                .count(),
            2
        );
    }

    #[test]
    fn invalid_nested_theme_falls_back_to_valid_legacy_theme() {
        let temp = TestDirectory::new("theme-fallback");
        copy_stores(&fixture("upgraded-with-custom-model"), temp.path());
        let general_path = temp.path().join("general.json");
        let mut general: Value =
            serde_json::from_slice(&fs::read(&general_path).expect("read general"))
                .expect("parse general");
        general["appearance"]["theme"] = Value::from("unsupported");
        fs::write(
            general_path,
            serde_json::to_vec_pretty(&general).expect("serialize general"),
        )
        .expect("write isolated general fixture");

        let report = inspect_dir(temp.path());

        assert_eq!(report.settings.general.theme, "dark");
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "field_invalid_value" && diagnostic.field == "appearance.theme"
        }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "legacy_field_fallback" && diagnostic.field == "theme"
        }));
    }
}
