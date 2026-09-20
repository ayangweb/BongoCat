use super::*;
use crate::{
    SettingsModelBehaviorBinding, SettingsModelCatalog, SettingsModelCatalogError,
    SettingsShortcutBinding,
};
use gpui_kit::{Keystroke, Modifiers};

/// A catalog entry for tests that do not care where the model lives.
///
/// Only the settings page reads `directory` and `cover`; every other assertion
/// in this module is about identity, availability or actions, so those two stay
/// unset unless the test is specifically about opening a location or showing a
/// cover.
fn model_entry(
    id: &str,
    origin: SettingsModelOrigin,
    availability: SettingsModelAvailability,
) -> SettingsModelEntry {
    SettingsModelEntry {
        id: id.to_owned(),
        title: "untitled".to_owned(),
        origin,
        availability,
        directory: None,
        cover: None,
    }
}

#[test]
fn shutdown_flush_chains_each_patch_from_the_latest_confirmed_revision() {
    let mut current_revision = Some(7);
    let first_response_revision = 8;
    assert!(accepts_snapshot_revision(
        current_revision,
        first_response_revision
    ));
    current_revision = Some(first_response_revision);

    let second_expected_revision = current_revision.expect("first patch must confirm");
    assert_eq!(second_expected_revision, 8);
    assert!(accepts_snapshot_revision(current_revision, 9));
}

fn key(key: &str, key_char: Option<&str>) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Modifiers::default(),
            key: key.to_owned(),
            key_char: key_char.map(str::to_owned),
        },
        is_held: false,
        prefer_character_input: false,
    }
}

fn captured_shortcut(key: &str, modifiers: Modifiers) -> Option<String> {
    let keys = capture_key(key).into_iter().collect();
    shortcut_from_capture(&modifiers, &keys)
}

#[test]
fn shortcut_capture_canonicalizes_modifiers_and_named_keys() {
    let mut modifiers = Modifiers {
        control: true,
        shift: true,
        ..Modifiers::default()
    };
    assert_eq!(
        captured_shortcut("arrowleft", modifiers).as_deref(),
        Some("Control+Shift+ArrowLeft")
    );

    modifiers = Modifiers::default();
    modifiers.control = true;
    assert_eq!(
        captured_shortcut("return", modifiers).as_deref(),
        Some("Control+Enter")
    );

    assert_eq!(
        captured_shortcut("f12", Modifiers::default()).as_deref(),
        Some("F12")
    );
}

#[test]
fn shortcut_capture_rejects_unmodified_non_function_and_unsupported_keys() {
    for key_name in ["a", "1", "return", "arrowleft", "space", "delete"] {
        assert!(
            captured_shortcut(key_name, Modifiers::default()).is_none(),
            "{key_name} must not be captured without a modifier"
        );
    }
    assert!(captured_shortcut("shift", Modifiers::default()).is_none());
    assert!(captured_shortcut("media-play", Modifiers::default()).is_none());
}

#[test]
fn shortcut_capture_clears_temporary_input_after_a_conflict() {
    let mut capture =
        ShortcutCapture::new(ShortcutCaptureTarget::Command("toggle_overlay".to_owned()));
    capture.modifiers.platform = true;
    capture.keys.insert("L".to_owned());

    capture.clear_temporary_input();

    assert_eq!(capture.modifiers, Modifiers::default());
    assert!(capture.keys.is_empty());
}

#[test]
fn shortcut_capture_previews_incomplete_and_unsupported_combinations() {
    let keys = BTreeSet::from(["A".to_owned()]);
    let mut modifiers = Modifiers::default();
    assert_eq!(
        shortcut_capture_preview(&modifiers, &keys).as_deref(),
        Some("A")
    );
    assert!(shortcut_from_capture(&modifiers, &keys).is_none());

    modifiers.control = true;
    assert_eq!(
        shortcut_from_capture(&modifiers, &keys).as_deref(),
        Some("Control+A")
    );

    let keys = BTreeSet::new();
    assert_eq!(
        shortcut_capture_preview(&modifiers, &keys).as_deref(),
        Some("Control")
    );
    assert!(shortcut_from_capture(&modifiers, &keys).is_none());
}

#[test]
fn macos_shortcut_display_uses_legacy_symbols() {
    for (shortcut, expected) in [
        ("Control+Alt+Shift+Meta+P", "⌃ ⌥ ⇧ ⌘ P"),
        ("Escape", "⎋"),
        ("Backspace", "⌫"),
        ("Tab", "⇥"),
        ("Enter", "↩︎"),
        ("Space", "␣"),
        ("Control+ArrowLeft", "⌃ ←"),
        ("Meta+BracketLeft", "⌘ ["),
    ] {
        assert_eq!(
            format_shortcut_display(shortcut, true),
            expected,
            "{shortcut}"
        );
    }
}

#[test]
fn non_macos_shortcut_display_preserves_canonical_names() {
    let shortcut = "Control+Alt+ArrowLeft";
    assert_eq!(format_shortcut_display(shortcut, false), shortcut);
}

#[test]
fn shortcut_display_uses_the_compiled_platform() {
    let expected = if cfg!(target_os = "macos") {
        "⌘ P"
    } else {
        "Meta+P"
    };
    assert_eq!(shortcut_display("Meta+P"), expected);
}

#[test]
fn shortcut_capture_conflict_preview_is_order_independent() {
    let shortcuts = SettingsShortcuts {
        commands: vec![
            SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "ctrl+b".to_owned(),
            },
            SettingsShortcutBinding {
                command: "toggle_mirror".to_owned(),
                shortcut: "Control+B".to_owned(),
            },
        ],
        model_behaviors: Vec::new(),
    };
    assert_eq!(
        conflicting_shortcut(&shortcuts).as_deref(),
        Some("Control+B")
    );
}

#[test]
fn shortcut_capture_targets_have_independent_tab_stops() {
    let active_model = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };
    let shortcuts = SettingsShortcuts {
        commands: vec![
            SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+B".to_owned(),
            },
            SettingsShortcutBinding {
                command: "open_settings".to_owned(),
                shortcut: "Control+S".to_owned(),
            },
        ],
        model_behaviors: vec![
            SettingsModelBehaviorBinding {
                model_id: "standard".to_owned(),
                behavior_id: "motion:tap:0".to_owned(),
                shortcut: "Control+M".to_owned(),
            },
            SettingsModelBehaviorBinding {
                model_id: "keyboard".to_owned(),
                behavior_id: "expression:ignored".to_owned(),
                shortcut: "Control+I".to_owned(),
            },
        ],
    };
    let entries = vec![
        model_entry(
            "standard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 1,
                motion_count: 1,
                behaviors: vec![
                    SettingsModelBehavior::Motion {
                        group: "tap".to_owned(),
                        index: 0,
                    },
                    SettingsModelBehavior::Expression {
                        name: "happy".to_owned(),
                    },
                ],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 1,
                motion_count: 0,
                behaviors: vec![SettingsModelBehavior::Expression {
                    name: "ignored".to_owned(),
                }],
            },
        ),
    ];
    let targets = shortcut_targets(&shortcuts, Some(&active_model), &entries);
    assert_eq!(targets.len(), 7);
    assert_eq!(shortcut_capture_tab_index(0), 100);
    assert_eq!(shortcut_capture_tab_index(1), 102);
    assert_eq!(shortcut_capture_tab_index(2), 104);
    assert_eq!(shortcut_clear_tab_index(2), 105);
    assert_eq!(targets.into_iter().collect::<BTreeSet<_>>().len(), 7);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let rows = shortcut_accessibility_rows(
            &shortcuts,
            Some(&active_model),
            &entries,
            SettingsLanguage::EnglishUnitedStates,
        );
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0].1, "Capture shortcut for Show or hide model window");
        assert_eq!(
            rows[1].1,
            "Capture shortcut for Show or hide settings window"
        );
        assert_eq!(rows[5].2, shortcut_display("Control+M"));
        assert_eq!(rows[6].2, "Not set");
        assert_eq!(
            shortcut_target_for_accessibility_node(
                &shortcuts,
                Some(&active_model),
                &entries,
                shortcut_accessibility_node_id(5),
            ),
            Some(ShortcutCaptureTarget::ModelBehavior {
                model_id: "standard".to_owned(),
                behavior_id: "motion:tap:0".to_owned(),
            })
        );
        let clear_rows = shortcut_clear_accessibility_rows(
            &shortcuts,
            Some(&active_model),
            &entries,
            SettingsLanguage::EnglishUnitedStates,
        );
        assert_eq!(clear_rows.len(), 3);
        assert_eq!(
            shortcut_clear_target_for_accessibility_node(
                &shortcuts,
                Some(&active_model),
                &entries,
                shortcut_clear_accessibility_node_id(2),
            ),
            Some(ShortcutCaptureTarget::ModelBehavior {
                model_id: "standard".to_owned(),
                behavior_id: "motion:tap:0".to_owned(),
            })
        );
    }
}

#[test]
fn window_shortcuts_are_visible_and_recordable_without_saved_bindings() {
    let mut shortcuts = SettingsShortcuts::default();
    let rows = window_shortcut_rows(&shortcuts);
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| row.shortcut.is_none()));

    let target = ShortcutCaptureTarget::Command("open_settings".to_owned());
    assert!(replace_shortcut(
        &mut shortcuts,
        &target,
        "Control+Shift+S".to_owned(),
    ));
    assert_eq!(shortcuts.commands.len(), 1);
    assert_eq!(shortcuts.commands[0].command, "open_settings");
    assert_eq!(
        window_shortcut_rows(&shortcuts)[1].shortcut.as_deref(),
        Some("Control+Shift+S")
    );
}

#[test]
fn captured_shortcut_updates_stable_identity_after_reordering() {
    let target = ShortcutCaptureTarget::Command("toggle_overlay".to_owned());
    let mut shortcuts = SettingsShortcuts {
        commands: vec![
            SettingsShortcutBinding {
                command: "open_settings".to_owned(),
                shortcut: "Control+S".to_owned(),
            },
            SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+B".to_owned(),
            },
        ],
        model_behaviors: Vec::new(),
    };

    assert!(replace_shortcut(
        &mut shortcuts,
        &target,
        "Control+O".to_owned()
    ));
    assert_eq!(shortcuts.commands[0].shortcut, "Control+S");
    assert_eq!(shortcuts.commands[1].shortcut, "Control+O");
    assert!(!replace_shortcut(
        &mut shortcuts,
        &ShortcutCaptureTarget::Command("missing".to_owned()),
        "Control+X".to_owned()
    ));
}

#[test]
fn behavior_shortcut_capture_creates_and_clear_removes_a_binding() {
    let target = ShortcutCaptureTarget::ModelBehavior {
        model_id: "standard".to_owned(),
        behavior_id: "expression:happy".to_owned(),
    };
    let mut shortcuts = SettingsShortcuts::default();

    assert!(replace_shortcut(
        &mut shortcuts,
        &target,
        "Control+Alt+H".to_owned(),
    ));
    assert_eq!(shortcuts.model_behaviors.len(), 1);
    assert_eq!(shortcuts.model_behaviors[0].shortcut, "Control+Alt+H");
    assert!(clear_shortcut(&mut shortcuts, &target));
    assert!(shortcuts.model_behaviors.is_empty());
    assert!(!clear_shortcut(&mut shortcuts, &target));
}

#[test]
fn command_shortcut_clear_removes_only_the_selected_binding() {
    let mut shortcuts = SettingsShortcuts {
        commands: vec![
            SettingsShortcutBinding {
                command: "toggle_overlay".to_owned(),
                shortcut: "Control+B".to_owned(),
            },
            SettingsShortcutBinding {
                command: "open_settings".to_owned(),
                shortcut: "Control+S".to_owned(),
            },
        ],
        model_behaviors: Vec::new(),
    };
    assert!(clear_shortcut(
        &mut shortcuts,
        &ShortcutCaptureTarget::Command("toggle_overlay".to_owned()),
    ));
    assert_eq!(shortcuts.commands.len(), 1);
    assert_eq!(shortcuts.commands[0].command, "open_settings");
}

#[test]
fn build_information_is_localized_and_contains_only_compiled_identity() {
    let product_version = env!("CARGO_PKG_VERSION");
    let build_info = crate::SettingsBuildInfo {
        product_version: product_version.to_owned(),
        environment: crate::SettingsBuildEnvironment::Development,
    };
    let detail = build_info_detail(SettingsLanguage::EnglishUnitedStates, &build_info);
    assert_eq!(detail, format!("Version {product_version} · Development"));
    assert!(!detail.contains('/'));
    assert!(!detail.contains("path"));

    let chinese = build_info_detail(SettingsLanguage::ChineseSimplified, &build_info);
    assert_eq!(chinese, format!("版本 {product_version} · 开发环境"));
}

#[test]
fn recovery_and_shortcut_presentations_follow_the_resolved_language() {
    let recovery = config_recovery_presentation(
        SettingsConfigurationStatus::RecoveryRequired { checked_backups: 2 },
        None,
        SettingsLanguage::ChineseSimplified,
    );
    assert_eq!(recovery.title, "配置不可用");
    assert_eq!(recovery.detail, "已检查 2 个备份候选");

    let command = ShortcutCaptureTarget::Command("toggle_overlay".to_owned());
    assert_eq!(
        shortcut_target_name(SettingsLanguage::ChineseSimplified, &command),
        "显示或隐藏模型窗口"
    );
    assert_eq!(
        shortcut_accessibility_label(SettingsLanguage::ChineseSimplified, &command),
        "为显示或隐藏模型窗口录入快捷键"
    );
}

#[test]
fn configuration_recovery_presentation_is_anonymous_and_complete() {
    let normal = config_recovery_presentation(
        SettingsConfigurationStatus::Ready,
        None,
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(normal.title, "Loaded normally");
    assert_eq!(normal.detail, "No recovery");
    assert!(!normal.can_restore);

    let recovered = config_recovery_presentation(
        SettingsConfigurationStatus::Ready,
        Some(SettingsConfigRecovery {
            source_schema_version: 1,
            skipped_newer_backups: 3,
        }),
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(recovered.title, "Recovered from backup");
    assert_eq!(recovered.detail, "Schema v1 · 3 newer backups skipped");
    assert!(!recovered.detail.contains('/') && !recovered.detail.contains('\\'));

    let one_skipped = config_recovery_presentation(
        SettingsConfigurationStatus::Ready,
        Some(SettingsConfigRecovery {
            source_schema_version: 1,
            skipped_newer_backups: 1,
        }),
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(one_skipped.detail, "Schema v1 · 1 newer backup skipped");

    let required = config_recovery_presentation(
        SettingsConfigurationStatus::RecoveryRequired { checked_backups: 2 },
        None,
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(required.title, "Configuration unavailable");
    assert_eq!(required.detail, "2 backup candidates checked");
    assert!(required.attention);
    assert!(required.can_restore);

    let restored = config_recovery_presentation(
        SettingsConfigurationStatus::DefaultsRestoredRestartRequired,
        None,
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(restored.title, "Defaults restored");
    assert_eq!(restored.detail, "Restart the application to continue");
    assert!(!restored.can_restore);
}

#[test]
fn model_import_suggestions_show_the_source_folder_name() {
    assert_eq!(
        suggested_model_title(Path::new("/private/source/Keyboard Model 2")),
        "Keyboard Model 2"
    );
    assert_eq!(
        suggested_model_title(Path::new("/private/source/送葬人 · 标准模式")),
        "送葬人 · 标准模式"
    );
    assert_eq!(suggested_model_title(Path::new("/")), "custom-model");
    assert_eq!(suggested_model_title(Path::new("/src/name ")), "name");
}

#[test]
fn model_title_input_is_free_form_bounded_text() {
    assert_eq!(
        sanitize_model_title_input("  送葬人 · 标准模式 "),
        "送葬人 · 标准模式"
    );
    assert_eq!(sanitize_model_title_input("a\tb"), "ab");
    assert_eq!(sanitize_model_title_input("a\nb"), "ab");
    assert_eq!(
        sanitize_model_title_input(&"x".repeat(200)).chars().count(),
        128
    );
}

#[test]
fn commands_accept_enter_and_space_without_command_modifiers() {
    assert!(is_activation_key(&key("enter", None)));
    assert!(is_activation_key(&key("space", Some(" "))));
    assert!(!is_activation_key(&key("a", Some("a"))));
    let mut modified = key("enter", None);
    modified.keystroke.modifiers.platform = true;
    assert!(!is_activation_key(&modified));
}

#[test]
fn appearance_theme_selection_has_stable_indices_and_system_projection() {
    assert_eq!(
        theme_options(SettingsLanguage::EnglishUnitedStates),
        ["System", "Light", "Dark"]
    );
    assert_eq!(
        theme_options(SettingsLanguage::ChineseSimplified),
        ["跟随系统", "浅色", "深色"]
    );
    assert_eq!(
        theme_from_display_name("深色", SettingsLanguage::ChineseSimplified),
        Some(SettingsTheme::Dark)
    );
    assert_eq!(
        theme_from_display_name("Unknown", SettingsLanguage::EnglishUnitedStates),
        None
    );
    assert_eq!(theme_index(SettingsTheme::System), 0);
    assert_eq!(theme_index(SettingsTheme::Light), 1);
    assert_eq!(theme_index(SettingsTheme::Dark), 2);
    assert_eq!(theme_from_index(0), Some(SettingsTheme::System));
    assert_eq!(theme_from_index(1), Some(SettingsTheme::Light));
    assert_eq!(theme_from_index(2), Some(SettingsTheme::Dark));
    assert_eq!(theme_from_index(3), None);
    assert_eq!(
        component_theme_mode(SettingsTheme::System, WindowAppearance::Light),
        ThemeMode::Light
    );
    assert_eq!(
        component_theme_mode(SettingsTheme::System, WindowAppearance::Dark),
        ThemeMode::Dark
    );
    assert_eq!(
        component_theme_mode(SettingsTheme::Light, WindowAppearance::Dark),
        ThemeMode::Light
    );
    assert_eq!(
        component_theme_mode(SettingsTheme::Dark, WindowAppearance::Light),
        ThemeMode::Dark
    );
}

/// A preference must resolve to the same mode no matter what the operating system is
/// doing, and only the "follow the system" choice may depend on it.
///
/// This is the invariant that keeps the two halves of the appearance together: if a
/// pinned preference ever delegated to the system for the component colours while the
/// native half stayed pinned, the product would paint itself in one theme inside a window
/// frame drawn in another. It is checked against every appearance the platform can
/// report, not just light and dark.
#[test]
fn only_the_system_choice_lets_the_system_decide() {
    let appearances = [
        WindowAppearance::Light,
        WindowAppearance::Dark,
        WindowAppearance::VibrantLight,
        WindowAppearance::VibrantDark,
    ];
    for theme in [
        SettingsTheme::System,
        SettingsTheme::Light,
        SettingsTheme::Dark,
    ] {
        let pinned = pinned_theme_mode(theme);
        for appearance in appearances {
            let resolved = component_theme_mode(theme, appearance);
            match pinned {
                Some(pinned) => assert_eq!(
                    resolved, pinned,
                    "{theme:?} pinned the component half but resolved to {resolved:?} for {appearance:?}"
                ),
                None => assert_eq!(
                    resolved,
                    ThemeMode::from(appearance),
                    "{theme:?} is not pinned, so it must follow {appearance:?}"
                ),
            }
        }
    }
}

/// The native half pins exactly when the component half does.
///
/// `pinned_theme_mode` and `pinned_native_theme` are two spellings of one decision, so a
/// change to either that forgets the other would silently split the appearance.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn the_native_and_component_halves_pin_together() {
    for theme in [
        SettingsTheme::System,
        SettingsTheme::Light,
        SettingsTheme::Dark,
    ] {
        assert_eq!(
            pinned_native_theme(theme).map(bongocat_platform::AppTheme::is_dark),
            pinned_theme_mode(theme).map(|mode| mode == ThemeMode::Dark),
            "{theme:?} pinned one half of the appearance and not the other"
        );
    }
}

#[test]
fn overlay_stepper_values_are_bounded_and_preserve_other_settings() {
    let settings = SettingsOverlay {
        click_through: false,
        always_on_top: false,
        scale_percent: 100,
        opacity_percent: 50,
        corner_radius_percent: 25,
        hide_on_pointer_hover: true,
        hide_on_pointer_hover_delay_seconds: 2,
        keep_inside_screen: false,
    };
    assert_eq!(stepped_overlay_scale(settings, -25).scale_percent, 75);
    assert_eq!(stepped_overlay_scale(settings, 25).scale_percent, 125);
    assert_eq!(stepped_overlay_scale(settings, -500).scale_percent, 25);
    assert_eq!(stepped_overlay_scale(settings, 500).scale_percent, 400);
    assert_eq!(stepped_overlay_opacity(settings, -10).opacity_percent, 40);
    assert_eq!(stepped_overlay_opacity(settings, 10).opacity_percent, 60);
    assert_eq!(stepped_overlay_opacity(settings, -500).opacity_percent, 1);
    assert_eq!(stepped_overlay_opacity(settings, 500).opacity_percent, 100);
    let changed = stepped_overlay_scale(settings, 25);
    assert!(!changed.click_through);
    assert!(!changed.always_on_top);
    assert_eq!(changed.opacity_percent, 50);
    assert_eq!(changed.corner_radius_percent, 25);
    assert!(changed.hide_on_pointer_hover);
    assert_eq!(changed.hide_on_pointer_hover_delay_seconds, 2);
    assert!(!changed.keep_inside_screen);
}

#[test]
fn cancellation_requested_while_starting_reaches_the_created_operation() {
    let (client, _endpoint) = SettingsClient::bounded(1);
    let (operation, _, _) = client.prepare_model_import().expect("prepared import");
    let draft = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Starting {
            cancel_requested: true,
        },
        ..ModelImportDraft::default()
    };

    assert!(!operation.is_cancelled());
    draft.apply_starting_cancellation(&operation);
    assert!(operation.is_cancelled());
    let status = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Cancelling import...");
}

#[test]
fn model_catalog_and_import_statuses_cover_loading_empty_error_and_cancellation() {
    assert_eq!(
        super::models::empty_model_catalog_status(None, SettingsLanguage::EnglishUnitedStates),
        "Loading models..."
    );

    let empty = SettingsModelCatalog::default();
    assert_eq!(
        super::models::empty_model_catalog_status(
            Some(&empty),
            SettingsLanguage::ChineseSimplified,
        ),
        "没有可用模型"
    );

    let mut unavailable = empty;
    unavailable.error = Some(SettingsModelCatalogError::Unavailable);
    assert_eq!(
        super::models::empty_model_catalog_status(
            Some(&unavailable),
            SettingsLanguage::ChineseSimplified,
        ),
        "模型列表不可用"
    );

    let cancelled = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: None,
        state: ModelImportState::Cancelled,
        ..ModelImportDraft::default()
    };
    let status = model_import_status(&cancelled, SettingsLanguage::ChineseSimplified);
    assert_eq!(status, "已取消导入");
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn model_import_accessibility_nodes_project_actions_progress_and_catalog_states() {
    let ready = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Ready,
        ..ModelImportDraft::default()
    };
    let [choose_folder, choose_archive, import, status] =
        super::accessibility::model_import_accessibility_nodes(
            &ready,
            false,
            true,
            SettingsLanguage::EnglishUnitedStates,
        );
    assert_eq!(choose_folder.role, AccessibilityRole::Button);
    assert_eq!(choose_folder.label, "Choose folder");
    assert!(choose_folder.supports_click);
    assert_eq!(choose_archive.role, AccessibilityRole::Button);
    assert_eq!(choose_archive.label, "Choose archive");
    assert!(choose_archive.supports_click);
    assert_eq!(import.label, "Import");
    assert!(!import.disabled);
    assert!(import.supports_click);
    assert_eq!(import.value.as_deref(), Some("Folder selected"));
    assert_eq!(status.role, AccessibilityRole::Status);
    assert_eq!(status.value.as_deref(), Some("Folder selected"));

    let cancelling = ModelImportDraft {
        state: ModelImportState::Starting {
            cancel_requested: true,
        },
        ..ready
    };
    let [choose_folder, choose_archive, import, status] =
        super::accessibility::model_import_accessibility_nodes(
            &cancelling,
            false,
            true,
            SettingsLanguage::EnglishUnitedStates,
        );
    assert!(choose_folder.disabled);
    assert!(choose_archive.disabled);
    assert_eq!(import.label, "Cancel");
    assert!(!import.disabled);
    assert!(import.supports_click);
    assert_eq!(status.value.as_deref(), Some("Cancelling import..."));

    let loading = super::accessibility::model_catalog_accessibility_status_node(
        None,
        SettingsLanguage::EnglishUnitedStates,
    )
    .expect("loading catalog must be announced");
    assert_eq!(loading.role, AccessibilityRole::Status);
    assert_eq!(loading.value.as_deref(), Some("Loading models..."));

    let unavailable = SettingsModelCatalog {
        error: Some(SettingsModelCatalogError::Unavailable),
        ..SettingsModelCatalog::default()
    };
    let unavailable = super::accessibility::model_catalog_accessibility_status_node(
        Some(&unavailable),
        SettingsLanguage::ChineseSimplified,
    )
    .expect("catalog error must be announced");
    assert_eq!(unavailable.value.as_deref(), Some("模型列表不可用"));

    let available = SettingsModelCatalog {
        entries: vec![model_entry(
            "preset",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 0,
                motion_count: 0,
                behaviors: Vec::new(),
            },
        )],
        ..SettingsModelCatalog::default()
    };
    assert!(
        super::accessibility::model_catalog_accessibility_status_node(
            Some(&available),
            SettingsLanguage::EnglishUnitedStates,
        )
        .is_none()
    );
}

#[test]
fn picker_and_import_statuses_never_contain_the_selected_path() {
    let mut draft = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/secret/model")),
        state: ModelImportState::PickerCancelled,
        ..ModelImportDraft::default()
    };
    let status = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Selection cancelled; previous selection retained");
    assert!(!status.contains("private"));

    // A failed dialog is reported by notification, so the inline status keeps
    // describing the selection that is still in effect instead of restating the
    // failure a second time.
    draft.state = ModelImportState::PickerFailed;
    let status = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Folder selected");
    assert!(!status.contains("secret"));

    // An archive source reports the archive's own wording, so the page never
    // claims a folder was chosen when the user chose a `.zip`.
    draft.source_kind = ModelSourceKind::Archive;
    let status = model_import_status(&draft, SettingsLanguage::ChineseSimplified);
    assert_eq!(status, "已选择压缩包");

    draft.state = ModelImportState::Ready;
    let status = model_import_status(&draft, SettingsLanguage::ChineseSimplified);
    assert_eq!(status, "已选择压缩包");

    // With nothing selected yet the same failure falls back to the neutral
    // "nothing chosen" wording.
    let empty = ModelImportDraft {
        state: ModelImportState::PickerFailed,
        ..ModelImportDraft::default()
    };
    assert_eq!(
        model_import_status(&empty, SettingsLanguage::EnglishUnitedStates),
        "No folder selected"
    );
}

#[test]
fn picker_open_state_blocks_conflicting_import_actions() {
    let draft = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Picking,
        ..ModelImportDraft::default()
    };

    assert!(draft.is_picker_open());
    assert!(!draft.can_import());
    let status = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Choosing folder...");

    let archive = ModelImportDraft {
        source_kind: ModelSourceKind::Archive,
        ..draft
    };
    let status = model_import_status(&archive, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Choosing archive...");
}

#[test]
fn suggested_titles_agree_for_a_folder_and_the_archive_made_from_it() {
    let root = PathBuf::from("/private/我的猫 · 标准模式");
    assert_eq!(suggested_model_title(&root), "我的猫 · 标准模式");
    // An archive suggests the same title as the folder it was compressed from,
    // rather than the exported file name with its extension attached.
    assert_eq!(
        suggested_model_title(&root.with_extension("zip")),
        "我的猫 · 标准模式"
    );
    assert_eq!(suggested_model_title(&PathBuf::from("/")), "custom-model");
}

#[test]
fn model_row_actions_preserve_origin_availability_and_active_identity() {
    let ready = SettingsModelAvailability::Ready {
        texture_count: 1,
        expression_count: 0,
        motion_count: 0,
        behaviors: Vec::new(),
    };
    let preset = model_entry("duplicate", SettingsModelOrigin::Preset, ready.clone());
    let installed = model_entry("duplicate", SettingsModelOrigin::Installed, ready);
    let active_preset = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };

    // A preset is app-bundled content: it can be activated but never deleted or
    // edited, whichever model the user is on.
    assert_eq!(
        model_row_actions(&preset, Some(&active_preset), false),
        ModelRowActions {
            active: true,
            can_activate: false,
            can_delete: false,
            can_edit: false,
            can_open_location: false,
        }
    );
    assert_eq!(
        model_row_actions(&installed, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: true,
            can_delete: true,
            can_edit: true,
            can_open_location: false,
        }
    );

    // "Open location" needs a directory that actually resolved, so an entry
    // whose files are gone offers no button instead of a dead one.
    let located = SettingsModelEntry {
        directory: Some(PathBuf::from("/private/models/duplicate")),
        ..installed.clone()
    };
    assert_eq!(
        model_row_actions(&located, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: true,
            can_delete: true,
            can_edit: true,
            can_open_location: true,
        }
    );

    let invalid = SettingsModelEntry {
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
        },
        ..installed.clone()
    };
    assert_eq!(
        model_row_actions(&invalid, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: false,
            can_delete: true,
            can_edit: true,
            can_open_location: false,
        }
    );
    assert_eq!(
        model_row_actions(&installed, Some(&active_preset), true),
        ModelRowActions {
            active: false,
            can_activate: false,
            can_delete: false,
            can_edit: false,
            can_open_location: false,
        }
    );
    assert!(model_delete_confirmation_is_valid(
        &[preset.clone(), installed.clone()],
        Some(&active_preset),
        &SettingsModelKey {
            id: "duplicate".to_owned(),
            origin: SettingsModelOrigin::Installed,
        },
    ));
    assert!(!model_delete_confirmation_is_valid(
        &[preset, installed],
        Some(&active_preset),
        &active_preset,
    ));
}

#[test]
fn behavior_targets_stay_scoped_to_model_and_behavior_identity() {
    let motion = SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    };
    let expression = SettingsModelBehavior::Expression {
        name: "live2d_expression0.exp3.json".to_owned(),
    };
    let entries = vec![
        model_entry(
            "standard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 1,
                motion_count: 1,
                behaviors: vec![motion.clone(), expression],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 0,
                motion_count: 1,
                behaviors: vec![motion],
            },
        ),
    ];
    // Behavior identity now lives with the shortcut rows the page that owns them
    // renders, so this is where "a motion and an expression are different
    // targets, and the same motion on two models is two targets" has to hold.
    let targets = |id: &str| {
        shortcut_behavior_rows(
            &SettingsShortcuts::default(),
            Some(&SettingsModelKey {
                id: id.to_owned(),
                origin: SettingsModelOrigin::Preset,
            }),
            &entries,
        )
        .into_iter()
        .map(|row| row.target)
        .collect::<Vec<_>>()
    };

    let standard = targets("standard");
    let keyboard = targets("keyboard");
    assert_eq!(standard.len(), 2);
    assert_ne!(standard[0], standard[1]);
    assert_eq!(keyboard.len(), 1);
    assert_ne!(keyboard[0], standard[0]);
}

#[test]
fn active_model_behavior_preview_targets_exclude_inactive_and_invalid_models() {
    let active = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };
    let behavior = SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    };
    let entries = vec![
        model_entry(
            "standard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 0,
                motion_count: 1,
                behaviors: vec![behavior.clone()],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 1,
                motion_count: 0,
                behaviors: vec![SettingsModelBehavior::Expression {
                    name: "inactive".to_owned(),
                }],
            },
        ),
    ];

    // The page no longer previews behaviors: the shortcuts page owns that list,
    // so the targets are only reached through the shortcut rows it renders.
    assert_eq!(
        shortcut_behavior_rows(&SettingsShortcuts::default(), Some(&active), &entries).len(),
        1
    );
    assert!(shortcut_behavior_rows(&SettingsShortcuts::default(), None, &entries).is_empty());
}

#[test]
fn invalid_model_status_is_stable_and_path_free() {
    let entry = model_entry(
        "private-model",
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelReferenceSymlinkEscape,
        },
    );
    let status = model_availability_status(&entry, false, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Installed · Package layout is invalid");
    assert!(!status.contains("private-model"));
    assert!(!status.contains('/'));
}

#[test]
fn model_presentations_follow_the_resolved_language() {
    let ready = model_entry(
        "preset-model",
        SettingsModelOrigin::Preset,
        SettingsModelAvailability::Ready {
            texture_count: 2,
            expression_count: 3,
            motion_count: 4,
            behaviors: Vec::new(),
        },
    );
    assert_eq!(
        model_availability_status(&ready, true, SettingsLanguage::ChineseSimplified),
        "预置 · 当前使用 · 2 个纹理 · 3 个表情 · 4 个动作"
    );

    let invalid = model_entry(
        "installed-model",
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    );
    let invalid_status =
        model_availability_status(&invalid, false, SettingsLanguage::ChineseSimplified);
    assert_eq!(invalid_status, "已安装 · 纹理无效");
    assert_eq!(
        model_delete_confirmation(SettingsLanguage::ChineseSimplified, &invalid_status),
        "已安装 · 纹理无效 · 确认删除"
    );

    let import_status = model_import_status(
        &ModelImportDraft::default(),
        SettingsLanguage::ChineseSimplified,
    );
    assert_eq!(import_status, "尚未选择文件夹");
    assert_eq!(
        model_import_progress(SettingsLanguage::ChineseSimplified, "正在复制", 5, 1024),
        "正在复制 · 5 个文件 · 1024 字节"
    );

    // A failed import reports itself through a notification only, so the tag
    // has nothing left to say rather than a second copy of the same error.
    let failed_import = ModelImportDraft {
        state: ModelImportState::Failed,
        ..ModelImportDraft::default()
    };
    assert_eq!(
        model_import_status(&failed_import, SettingsLanguage::ChineseSimplified),
        ""
    );
}

#[test]
fn model_delete_confirmation_tab_order_matches_visual_order() {
    assert_eq!(
        model_row_action_tab_indices(40, false),
        ModelRowActionTabIndices {
            activate: 40,
            open_location: 41,
            edit: 42,
            delete: 43,
            cancel_delete: 44,
        }
    );
    // Confirming hides the leaving actions, so the two remaining controls close
    // the gap rather than keeping indices for buttons that are not rendered.
    assert_eq!(
        model_row_action_tab_indices(40, true),
        ModelRowActionTabIndices {
            activate: 40,
            open_location: 41,
            edit: 42,
            delete: 41,
            cancel_delete: 42,
        }
    );
}

#[test]
fn startup_item_presentations_cover_every_platform_state_and_retry() {
    let cases = [
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Disabled),
            false,
            StartupItemAction::SetEnabled(true),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Enabled),
            true,
            StartupItemAction::SetEnabled(false),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Stale),
            false,
            StartupItemAction::SetEnabled(true),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::RequiresApproval),
            true,
            StartupItemAction::SetEnabled(false),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::NotFound),
            false,
            StartupItemAction::SetEnabled(true),
        ),
        (
            SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment,
            )),
            false,
            StartupItemAction::None,
        ),
        (
            SettingsStartupItemStatus::ReadError(crate::SettingsStartupItemError::StateReadFailed),
            false,
            StartupItemAction::Retry,
        ),
    ];

    for (status, enabled, action) in cases {
        let presentation =
            startup_item_presentation(Some(status), false, SettingsLanguage::EnglishUnitedStates);
        assert_eq!(presentation.enabled, enabled);
        assert_eq!(presentation.action, action);
        assert!(!presentation.description.is_empty());
        assert_eq!(
            startup_item_presentation(Some(status), true, SettingsLanguage::EnglishUnitedStates,)
                .action,
            StartupItemAction::None
        );
    }
    assert_eq!(
        startup_item_presentation(None, false, SettingsLanguage::EnglishUnitedStates).action,
        StartupItemAction::None
    );
    assert_eq!(
        startup_item_presentation(
            Some(SettingsStartupItemStatus::State(
                SettingsStartupItemState::Enabled
            )),
            false,
            SettingsLanguage::ChineseSimplified,
        )
        .description,
        "应用将在登录系统时启动"
    );
}

/// The five navigation buttons carry a label and nothing else.
///
/// These nodes used to put the page description in `value`, which duplicated
/// the header text that has since been removed from every page. The
/// accessibility tree is only built under the settings-window smoke, which
/// cannot fail the process on macOS (TODO 87), so this test is the only place
/// the shape is actually pinned.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn navigation_accessibility_nodes_carry_no_descriptive_value() {
    for language in [
        SettingsLanguage::EnglishUnitedStates,
        SettingsLanguage::ChineseSimplified,
    ] {
        let nodes = navigation_accessibility_nodes(language);
        let ids: Vec<AccessibilityNodeId> = nodes.iter().map(|node| node.id).collect();
        assert_eq!(
            ids,
            vec![
                ACCESSIBILITY_GENERAL,
                ACCESSIBILITY_MODELS,
                ACCESSIBILITY_SHORTCUTS,
                ACCESSIBILITY_ABOUT,
            ]
        );
        for node in &nodes {
            assert_eq!(node.role, AccessibilityRole::Button);
            assert!(
                node.value.is_none(),
                "navigation node {} must not carry a value",
                node.id.get()
            );
            assert!(node.supports_click);
            assert!(node.supports_focus);
        }
        // The label is the localized page title, never empty and never the
        // same as the English one once a non-default language is selected.
        assert_eq!(
            nodes[0].label,
            bongocat_i18n::text(language.catalog_locale(), "navigation.general.title")
        );
        assert!(!nodes[0].label.is_empty());
    }
    let english = navigation_accessibility_nodes(SettingsLanguage::EnglishUnitedStates);
    let chinese = navigation_accessibility_nodes(SettingsLanguage::ChineseSimplified);
    assert_ne!(english[0].label, chinese[0].label);
}
