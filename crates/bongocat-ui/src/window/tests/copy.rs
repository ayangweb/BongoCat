//! The localized copy each page and row shows.

use super::*;

/// A window to read a report from, standing in for the real one.
///
/// The report takes the scale factor and the size from the window rather than
/// from the snapshot, so every test that builds a document has to say what the
/// window looked like. Two logical pixels at 1.5 is the layout case the
/// acceptance criteria name, so the fixture is that one.
fn window_facts() -> crate::window::about::WindowFacts {
    crate::window::about::WindowFacts::new(1.5, 800, 600)
}

/// The report a user pastes, parsed back out of the JSON the clipboard receives.
fn report_of(snapshot: &SettingsSnapshot) -> serde_json::Value {
    let json = crate::window::about::SoftwareInformation::read(snapshot, window_facts())
        .to_json()
        .expect("serialize the report");
    serde_json::from_str(&json).expect("the report is valid JSON")
}

#[test]
fn build_information_is_localized_and_contains_only_compiled_identity() {
    let product_version = env!("CARGO_PKG_VERSION");
    let build_info = crate::SettingsBuildInfo {
        product_version: product_version.to_owned(),
        environment: crate::SettingsBuildEnvironment::Development,
        cubism_core_version: "6.0.1".to_owned(),
    };
    let detail = build_info_detail(SettingsLanguage::English, &build_info);
    assert_eq!(
        detail,
        format!("Version {product_version} · Development build")
    );
    assert!(!detail.contains('/'));
    assert!(!detail.contains("path"));

    let chinese = build_info_detail(SettingsLanguage::ChineseSimplified, &build_info);
    assert_eq!(chinese, format!("版本 {product_version} · 开发版"));
}

/// The report has to survive being pasted into an issue as a code block, which
/// means the thing that is copied has to parse. A user who edits it by hand, or
/// pastes it through something that reformats it, is not the failure this
/// guards; a document that is not valid JSON is.
#[test]
fn copied_software_information_is_a_json_document() {
    let json = crate::window::about::SoftwareInformation::read(
        &crate::tests::snapshot(1, true, true),
        window_facts(),
    )
    .to_json()
    .expect("serialize the report");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("the report is valid JSON");
    assert_eq!(parsed["app_name"], "BongoCat");
    assert_eq!(parsed["app_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(parsed["build_environment"], "development");
    assert_eq!(parsed["runtime_health"], "ready");
    assert_eq!(parsed["input_service_status"], "not_started");
    assert_eq!(parsed["platform"], std::env::consts::OS);
    assert_eq!(parsed["platform_arch"], std::env::consts::ARCH);
    assert!(
        json.contains('\n'),
        "the report is pretty-printed for a human"
    );
}

/// A report is read by someone triaging many of them, so the fields a triage
/// actually turns on have to be present rather than optional. Each one names a
/// question a maintainer asks before they can act: what was built, what does it
/// run on, and is the input pipeline even working.
#[test]
fn the_report_names_the_facts_a_bug_triage_needs() {
    let parsed = report_of(&crate::tests::snapshot(1, true, true));
    let object = parsed.as_object().expect("the report is an object");
    for field in [
        "app_name",
        "app_version",
        "build_environment",
        "cubism_core_version",
        "platform",
        "platform_arch",
        "platform_version",
        "platform_build",
        "locale",
        "runtime_health",
        "runtime_error_code",
        "input_service_status",
        "input_capability",
        "input_capability_available",
        "connected_gamepad_count",
        "input_release_reconciliations",
        "active_model_origin",
        "ready_installed_model_count",
        "invalid_model_count",
        "ui_scale_factor",
        "ui_window_size",
    ] {
        assert!(
            object.contains_key(field),
            "the report is missing {field}, which a triage needs"
        );
    }
    // A Core version is what tells a maintainer whether a model that will not
    // load is malformed or simply older than the Core this build ships.
    assert!(
        !object["cubism_core_version"]
            .as_str()
            .expect("a Cubism version string")
            .is_empty()
    );
}

/// The report has to name the capability the running platform actually gates
/// input behind, with the state this process is in.
///
/// A Windows report used to say `input_monitoring_permission: "unsupported"`,
/// which reads as "this platform has no such concept" when the truth is "this
/// platform gates input behind elevation, and you are not elevated" — the one
/// answer that decides whether raw input keeps arriving while an elevated window
/// has focus. So the two fields below are the whole contract: the name is the
/// platform's own, and the boolean is that platform's own question.
#[test]
fn the_report_names_the_platform_input_capability_and_whether_we_have_it() {
    for (name, available) in [("administrator", false), ("input_monitoring", true)] {
        let mut snapshot = crate::tests::snapshot(1, true, true);
        snapshot.input_diagnostics.input_capability =
            crate::SettingsInputCapability { name, available };
        let parsed = report_of(&snapshot);
        assert_eq!(parsed["input_capability"], name);
        assert_eq!(parsed["input_capability_available"], available);
    }
}

/// A report that says the platform has no permission concept is worse than one
/// that says nothing about it, because a maintainer reads it as a fact about the
/// build. The shape has to keep making room for the platform's own answer.
#[test]
fn the_report_never_claims_a_platform_has_no_input_permission_concept() {
    let mut snapshot = crate::tests::snapshot(1, true, true);
    snapshot.input_diagnostics.input_capability = crate::SettingsInputCapability {
        name: "administrator",
        available: false,
    };
    let report = crate::window::about::SoftwareInformation::read(&snapshot, window_facts())
        .to_json()
        .expect("serialize the report");
    for retired in [
        "unsupported",
        "granted",
        "denied",
        "input_monitoring_permission",
    ] {
        assert!(
            !report.contains(retired),
            "the report still carries the macOS-only permission vocabulary: {retired}"
        );
    }
}

/// A layout report cannot be reproduced from a version and an OS build alone:
/// the same build is correct at one scale and clipped at another. So the report
/// carries the scale factor and the logical size, and the two have to describe
/// the same window.
#[test]
fn the_report_carries_the_window_scale_and_size() {
    let parsed = report_of(&crate::tests::snapshot(1, true, true));
    assert_eq!(parsed["ui_scale_factor"], 1.5);
    assert_eq!(parsed["ui_window_size"], serde_json::json!([800, 600]));
    // The layout floor in the acceptance criteria is 800x600, so a report from a
    // window at or below it is the one worth reproducing.
    let size = parsed["ui_window_size"].as_array().expect("a size pair");
    assert!(size[0].as_u64().expect("a width") <= 800);
    assert!(size[1].as_u64().expect("a height") <= 600);
}

/// "My model is not showing up" is answered by a count, and a preset is never
/// part of it: the presets ship with the product, so anything missing is
/// something the user added.
#[test]
fn the_model_facts_count_imported_models_without_naming_them() {
    let mut snapshot = crate::tests::snapshot(1, true, true);
    let parsed = report_of(&snapshot);
    assert_eq!(
        parsed["active_model_origin"], "preset",
        "the fixture's active model is a built-in one"
    );

    let entry = |origin: SettingsModelOrigin, ready: bool| {
        let mut entry = model_entry(
            "private-model-name",
            origin,
            if ready {
                SettingsModelAvailability::Ready {
                    behaviors: Vec::new(),
                }
            } else {
                SettingsModelAvailability::Invalid {
                    diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
                }
            },
        );
        // A title is the user's own text, so the fixture carries one and the
        // report still must not.
        entry.title = "A private title".to_owned();
        entry
    };
    snapshot.model_catalog.entries = vec![
        entry(SettingsModelOrigin::BuiltIn, true),
        entry(SettingsModelOrigin::Imported, true),
        entry(SettingsModelOrigin::Imported, false),
    ];
    let parsed = report_of(&snapshot);
    assert_eq!(parsed["ready_installed_model_count"], 1);
    assert_eq!(parsed["invalid_model_count"], 1);
    let json = parsed.to_string();
    assert!(
        !json.contains("private-model-name") && !json.contains("A private title"),
        "a model identity leaked into the report: {json}"
    );
}

/// No active model is a real state — the store can be empty while it scans — and
/// it has to read as absent rather than as a model from somewhere.
#[test]
fn an_absent_active_model_is_reported_as_absent() {
    let mut snapshot = crate::tests::snapshot(1, true, true);
    snapshot.active_model = None;
    assert!(report_of(&snapshot)["active_model_origin"].is_null());
}

/// The privacy rule is enforced by what the document chooses to serialize, and
/// this is the test that says so. Everything here is a fact about the build or
/// the process; nothing describes what the user has configured or where their
/// files are.
#[test]
fn the_report_carries_no_path_no_user_data_and_no_framework_name() {
    let json = crate::window::about::SoftwareInformation::read(
        &crate::tests::snapshot(1, true, true),
        window_facts(),
    )
    .to_json()
    .expect("serialize the report");
    assert!(!json.contains('/'), "a path leaked into the report");
    assert!(!json.contains('\\'), "a path leaked into the report");
    assert!(!json.to_lowercase().contains("path"));
    assert!(!json.contains('~'), "a home directory reference leaked");
    assert!(!json.contains("Users/"), "a home directory leaked");
    // The fixture's active model is called `standard`, and a model package is
    // identified by its file names. Either appearing here would mean the report
    // had started describing what the user has instead of what was built.
    for owned in ["standard", ".moc3", ".physics3d", ".model3.json", "moc3"] {
        assert!(
            !json.contains(owned),
            "a model name or asset extension leaked into the report: {owned}"
        );
    }
    // The report is a JSON document, so a translated label cannot appear in it:
    // the same fields are readable whatever language the window is in.
    for localized in ["Version ", "版本", "平台", "Architektur", "Plattform"] {
        assert!(
            !json.contains(localized),
            "a translated label leaked into the report: {localized}"
        );
    }
}

#[test]
fn shortcut_presentations_follow_the_resolved_language() {
    let command = ShortcutCaptureTarget::Command("toggle_overlay".to_owned());
    assert_eq!(
        shortcut_command_name(SettingsLanguage::ChineseSimplified, "toggle_overlay"),
        "显示或隐藏模型窗口"
    );
    assert_eq!(
        ShortcutRow {
            target: command,
            behavior: None,
            playable: None,
            shortcut: None,
            custom_name: None,
        }
        .name(SettingsLanguage::English),
        "Show or hide the model window"
    );
    for (command, chinese, english) in [
        (
            "toggle_ignore_mouse_input",
            "切换忽略鼠标输入",
            "Toggle ignoring mouse input",
        ),
        (
            "toggle_ignore_keyboard_input",
            "切换忽略键盘输入",
            "Toggle ignoring keyboard input",
        ),
        (
            "toggle_ignore_gamepad_input",
            "切换忽略手柄输入",
            "Toggle ignoring gamepad input",
        ),
    ] {
        assert_eq!(
            shortcut_command_name(SettingsLanguage::ChineseSimplified, command),
            chinese
        );
        assert_eq!(
            shortcut_command_name(SettingsLanguage::English, command),
            english
        );
    }
}

/// The page's two scopes are groups, so each one's rows have to be a half of the
/// one combined list the keyboard tab order is
/// numbered from.
#[test]
fn shortcut_scopes_split_the_combined_row_order_into_two_halves() {
    let active = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::BuiltIn,
    };
    let entries = vec![model_entry(
        "standard",
        SettingsModelOrigin::BuiltIn,
        SettingsModelAvailability::Ready {
            behaviors: vec![SettingsModelBehavior::Motion {
                group: "CAT_motion".to_owned(),
                index: 0,
            }],
        },
    )];
    let shortcuts = SettingsShortcuts::default();

    let window_rows = ShortcutScope::Window.rows(&shortcuts, Some(&active), &entries, &[]);
    let model_rows = ShortcutScope::Model.rows(&shortcuts, Some(&active), &entries, &[]);
    let combined = shortcut_rows(&shortcuts, Some(&active), &entries, &[]);

    assert_eq!(ShortcutScope::Window.row_index_offset(&shortcuts), 0);
    assert_eq!(
        ShortcutScope::Model.row_index_offset(&shortcuts),
        window_rows.len()
    );
    assert_eq!(combined.len(), window_rows.len() + model_rows.len());
    assert!(
        window_rows
            .iter()
            .all(|row| matches!(row.target, ShortcutCaptureTarget::Command(_)))
    );
    assert!(
        model_rows
            .iter()
            .all(|row| matches!(row.target, ShortcutCaptureTarget::ModelBehavior { .. }))
    );
    for (rendered, combined) in window_rows
        .iter()
        .chain(model_rows.iter())
        .zip(combined.iter())
    {
        assert_eq!(rendered.target, combined.target);
        assert_eq!(rendered.shortcut, combined.shortcut);
    }
}

/// Two scopes sharing a title would collapse into one sidebar entry and hide the
/// other scope, and a scope whose empty state has no message would render a
/// blank body.
#[test]
fn shortcut_scope_titles_are_distinct_and_only_the_model_scope_has_an_empty_state() {
    for language in SettingsLanguage::ALL {
        let window_title = ShortcutScope::Window.title(language);
        let model_title = ShortcutScope::Model.title(language);
        assert!(!window_title.is_empty());
        assert!(!model_title.is_empty());
        assert_ne!(window_title, model_title);
        assert!(ShortcutScope::Window.empty_message(language).is_none());
        assert!(ShortcutScope::Model.empty_message(language).is_some());
    }
}

/// Every scope names its own gate. A missing label would draw a nameless switch
/// above the rows, and a shared label would read as the same setting twice — on
/// a page whose whole point is that each scope owns its own switch.
#[test]
fn shortcut_scope_gates_have_their_own_localized_label() {
    for language in SettingsLanguage::ALL {
        let window_label = ShortcutScope::Window.gate_label(language);
        let model_label = ShortcutScope::Model.gate_label(language);
        assert!(!window_label.is_empty());
        assert!(!model_label.is_empty());
        assert_ne!(window_label, model_label);
    }
}

#[test]
fn model_behavior_shortcut_copy_stays_aligned_between_scope_and_gate() {
    for (language, scope_title, gate_title) in [
        (
            SettingsLanguage::English,
            "Model behavior shortcuts",
            "Enable model behavior shortcuts",
        ),
        (
            SettingsLanguage::ChineseSimplified,
            "模型行为快捷键",
            "启用模型行为快捷键",
        ),
    ] {
        assert_eq!(ShortcutScope::Model.title(language), scope_title);
        assert_eq!(ShortcutScope::Model.gate_label(language), gate_title);
    }
}

/// The login item row names one concept, so the switch and every state that
/// explains why it cannot act have to spell it the same way.
///
/// The switch used to be called "登录时启动" while the three unavailable states
/// said the same words again, and a reader who has only ever seen the Windows
/// task manager has to work out that this is what "start with Windows" is
/// called here. Each catalog entry is pinned so the wording cannot drift back
/// into two vocabularies for the same setting.
#[test]
fn the_login_item_row_names_one_concept_in_its_switch_and_its_states() {
    for (locale, label, platform, operating_system, build) in [
        (
            "en-US",
            "Run at startup",
            "Running at startup is unavailable on this system",
            "Running at startup is unavailable on this operating system",
            "Running at startup is unavailable in development builds",
        ),
        (
            "zh-CN",
            "开机自启动",
            "当前系统不支持开机自启动",
            "当前操作系统不支持开机自启动",
            "开发版本不支持开机自启动",
        ),
    ] {
        assert_eq!(
            bongocat_i18n::text(locale, "settings.app_system.open_at_login.label"),
            label
        );
        for (key, expected) in [
            ("unsupported_platform", platform),
            ("unsupported_os", operating_system),
            ("unsupported_build", build),
        ] {
            assert_eq!(
                bongocat_i18n::text(locale, &format!("settings.app_system.startup.{key}")),
                expected,
                "{locale}: {key} names the setting differently from its switch"
            );
        }
    }
}

#[test]
fn model_window_visibility_copy_uses_the_shared_hide_label() {
    for (language, label) in [
        (SettingsLanguage::English, "Hide model window"),
        (SettingsLanguage::ChineseSimplified, "隐藏模型窗口"),
    ] {
        assert_eq!(
            bongocat_i18n::text(
                language.catalog_locale(),
                "settings.overlay.hide_model_window.label"
            ),
            label
        );
    }
}

#[test]
fn hover_hide_copy_uses_the_same_mouse_hover_subject() {
    for (language, switch_label, delay_label) in [
        (
            SettingsLanguage::English,
            "Hide on mouse hover",
            "Mouse hover hide delay (seconds)",
        ),
        (
            SettingsLanguage::ChineseSimplified,
            "鼠标悬停时隐藏",
            "鼠标悬停时隐藏延迟（秒）",
        ),
    ] {
        let locale = language.catalog_locale();
        assert_eq!(
            bongocat_i18n::text(locale, "settings.overlay.hide_on_mouse_hover.label"),
            switch_label
        );
        assert_eq!(
            bongocat_i18n::text(locale, "settings.overlay.hide_on_mouse_hover_delay.label"),
            delay_label
        );
    }
}

/// The idle hide copy names the inactivity rather than a device.
///
/// Both rows control the same presentation, but they read from different
/// conditions: the hover hide reacts to the pointer, the idle hide to the
/// absence of any input. Calling it a mouse or keyboard hide would make the
/// delay row look like it belongs to the row above it.
#[test]
fn idle_hide_copy_names_the_inactivity_instead_of_a_device() {
    for (locale, switch_label, delay_label) in [
        ("en-US", "Hide when idle", "Idle hide delay (seconds)"),
        ("zh-CN", "无操作时隐藏", "无操作时隐藏延迟（秒）"),
    ] {
        assert_eq!(
            bongocat_i18n::text(locale, "settings.overlay.hide_on_idle.label"),
            switch_label
        );
        assert_eq!(
            bongocat_i18n::text(locale, "settings.overlay.hide_on_idle_delay.label"),
            delay_label
        );
    }
}

#[test]
fn model_window_performance_title_names_the_window() {
    assert_eq!(
        bongocat_i18n::text("zh-CN", "settings.overlay.performance.title"),
        "窗口性能"
    );
    assert_eq!(
        bongocat_i18n::text("en-US", "settings.overlay.performance.title"),
        "Window performance"
    );
}

/// A shortcut target maps to exactly the scope whose switch gates its row:
/// application commands to the window gate, model behaviors to the model
/// gate. The render layer and the mutating methods
/// all route through this mapping, so a drift here would make a disabled row
/// accept edits through one of the other layers.
#[test]
fn shortcut_targets_map_to_the_scope_that_gates_them() {
    let command = ShortcutCaptureTarget::Command("toggle_overlay".to_owned());
    let behavior = ShortcutCaptureTarget::ModelBehavior {
        model: settings_model_key("model", SettingsModelOrigin::BuiltIn),
        behavior_id: "motion:group:index".to_owned(),
    };
    assert!(matches!(
        ShortcutScope::for_target(&command),
        ShortcutScope::Window
    ));
    assert!(matches!(
        ShortcutScope::for_target(&behavior),
        ShortcutScope::Model
    ));
}

#[test]
fn invalid_model_status_is_stable_and_path_free() {
    let entry = model_entry(
        "private-model",
        SettingsModelOrigin::Imported,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelReferenceSymlinkEscape,
        },
    );
    let status = model_availability_status(&entry, SettingsLanguage::English);
    assert_eq!(
        status.as_ref().map(|status| status.as_ref()),
        Some("Imported · Package layout is invalid")
    );
    let status = status.as_ref().map(|status| status.as_ref()).unwrap_or("");
    assert!(!status.contains("private-model"));
    assert!(!status.contains('/'));
}

#[test]
fn model_presentations_follow_the_resolved_language() {
    let ready = model_entry(
        "preset-model",
        SettingsModelOrigin::BuiltIn,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    );
    // A ready model card shows no status line at all: the counts summary was
    // removed, so there is nothing left to localize for it.
    assert!(model_availability_status(&ready, SettingsLanguage::ChineseSimplified).is_none());

    let invalid = model_entry(
        "installed-model",
        SettingsModelOrigin::Imported,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    );
    let invalid_status = model_availability_status(&invalid, SettingsLanguage::ChineseSimplified)
        .expect("invalid models keep a diagnostic status");
    assert_eq!(invalid_status, "已导入 · 模型纹理无效");

    // The import card shows the step it is on rather than the ones it has
    // finished, and the step follows the resolved language too. Idle is the
    // upload prompt, which is rendered from the catalog copy rather than from a
    // step.
    assert_eq!(
        super::models::import_card_step(
            &ModelImportDraft::default(),
            SettingsLanguage::ChineseSimplified,
        ),
        None,
        "an idle draft renders the prompt, not a step"
    );

    // Both running phases come back on their own, and the capture replaces the
    // import line rather than being appended under it: the card reports one step
    // at a time.
    let importing = ModelImportDraft {
        state: ModelImportState::Starting {
            cancel_requested: false,
        },
        ..ModelImportDraft::default()
    };
    assert_eq!(
        super::models::import_card_step(&importing, SettingsLanguage::ChineseSimplified).as_deref(),
        Some("正在导入模型…")
    );

    let capturing = ModelImportDraft {
        state: ModelImportState::Capturing,
        ..ModelImportDraft::default()
    };
    assert_eq!(
        super::models::import_card_step(&capturing, SettingsLanguage::ChineseSimplified).as_deref(),
        Some("正在截取模型封面…")
    );
}

#[test]
fn model_row_action_tab_order_matches_visual_order() {
    // The delete confirmation is a surface anchored to the delete control, not
    // a replacement for the row, so the four positions never move: a card that
    // opened a confirmation would otherwise renumber the controls the user is
    // tabbing past.
    assert_eq!(
        model_row_action_tab_indices(40),
        ModelRowActionTabIndices {
            activate: 40,
            open_location: 41,
            edit: 42,
            delete: 43,
        }
    );
    // Each card owns a stride of five, so the next card's actions start clear of
    // this one's even though only four are used.
    assert_eq!(
        model_row_action_tab_indices(45),
        ModelRowActionTabIndices {
            activate: 45,
            open_location: 46,
            edit: 47,
            delete: 48,
        }
    );
}
