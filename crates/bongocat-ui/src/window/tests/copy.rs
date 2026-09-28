//! The localized copy each page and row shows.

use super::*;

#[test]
fn build_information_is_localized_and_contains_only_compiled_identity() {
    let product_version = env!("CARGO_PKG_VERSION");
    let build_info = crate::SettingsBuildInfo {
        product_version: product_version.to_owned(),
        environment: crate::SettingsBuildEnvironment::Development,
    };
    let detail = build_info_detail(SettingsLanguage::EnglishUnitedStates, &build_info);
    assert_eq!(
        detail,
        format!("Version {product_version} · Development build")
    );
    assert!(!detail.contains('/'));
    assert!(!detail.contains("path"));

    let chinese = build_info_detail(SettingsLanguage::ChineseSimplified, &build_info);
    assert_eq!(chinese, format!("版本 {product_version} · 开发版"));
}

#[test]
fn software_information_is_useful_for_bug_reports_without_paths_or_user_data() {
    let build_info = crate::SettingsBuildInfo {
        product_version: env!("CARGO_PKG_VERSION").to_owned(),
        environment: crate::SettingsBuildEnvironment::Production,
    };
    let text = about::software_info_text(SettingsLanguage::EnglishUnitedStates, &build_info);
    assert!(text.starts_with("BongoCat\n"));
    assert!(text.contains(&format!("Version {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("Release build"));
    assert!(text.contains(&format!("Platform: {}", std::env::consts::OS)));
    assert!(text.contains(&format!("Architecture: {}", std::env::consts::ARCH)));
    assert!(!text.contains("Tauri"));
    assert!(!text.contains('/'));
    assert!(!text.to_lowercase().contains("path"));
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
        }
        .name(SettingsLanguage::EnglishUnitedStates),
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
            shortcut_command_name(SettingsLanguage::EnglishUnitedStates, command),
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

    let window_rows = ShortcutScope::Window.rows(&shortcuts, Some(&active), &entries);
    let model_rows = ShortcutScope::Model.rows(&shortcuts, Some(&active), &entries);
    let combined = shortcut_rows(&shortcuts, Some(&active), &entries);

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
            SettingsLanguage::EnglishUnitedStates,
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

#[test]
fn model_window_visibility_copy_uses_the_shared_hide_label() {
    for (language, label) in [
        (SettingsLanguage::EnglishUnitedStates, "Hide model window"),
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
            SettingsLanguage::EnglishUnitedStates,
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
    let status = model_availability_status(&entry, SettingsLanguage::EnglishUnitedStates);
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
