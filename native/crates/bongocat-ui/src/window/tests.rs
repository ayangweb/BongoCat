use super::*;
use crate::{SettingsModelBehaviorBinding, SettingsShortcutBinding};
use gpui::{Keystroke, Modifiers};

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
        let rows = shortcut_accessibility_rows(&shortcuts);
        assert_eq!(rows.len(), 3);
        assert!(rows[0].1.contains("toggle_overlay"));
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
        service_status: SettingsInputServiceStatus::Running,
        service_start_attempts: 1,
        pressed_key_count: 1,
        pressed_mouse_button_count: 2,
        pressed_gamepad_button_count: 3,
        connected_gamepad_count: 4,
        captured_down: 5,
        captured_up: 6,
        reconciled_release: 7,
        released_by_reset: 8,
        duplicate_down: 9,
        unmatched_release: 10,
        invalid_source: 11,
        reset_count: 12,
        sequence_gap_count: 13,
        missing_sequence_count: 14,
        duplicate_sequence_count: 15,
        out_of_order_sequence_count: 16,
        non_monotonic_time_count: 17,
        gamepad_connections: 18,
        gamepad_disconnections: 19,
        stale_gamepad_events: 20,
        released_by_disconnect: 21,
        transport_enqueued: 22,
        transport_queue_full: 23,
        transport_recovered_after_overflow: 24,
        transport_runtime_stopped: 25,
    };
    let metrics = input_diagnostic_metrics(diagnostics);
    assert_eq!(metrics.len(), 25);
    assert_eq!(metrics.first(), Some(&("Pressed keys", 1)));
    assert_eq!(metrics.last(), Some(&("Rejected after shutdown", 25)));
    assert_eq!(
        metrics.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        (1..=25).collect::<Vec<_>>()
    );
    assert!(metrics.iter().all(|(label, _)| {
        !label.contains("HID") && !label.contains("path") && !label.contains("timestamp value")
    }));
    let service = input_service_presentation(diagnostics);
    assert_eq!(service.title, "Running");
    assert_eq!(service.detail, "Start attempts: 1");
    assert!(service.running);
    assert!(!service.attention);
}

#[test]
fn runtime_diagnostics_presentation_keeps_codes_anonymous_and_actionable() {
    let presentation = runtime_diagnostics_presentation(SettingsRuntimeDiagnostics {
        render_error: Some(SettingsRuntimeErrorCode::GpuPreparationFailed),
        last_command_failure: Some(crate::SettingsRuntimeCommandFailure {
            sequence: 17,
            code: SettingsRuntimeErrorCode::GpuPreparationFailed,
        }),
        command_transport: Default::default(),
    });
    assert_eq!(presentation.title, "GPU preparation failed");
    assert!(presentation.attention);
    assert_eq!(presentation.detail, "gpu_preparation_failed · command #17");
    assert!(!presentation.detail.contains('/'));
}

#[test]
fn input_service_status_keeps_permission_failure_actionable_and_anonymous() {
    let service = input_service_presentation(SettingsInputDiagnostics {
        service_status: SettingsInputServiceStatus::PermissionDenied,
        service_start_attempts: 1,
        ..SettingsInputDiagnostics::default()
    });
    assert_eq!(service.title, "Permission required");
    assert_eq!(service.detail, "Start attempts: 1");
    assert!(service.attention);
    assert!(!service.detail.contains("path"));
}

#[test]
fn configuration_recovery_presentation_is_anonymous_and_complete() {
    let normal = config_recovery_presentation(SettingsConfigurationStatus::Ready, None);
    assert_eq!(normal.title, "Loaded normally");
    assert_eq!(normal.detail, "No recovery");
    assert!(!normal.recovered);
    assert!(!normal.can_restore);

    let recovered = config_recovery_presentation(
        SettingsConfigurationStatus::Ready,
        Some(SettingsConfigRecovery {
            source_schema_version: 2,
            skipped_newer_backups: 3,
        }),
    );
    assert_eq!(recovered.title, "Recovered from backup");
    assert_eq!(recovered.detail, "Schema v2 · 3 newer backups skipped");
    assert!(recovered.recovered);
    assert!(!recovered.detail.contains('/') && !recovered.detail.contains('\\'));

    let one_skipped = config_recovery_presentation(
        SettingsConfigurationStatus::Ready,
        Some(SettingsConfigRecovery {
            source_schema_version: 2,
            skipped_newer_backups: 1,
        }),
    );
    assert_eq!(one_skipped.detail, "Schema v2 · 1 newer backup skipped");

    let required = config_recovery_presentation(
        SettingsConfigurationStatus::RecoveryRequired { checked_backups: 2 },
        None,
    );
    assert_eq!(required.title, "Configuration unavailable");
    assert_eq!(required.detail, "2 backup candidates checked");
    assert!(required.attention);
    assert!(required.can_restore);

    let restored = config_recovery_presentation(
        SettingsConfigurationStatus::DefaultsRestoredRestartRequired,
        None,
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
fn overlay_stepper_values_are_bounded_and_preserve_other_settings() {
    let settings = SettingsOverlay {
        click_through: false,
        always_on_top: false,
        scale_percent: 100,
        opacity_percent: 50,
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
    let (status, failed) = model_import_status(&draft);
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
    let (status, failed) = model_import_status(&draft);
    assert!(!failed);
    assert_eq!(status, "Selection cancelled; previous folder retained");
    assert!(!status.contains("private"));

    draft.state = ModelImportState::PickerFailed(DirectoryPickerError::SelectionInvalid);
    let (status, failed) = model_import_status(&draft);
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
    let (status, failed) = model_import_status(&draft);
    assert!(!failed);
    assert_eq!(status, "Choosing folder...");
}

#[test]
fn model_row_actions_preserve_origin_availability_and_active_identity() {
    let ready = SettingsModelAvailability::Ready {
        texture_count: 1,
        expression_count: 0,
        motion_count: 0,
    };
    let preset = SettingsModelEntry {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Preset,
        availability: ready,
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
fn invalid_model_status_is_stable_and_path_free() {
    let entry = SettingsModelEntry {
        id: "private-model".to_owned(),
        origin: SettingsModelOrigin::Installed,
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelReferenceSymlinkEscape,
        },
    };
    let status = model_availability_status(&entry, false);
    assert_eq!(status, "Installed · Package layout is invalid");
    assert!(!status.contains("private-model"));
    assert!(!status.contains('/'));
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
        let presentation = startup_item_presentation(Some(status), false);
        assert_eq!(presentation.enabled, enabled);
        assert_eq!(presentation.action, action);
        assert!(!presentation.description.is_empty());
        assert_eq!(
            startup_item_presentation(Some(status), true).action,
            StartupItemAction::None
        );
    }
    assert_eq!(
        startup_item_presentation(None, false).action,
        StartupItemAction::None
    );
}
