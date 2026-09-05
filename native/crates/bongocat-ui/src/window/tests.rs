use super::*;
use crate::{SettingsModelBehaviorBinding, SettingsShortcutBinding};
use gpui_kit::{Keystroke, Modifiers};

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

#[test]
fn shortcut_capture_canonicalizes_modifiers_and_named_keys() {
    let mut event = key("arrowleft", None);
    event.keystroke.modifiers.control = true;
    event.keystroke.modifiers.shift = true;
    assert_eq!(
        shortcut_from_key_event(&event).as_deref(),
        Some("Control+Shift+ArrowLeft")
    );

    let event = key("return", None);
    assert_eq!(shortcut_from_key_event(&event).as_deref(), Some("Enter"));
}

#[test]
fn shortcut_capture_rejects_modifier_only_and_unsupported_keys() {
    assert!(shortcut_from_key_event(&key("shift", None)).is_none());
    assert!(shortcut_from_key_event(&key("media-play", None)).is_none());
}

#[test]
fn escape_is_reserved_for_cancelling_capture() {
    let event = key("escape", None);
    assert!(is_capture_cancel(&event));

    let mut modified = key("escape", None);
    modified.keystroke.modifiers.control = true;
    assert!(!is_capture_cancel(&modified));
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
    assert!(shortcut_conflicts(&shortcuts));
}

#[test]
fn shortcut_capture_targets_have_independent_tab_stops() {
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
        model_behaviors: vec![SettingsModelBehaviorBinding {
            model_id: "standard".to_owned(),
            behavior_id: "motion:tap:0".to_owned(),
            shortcut: "Control+M".to_owned(),
        }],
    };
    let targets = shortcut_targets(&shortcuts);
    assert_eq!(targets.len(), 3);
    assert_eq!(shortcut_capture_tab_index(0), 35);
    assert_eq!(shortcut_capture_tab_index(1), 36);
    assert_eq!(shortcut_capture_tab_index(2), 37);
    assert_eq!(targets.into_iter().collect::<BTreeSet<_>>().len(), 3);
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let rows = shortcut_accessibility_rows(&shortcuts, SettingsLanguage::EnglishUnitedStates);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1, "Capture shortcut for Show or hide desktop cat");
        assert_eq!(rows[2].2, "Control+M");
        assert_eq!(
            shortcut_target_for_accessibility_node(&shortcuts, shortcut_accessibility_node_id(2)),
            Some(ShortcutCaptureTarget::ModelBehavior {
                model_id: "standard".to_owned(),
                behavior_id: "motion:tap:0".to_owned(),
            })
        );
    }
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
fn diagnostics_page_projects_only_named_aggregate_counters() {
    let diagnostics = SettingsInputDiagnostics {
        input_monitoring_permission: crate::SettingsInputMonitoringPermission::Granted,
        service_status: SettingsInputServiceStatus::Running,
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
        diagnostics_export_status(SettingsLanguage::ChineseSimplified, Some(128)),
        "已导出 128 字节"
    );
    let command = ShortcutCaptureTarget::Command("toggle_overlay".to_owned());
    assert_eq!(
        shortcut_target_name(SettingsLanguage::ChineseSimplified, &command),
        "显示或隐藏桌面猫"
    );
    assert_eq!(
        shortcut_accessibility_label(SettingsLanguage::ChineseSimplified, &command),
        "为显示或隐藏桌面猫录入快捷键"
    );
    assert_eq!(
        shortcut_capture_error(
            SettingsLanguage::ChineseSimplified,
            ShortcutCaptureError::UnsupportedKey,
        ),
        "不支持的按键"
    );
    assert_eq!(
        shortcut_capture_error(
            SettingsLanguage::ChineseSimplified,
            ShortcutCaptureError::AlreadyAssigned,
        ),
        "该快捷键已被占用"
    );
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
    assert_eq!(restored.detail, "Restart BongoCat to continue");
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
        "BongoCat 将在登录系统时启动"
    );
}
