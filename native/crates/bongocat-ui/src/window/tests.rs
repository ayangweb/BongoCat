use super::*;
use crate::{
    SettingsDiagnosticsExportStatus, SettingsModelBehaviorBinding, SettingsModelCatalog,
    SettingsModelCatalogError, SettingsShortcutBinding,
};
use gpui_kit::{Keystroke, Modifiers};

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
    let mut modifiers = Modifiers::default();
    modifiers.control = true;
    modifiers.shift = true;
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
        SettingsModelEntry {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::Preset,
            availability: SettingsModelAvailability::Ready {
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
        },
        SettingsModelEntry {
            id: "keyboard".to_owned(),
            origin: SettingsModelOrigin::Preset,
            availability: SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 1,
                motion_count: 0,
                behaviors: vec![SettingsModelBehavior::Expression {
                    name: "ignored".to_owned(),
                }],
            },
        },
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
        assert_eq!(rows[5].2, "Control+M");
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
fn diagnostics_page_projects_only_named_aggregate_counters() {
    let diagnostics = SettingsInputDiagnostics {
        input_monitoring_permission: crate::SettingsInputMonitoringPermission::Granted,
        service_status: SettingsInputServiceStatus::Running,
        service_error_code: None,
        service_start_attempts: 1,
        pressed_key_count: 1,
        pressed_mouse_button_count: 2,
        pressed_gamepad_button_count: 3,
        connected_gamepad_count: 4,
        captured_down: 5,
        captured_up: 6,
        reconciled_release: 7,
        fallback_release: 8,
        released_by_reset: 9,
        duplicate_down: 10,
        unmatched_release: 11,
        invalid_source: 12,
        reset_count: 13,
        sequence_gap_count: 14,
        missing_sequence_count: 15,
        duplicate_sequence_count: 16,
        out_of_order_sequence_count: 17,
        non_monotonic_time_count: 18,
        gamepad_connections: 19,
        gamepad_disconnections: 20,
        stale_gamepad_events: 21,
        released_by_disconnect: 22,
        transport_enqueued: 23,
        transport_queue_full: 24,
        transport_recovered_after_overflow: 25,
        transport_runtime_stopped: 26,
    };
    let metrics = input_diagnostic_metrics(SettingsLanguage::EnglishUnitedStates, diagnostics);
    assert_eq!(metrics.len(), 26);
    assert_eq!(metrics.first(), Some(&("Pressed keys", 1)));
    assert_eq!(metrics.last(), Some(&("Rejected after shutdown", 26)));
    assert_eq!(
        metrics.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        (1..=26).collect::<Vec<_>>()
    );
    assert!(metrics.iter().all(|(label, _)| {
        !label.contains("HID") && !label.contains("path") && !label.contains("timestamp value")
    }));
    let service = input_service_presentation(diagnostics, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(service.title, "Running");
    assert_eq!(
        service.detail,
        "Input Monitoring: Granted\nStart attempts: 1"
    );
    assert!(service.running);
    assert!(!service.attention);
}

#[test]
fn build_information_is_localized_and_contains_only_compiled_identity() {
    let build_info = crate::SettingsBuildInfo {
        product_version: "0.1.0".to_owned(),
        environment: crate::SettingsBuildEnvironment::Development,
    };
    let detail = build_info_detail(SettingsLanguage::EnglishUnitedStates, &build_info);
    assert_eq!(detail, "Version 0.1.0 · Development");
    assert!(!detail.contains('/'));
    assert!(!detail.contains("path"));

    let chinese = build_info_detail(SettingsLanguage::ChineseSimplified, &build_info);
    assert_eq!(chinese, "版本 0.1.0 · 开发环境");
}

#[test]
fn runtime_diagnostics_presentation_keeps_codes_anonymous_and_actionable() {
    let presentation = runtime_diagnostics_presentation(
        SettingsRuntimeDiagnostics {
            render_error: Some(SettingsRuntimeErrorCode::GpuPreparationFailed),
            last_command_failure: Some(crate::SettingsRuntimeCommandFailure {
                sequence: 17,
                code: SettingsRuntimeErrorCode::GpuPreparationFailed,
            }),
            command_transport: Default::default(),
            ..SettingsRuntimeDiagnostics::default()
        },
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(presentation.title, "GPU preparation failed");
    assert!(presentation.attention);
    assert_eq!(presentation.detail, "GPU preparation failed · command #17");
    assert!(!presentation.detail.contains('/'));
}

#[test]
fn diagnostics_presentations_follow_the_resolved_language() {
    let diagnostics = SettingsInputDiagnostics {
        pressed_key_count: 2,
        service_status: SettingsInputServiceStatus::PermissionDenied,
        service_start_attempts: 3,
        ..SettingsInputDiagnostics::default()
    };
    let metrics = input_diagnostic_metrics(SettingsLanguage::ChineseSimplified, diagnostics);
    assert_eq!(metrics.first(), Some(&("按下的按键", 2)));

    let service = input_service_presentation(diagnostics, SettingsLanguage::ChineseSimplified);
    assert_eq!(service.title, "需要权限");
    assert_eq!(service.detail, "输入监控：不支持\n启动尝试：3");

    let runtime = runtime_diagnostics_presentation(
        SettingsRuntimeDiagnostics {
            render_error: Some(SettingsRuntimeErrorCode::ModelLoadFailed),
            last_command_failure: Some(crate::SettingsRuntimeCommandFailure {
                sequence: 4,
                code: SettingsRuntimeErrorCode::TransportClosed,
            }),
            command_transport: Default::default(),
            ..SettingsRuntimeDiagnostics::default()
        },
        SettingsLanguage::ChineseSimplified,
    );
    assert_eq!(runtime.title, "模型加载失败");
    assert_eq!(runtime.detail, "运行时传输已关闭 · 命令 #4");

    let recovery = config_recovery_presentation(
        SettingsConfigurationStatus::RecoveryRequired { checked_backups: 2 },
        None,
        SettingsLanguage::ChineseSimplified,
    );
    assert_eq!(recovery.title, "配置不可用");
    assert_eq!(recovery.detail, "已检查 2 个备份候选");

    assert_eq!(
        diagnostics_export_status(
            SettingsLanguage::ChineseSimplified,
            Some(SettingsDiagnosticsExportStatus {
                format_version: 1,
                bytes_written: 128,
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 256,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 2,
            }),
        ),
        "报告 v1：128 字节 · 预览包 v1：3 个条目，256 字节 · 跳过 2 个来源日志"
    );
    assert_eq!(
        diagnostics_export_status(
            SettingsLanguage::EnglishUnitedStates,
            Some(SettingsDiagnosticsExportStatus {
                format_version: 1,
                bytes_written: 128,
                preview_bundle_format_version: 1,
                preview_bundle_bytes_written: 256,
                preview_bundle_entry_count: 3,
                preview_bundle_skipped_source_files: 2,
            }),
        ),
        "Report v1: 128 bytes · Preview bundle v1: 3 entries, 256 bytes · Skipped 2 source logs"
    );
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
fn runtime_shutdown_failures_are_localized_and_actionable() {
    let presentation = runtime_diagnostics_presentation(
        SettingsRuntimeDiagnostics {
            shutdown_timed_out: 2,
            shutdown_worker_panicked: 1,
            ..SettingsRuntimeDiagnostics::default()
        },
        SettingsLanguage::ChineseSimplified,
    );
    assert!(presentation.attention);
    assert_eq!(presentation.title, "没有渲染器错误");
    assert_eq!(presentation.detail, "没有命令失败 · 退出失败：3");
}

#[test]
fn input_service_status_keeps_permission_failure_actionable_and_anonymous() {
    let service = input_service_presentation(
        SettingsInputDiagnostics {
            input_monitoring_permission: crate::SettingsInputMonitoringPermission::Denied,
            service_status: SettingsInputServiceStatus::PermissionDenied,
            service_start_attempts: 1,
            ..SettingsInputDiagnostics::default()
        },
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(service.title, "Permission required");
    assert_eq!(
        service.detail,
        "Input Monitoring: Permission required\nStart attempts: 1"
    );
    assert!(service.attention);
    assert!(!service.detail.contains("path"));
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
    assert!(!normal.recovered);
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
    assert!(recovered.recovered);
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
fn model_id_suggestions_are_portable_bounded_and_path_free() {
    assert_eq!(
        suggested_model_id(Path::new("/private/source/Keyboard Model 2")),
        "keyboard-model-2"
    );
    assert_eq!(
        suggested_model_id(Path::new("/private/source/模型目录")),
        "custom-model"
    );
    assert_eq!(
        suggested_model_id(Path::new("/private/source/CON.custom")),
        "model-con.custom"
    );
    let reserved = format!("CON.{}", "x".repeat(80));
    let suggestion = suggested_model_id(Path::new(&reserved));
    assert!(suggestion.starts_with("model-con."));
    assert!(suggestion.len() <= 64);
    assert!(!suggestion.ends_with('.'));
}

#[test]
fn model_id_input_accepts_only_the_product_ascii_shape() {
    assert_eq!(sanitize_model_id_input("a-/b_c.d"), "a-b_c.d");
    assert_eq!(sanitize_model_id_input("模型目录"), "");
    assert_eq!(sanitize_model_id_input(&"x".repeat(80)).len(), 64);
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

#[test]
fn overlay_stepper_values_are_bounded_and_preserve_other_settings() {
    let settings = SettingsOverlay {
        click_through: false,
        always_on_top: false,
        scale_percent: 100,
        opacity_percent: 50,
        keep_inside_work_area: false,
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
    assert!(!changed.keep_inside_work_area);
}

#[test]
fn cancellation_requested_while_starting_reaches_the_created_operation() {
    let (client, _endpoint) = SettingsClient::bounded(1);
    let (operation, _, _) = client.prepare_model_import().expect("prepared import");
    let draft = ModelImportDraft {
        id: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Starting {
            cancel_requested: true,
        },
    };

    assert!(!operation.is_cancelled());
    draft.apply_starting_cancellation(&operation);
    assert!(operation.is_cancelled());
    let (status, failed) = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert!(!failed);
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
        id: "custom-model".to_owned(),
        source_root: None,
        state: ModelImportState::Cancelled,
    };
    let (status, failed) = model_import_status(&cancelled, SettingsLanguage::ChineseSimplified);
    assert!(!failed);
    assert_eq!(status, "已取消导入");
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn model_import_accessibility_nodes_project_actions_progress_and_catalog_states() {
    let ready = ModelImportDraft {
        id: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Ready,
    };
    let [choose_folder, import, status] = super::accessibility::model_import_accessibility_nodes(
        &ready,
        false,
        true,
        SettingsLanguage::EnglishUnitedStates,
    );
    assert_eq!(choose_folder.role, AccessibilityRole::Button);
    assert_eq!(choose_folder.label, "Choose folder");
    assert!(choose_folder.supports_click);
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
    let [choose_folder, import, status] = super::accessibility::model_import_accessibility_nodes(
        &cancelling,
        false,
        true,
        SettingsLanguage::EnglishUnitedStates,
    );
    assert!(choose_folder.disabled);
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

    let mut unavailable = SettingsModelCatalog::default();
    unavailable.error = Some(SettingsModelCatalogError::Unavailable);
    let unavailable = super::accessibility::model_catalog_accessibility_status_node(
        Some(&unavailable),
        SettingsLanguage::ChineseSimplified,
    )
    .expect("catalog error must be announced");
    assert_eq!(unavailable.value.as_deref(), Some("模型列表不可用"));

    let available = SettingsModelCatalog {
        entries: vec![SettingsModelEntry {
            id: "preset".to_owned(),
            origin: SettingsModelOrigin::Preset,
            availability: SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 0,
                motion_count: 0,
                behaviors: Vec::new(),
            },
        }],
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
fn picker_status_never_contains_the_selected_path() {
    let mut draft = ModelImportDraft {
        id: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/secret/model")),
        state: ModelImportState::PickerCancelled,
    };
    let (status, failed) = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert!(!failed);
    assert_eq!(status, "Selection cancelled; previous folder retained");
    assert!(!status.contains("private"));

    draft.state = ModelImportState::PickerFailed(DirectoryPickerError::SelectionInvalid);
    let (status, failed) = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert!(failed);
    assert_eq!(status, "Selected folder is unavailable");
    assert!(!status.contains("secret"));
}

#[test]
fn picker_open_state_blocks_conflicting_import_actions() {
    let draft = ModelImportDraft {
        id: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Picking,
    };

    assert!(draft.is_picker_open());
    assert!(!draft.can_import());
    let (status, failed) = model_import_status(&draft, SettingsLanguage::EnglishUnitedStates);
    assert!(!failed);
    assert_eq!(status, "Choosing folder...");
}

#[test]
fn model_row_actions_preserve_origin_availability_and_active_identity() {
    let ready = SettingsModelAvailability::Ready {
        texture_count: 1,
        expression_count: 0,
        motion_count: 0,
        behaviors: Vec::new(),
    };
    let preset = SettingsModelEntry {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Preset,
        availability: ready.clone(),
    };
    let installed = SettingsModelEntry {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Installed,
        availability: ready,
    };
    let active_preset = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };

    assert_eq!(
        model_row_actions(&preset, Some(&active_preset), false),
        ModelRowActions {
            active: true,
            can_activate: false,
            can_delete: false,
        }
    );
    assert_eq!(
        model_row_actions(&installed, Some(&active_preset), false),
        ModelRowActions {
            active: false,
            can_activate: true,
            can_delete: true,
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
        }
    );
    assert_eq!(
        model_row_actions(&installed, Some(&active_preset), true),
        ModelRowActions {
            active: false,
            can_activate: false,
            can_delete: false,
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
fn model_behavior_preview_keys_are_scoped_to_model_and_behavior_identity() {
    let model = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };
    let motion = SettingsModelBehavior::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    };
    let expression = SettingsModelBehavior::Expression {
        name: "live2d_expression0.exp3.json".to_owned(),
    };

    assert_ne!(
        ModelBehaviorKey::new(&model, &motion),
        ModelBehaviorKey::new(&model, &expression)
    );
    assert_ne!(
        ModelBehaviorKey::new(&model, &motion),
        ModelBehaviorKey::new(
            &SettingsModelKey {
                id: "keyboard".to_owned(),
                origin: SettingsModelOrigin::Preset,
            },
            &motion,
        )
    );
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
        SettingsModelEntry {
            id: "standard".to_owned(),
            origin: SettingsModelOrigin::Preset,
            availability: SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 0,
                motion_count: 1,
                behaviors: vec![behavior.clone()],
            },
        },
        SettingsModelEntry {
            id: "keyboard".to_owned(),
            origin: SettingsModelOrigin::Preset,
            availability: SettingsModelAvailability::Ready {
                texture_count: 1,
                expression_count: 1,
                motion_count: 0,
                behaviors: vec![SettingsModelBehavior::Expression {
                    name: "inactive".to_owned(),
                }],
            },
        },
    ];

    assert_eq!(
        super::model_actions::active_model_behavior_targets(&entries, Some(&active)),
        vec![(active, behavior)]
    );
    assert!(super::model_actions::active_model_behavior_targets(&entries, None).is_empty());
}

#[test]
fn invalid_model_status_is_stable_and_path_free() {
    let entry = SettingsModelEntry {
        id: "private-model".to_owned(),
        origin: SettingsModelOrigin::Installed,
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelReferenceSymlinkEscape,
        },
    };
    let status = model_availability_status(&entry, false, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(status, "Installed · Package layout is invalid");
    assert!(!status.contains("private-model"));
    assert!(!status.contains('/'));
}

#[test]
fn model_presentations_follow_the_resolved_language() {
    let ready = SettingsModelEntry {
        id: "preset-model".to_owned(),
        origin: SettingsModelOrigin::Preset,
        availability: SettingsModelAvailability::Ready {
            texture_count: 2,
            expression_count: 3,
            motion_count: 4,
            behaviors: Vec::new(),
        },
    };
    assert_eq!(
        model_availability_status(&ready, true, SettingsLanguage::ChineseSimplified),
        "预置 · 当前使用 · 2 个纹理 · 3 个表情 · 4 个动作"
    );

    let invalid = SettingsModelEntry {
        id: "installed-model".to_owned(),
        origin: SettingsModelOrigin::Installed,
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    };
    let invalid_status =
        model_availability_status(&invalid, false, SettingsLanguage::ChineseSimplified);
    assert_eq!(invalid_status, "已安装 · 纹理无效");
    assert_eq!(
        model_delete_confirmation(SettingsLanguage::ChineseSimplified, &invalid_status),
        "已安装 · 纹理无效 · 确认删除"
    );

    let (import_status, failed) = model_import_status(
        &ModelImportDraft::default(),
        SettingsLanguage::ChineseSimplified,
    );
    assert!(!failed);
    assert_eq!(import_status, "尚未选择文件夹");
    assert_eq!(
        model_import_progress(SettingsLanguage::ChineseSimplified, "正在复制", 5, 1024),
        "正在复制 · 5 个文件 · 1024 字节"
    );

    let failed_import = ModelImportDraft {
        state: ModelImportState::Failed(SettingsError::new(SettingsErrorCode::ModelImportFailed)),
        ..ModelImportDraft::default()
    };
    let (failed_status, failed) =
        model_import_status(&failed_import, SettingsLanguage::ChineseSimplified);
    assert!(failed);
    assert_eq!(failed_status, "无法导入模型");
}

#[test]
fn model_delete_confirmation_tab_order_matches_visual_order() {
    assert_eq!(
        model_row_action_tab_indices(40, false),
        ModelRowActionTabIndices {
            activate: 40,
            delete: 41,
            cancel_delete: 42,
        }
    );
    assert_eq!(
        model_row_action_tab_indices(40, true),
        ModelRowActionTabIndices {
            activate: 40,
            cancel_delete: 41,
            delete: 42,
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
