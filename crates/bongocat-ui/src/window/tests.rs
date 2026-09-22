use super::shortcuts_page::ShortcutScope;
use super::*;
use crate::{
    SettingsModelBehaviorBinding, SettingsModelCatalog, SettingsModelCatalogError,
    SettingsShortcutBinding,
};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{ElementId, Keystroke, Modifiers, TestAppContext, VisualTestContext};

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

/// Conflict is a property of bindings that are live at the same moment, not of
/// the persisted configuration as a whole. Every model counts its own behavior
/// defaults from the first digit of the primary modifier, so the same chord
/// routinely appears under two models; only the model the user is on can
/// conflict with itself, a command, or nothing else.
#[test]
fn shortcut_capture_conflicts_are_scoped_to_one_model() {
    let binding =
        |model_id: &str, behavior_id: &str, shortcut: &str| SettingsModelBehaviorBinding {
            model_id: model_id.to_owned(),
            behavior_id: behavior_id.to_owned(),
            shortcut: shortcut.to_owned(),
        };

    let cross_model = SettingsShortcuts {
        commands: Vec::new(),
        model_behaviors: vec![
            binding("standard", "motion:CAT_motion:0", "Control+1"),
            binding("keyboard", "motion:CAT_motion:0", "ctrl+1"),
        ],
    };
    assert_eq!(conflicting_shortcut(&cross_model), None);

    let same_model = SettingsShortcuts {
        commands: Vec::new(),
        model_behaviors: vec![
            binding("standard", "motion:CAT_motion:0", "Control+1"),
            binding("standard", "motion:CAT_motion:1", "Control+1"),
        ],
    };
    assert_eq!(
        conflicting_shortcut(&same_model).as_deref(),
        Some("Control+1")
    );

    let shadows_a_command = SettingsShortcuts {
        commands: vec![SettingsShortcutBinding {
            command: "toggle_overlay".to_owned(),
            shortcut: "Control+1".to_owned(),
        }],
        model_behaviors: vec![binding("keyboard", "motion:CAT_motion:0", "Control+1")],
    };
    assert_eq!(
        conflicting_shortcut(&shadows_a_command).as_deref(),
        Some("Control+1")
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
            shortcut: None,
        }
        .name(SettingsLanguage::EnglishUnitedStates),
        "Show or hide model window"
    );
}

/// A model's behaviors are labelled by flattened position, not by the resource
/// identity the package declares.
///
/// The numbering runs across motion groups before the expressions start, so a
/// model with two motions in `CAT_motion` and two in `CAT_motion_lock` reads
/// "Motion 1..4" rather than restarting at the group boundary, and the same
/// number never appears twice inside one kind.
#[test]
fn model_behavior_rows_are_named_by_flattened_position() {
    let active = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };
    let behaviors = [
        SettingsModelBehavior::Motion {
            group: "CAT_motion".to_owned(),
            index: 0,
        },
        SettingsModelBehavior::Motion {
            group: "CAT_motion".to_owned(),
            index: 1,
        },
        SettingsModelBehavior::Motion {
            group: "CAT_motion_lock".to_owned(),
            index: 0,
        },
        SettingsModelBehavior::Motion {
            group: "CAT_motion_lock".to_owned(),
            index: 1,
        },
        SettingsModelBehavior::Expression {
            name: "live2d_expression0.exp3.json".to_owned(),
        },
        SettingsModelBehavior::Expression {
            name: "live2d_expression1.exp3.json".to_owned(),
        },
        SettingsModelBehavior::Expression {
            name: "live2d_expression2.exp3.json".to_owned(),
        },
    ];
    let entries = vec![model_entry(
        "standard",
        SettingsModelOrigin::Preset,
        SettingsModelAvailability::Ready {
            behaviors: behaviors.to_vec(),
        },
    )];
    let rows = shortcut_behavior_rows(&SettingsShortcuts::default(), Some(&active), &entries);
    assert_eq!(rows.len(), behaviors.len());

    let english = rows
        .iter()
        .map(|row| row.name(SettingsLanguage::EnglishUnitedStates))
        .collect::<Vec<_>>();
    assert_eq!(
        english,
        [
            "Motion 1",
            "Motion 2",
            "Motion 3",
            "Motion 4",
            "Expression 1",
            "Expression 2",
            "Expression 3",
        ]
    );

    let chinese = rows
        .iter()
        .map(|row| row.name(SettingsLanguage::ChineseSimplified))
        .collect::<Vec<_>>();
    assert_eq!(
        chinese,
        [
            "动作 1", "动作 2", "动作 3", "动作 4", "表情 1", "表情 2", "表情 3",
        ]
    );

    // The label is a display concern only: the row still binds the package's own
    // identity, which is what the configuration stores.
    assert!(rows.iter().all(|row| matches!(
        &row.target,
        ShortcutCaptureTarget::ModelBehavior { model_id, behavior_id }
            if model_id == "standard" && !behavior_id.is_empty()
    )));
    assert_eq!(
        rows.len(),
        rows.iter()
            .map(|row| row.target.clone())
            .collect::<BTreeSet<_>>()
            .len()
    );
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

/// The hover hide delay belongs to the switch above it.
///
/// Both overlay backends arm the behaviour with
/// `options.hide_on_pointer_hover && input_running`, so the delay is only read while
/// that switch is on. This pins the key the row reads for its own enabled state: the
/// switch, not the delay's value — which is also why turning the switch off leaves the
/// delay the user recorded untouched instead of resetting it.
#[test]
fn the_hover_hide_delay_only_applies_while_the_switch_is_on() {
    for (hide_on_pointer_hover, delay_seconds, expected) in [
        (false, 0, false),
        (false, 30, false),
        (true, 0, true),
        (true, 30, true),
    ] {
        let overlay = SettingsOverlay {
            hide_on_pointer_hover,
            hide_on_pointer_hover_delay_seconds: delay_seconds,
            ..SettingsOverlay::default()
        };
        assert_eq!(
            hover_hide_delay_applies(overlay),
            expected,
            "{overlay:?} must follow the switch rather than the delay"
        );
    }
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
    // The request has landed, so the card stops offering to send another one.
    assert!(draft.shows_cancel());
    assert!(!draft.is_cancellable());
}

#[test]
fn model_catalog_statuses_cover_loading_empty_and_error() {
    assert_eq!(
        super::models::empty_model_catalog_status(None, SettingsLanguage::EnglishUnitedStates),
        "Loading models…"
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
}

#[test]
fn the_import_card_never_contains_the_selected_path() {
    // The card reports the step it is on, never the source: a path on screen
    // would be the one place the page leaks where a user keeps their files.
    for state in [
        ModelImportState::Idle,
        ModelImportState::Picking,
        ModelImportState::Starting {
            cancel_requested: false,
        },
        ModelImportState::Capturing,
    ] {
        let draft = ModelImportDraft {
            title: "custom-model".to_owned(),
            source_root: Some(PathBuf::from("/private/secret/model")),
            state,
            ..ModelImportDraft::default()
        };
        for step in super::models::import_card_step(&draft, SettingsLanguage::EnglishUnitedStates)
            .into_iter()
        {
            assert!(
                !step.contains("private") && !step.contains("secret"),
                "a step label must not name the source path: {step}"
            );
        }
    }
}

#[test]
fn an_open_picker_blocks_starting_another_import() {
    let draft = ModelImportDraft {
        title: "custom-model".to_owned(),
        source_root: Some(PathBuf::from("/private/source")),
        state: ModelImportState::Picking,
        ..ModelImportDraft::default()
    };

    assert!(draft.is_picker_open());
    assert!(!draft.can_import());
    // A dialog cannot be cancelled from the card, so it offers no control.
    assert!(!draft.shows_cancel());
    assert_eq!(
        super::models::import_card_step(&draft, SettingsLanguage::EnglishUnitedStates).as_deref(),
        Some("Opening the file picker…"),
        "an open dialog is the step the card reports"
    );
}

#[test]
fn the_suggested_title_is_the_chosen_folders_own_name() {
    let root = PathBuf::from("/private/我的猫 · 标准模式");
    assert_eq!(suggested_model_title(&root), "我的猫 · 标准模式");
    // A path with no name of its own has nothing to suggest, so the page falls
    // back to the placeholder the service also uses.
    assert_eq!(suggested_model_title(&PathBuf::from("/")), "custom-model");
}

#[test]
fn model_row_actions_preserve_origin_availability_and_active_identity() {
    let ready = SettingsModelAvailability::Ready {
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

    // An imported model keeps its delete control while it is the one on screen:
    // deleting it switches the runtime to the standard preset first, so the
    // control cannot disappear from a card the user imported.
    let active_installed = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Installed,
    };
    assert_eq!(
        model_row_actions(&installed, Some(&active_installed), false),
        ModelRowActions {
            active: true,
            can_activate: false,
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
}

/// An open delete question is dropped as soon as its control would not be drawn.
///
/// The card renders the delete control — and with it the confirmation surface —
/// only while `can_delete` holds, so the page has to drop the question under the
/// same conditions. A question that outlived its control would come back unasked
/// the moment the card could draw that control again.
#[test]
fn an_open_delete_question_lives_only_while_its_control_would() {
    let ready = SettingsModelAvailability::Ready {
        behaviors: Vec::new(),
    };
    let preset = model_entry("duplicate", SettingsModelOrigin::Preset, ready.clone());
    let installed = model_entry("duplicate", SettingsModelOrigin::Installed, ready);
    let active_preset = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };
    let target = SettingsModelKey {
        id: "duplicate".to_owned(),
        origin: SettingsModelOrigin::Installed,
    };
    let catalog = [preset, installed.clone()];

    assert!(model_delete_confirmation_is_valid(
        &catalog,
        Some(&active_preset),
        false,
        &target
    ));

    // The target can stop being deletable in two ways, and each one takes the
    // control off the card: it leaves the catalog, or editing is structurally
    // blocked (an import running, a picker open). Becoming the active model is
    // not one of them — an imported model keeps its delete control while it is
    // the one on screen — and neither is an in-flight command, which never
    // feeds the visual gate (ADR-0053).
    assert!(
        model_delete_confirmation_is_valid(&catalog, Some(&target), false, &target),
        "an imported model that became active still offers deletion"
    );
    assert!(
        !model_delete_confirmation_is_valid(&catalog[..1], Some(&active_preset), false, &target),
        "a model that left the catalog has no card left to ask on"
    );
    assert!(
        !model_delete_confirmation_is_valid(&catalog, Some(&active_preset), true, &target),
        "a command in flight takes the delete control off the card"
    );

    // Availability is not part of it: a package that failed to load is exactly the
    // one a user wants to remove.
    let broken = SettingsModelEntry {
        availability: SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
        },
        ..installed
    };
    assert!(model_delete_confirmation_is_valid(
        &[broken],
        Some(&active_preset),
        false,
        &target
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
                behaviors: vec![motion.clone(), expression],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
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
                behaviors: vec![behavior.clone()],
            },
        ),
        model_entry(
            "keyboard",
            SettingsModelOrigin::Preset,
            SettingsModelAvailability::Ready {
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

/// The page's two scopes are groups, so each one's rows have to be a half of the
/// one combined list the keyboard tab order is
/// numbered from.
#[test]
fn shortcut_scopes_split_the_combined_row_order_into_two_halves() {
    let active = SettingsModelKey {
        id: "standard".to_owned(),
        origin: SettingsModelOrigin::Preset,
    };
    let entries = vec![model_entry(
        "standard",
        SettingsModelOrigin::Preset,
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

/// A shortcut target maps to exactly the scope whose switch gates its row:
/// application commands to the window gate, model behaviors to the model
/// gate. The render layer and the mutating methods
/// all route through this mapping, so a drift here would make a disabled row
/// accept edits through one of the other layers.
#[test]
fn shortcut_targets_map_to_the_scope_that_gates_them() {
    let command = ShortcutCaptureTarget::Command("toggle_overlay".to_owned());
    let behavior = ShortcutCaptureTarget::ModelBehavior {
        model_id: "model".to_owned(),
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
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelReferenceSymlinkEscape,
        },
    );
    let status = model_availability_status(&entry, SettingsLanguage::EnglishUnitedStates);
    assert_eq!(
        status.as_ref().map(|status| status.as_ref()),
        Some("Installed · Package layout is invalid")
    );
    let status = status.as_ref().map(|status| status.as_ref()).unwrap_or("");
    assert!(!status.contains("private-model"));
    assert!(!status.contains('/'));
}

#[test]
fn model_presentations_follow_the_resolved_language() {
    let ready = model_entry(
        "preset-model",
        SettingsModelOrigin::Preset,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    );
    // A ready model card shows no status line at all: the counts summary was
    // removed, so there is nothing left to localize for it.
    assert!(model_availability_status(&ready, SettingsLanguage::ChineseSimplified).is_none());

    let invalid = model_entry(
        "installed-model",
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    );
    let invalid_status = model_availability_status(&invalid, SettingsLanguage::ChineseSimplified)
        .expect("invalid models keep a diagnostic status");
    assert_eq!(invalid_status, "已安装 · 模型纹理无效");

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

    // The capture is a real second phase, and it replaces the import line rather
    // than being appended under it: the card reports one step at a time.
    let capturing = ModelImportDraft {
        state: ModelImportState::Capturing,
        ..ModelImportDraft::default()
    };
    assert_eq!(
        super::models::import_card_step(&capturing, SettingsLanguage::ChineseSimplified).as_deref(),
        Some("正在截取封面中…")
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
        // The switch position answers the two steady states, so they are the only
        // ones without row copy; every state that needs explaining still has it.
        let steady_state = matches!(
            status,
            SettingsStartupItemStatus::State(
                SettingsStartupItemState::Disabled | SettingsStartupItemState::Enabled
            )
        );
        assert_eq!(
            presentation.description.is_some(),
            !steady_state,
            "{status:?} must carry row copy exactly when the switch cannot explain itself"
        );
        let build_unavailable = matches!(
            status,
            SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment
            ))
        );
        assert_eq!(
            presentation.unavailable_hint,
            build_unavailable.then(|| bongocat_i18n::text(
                SettingsLanguage::EnglishUnitedStates.catalog_locale(),
                "settings.application.startup.unsupported_build",
            )),
            "{status:?} must carry a hover hint exactly when the build cannot offer login startup"
        );
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
        None
    );
    // A state the switch cannot explain still carries the localized copy, so the
    // steady states are the only rows that lost their second line.
    assert_eq!(
        startup_item_presentation(
            Some(SettingsStartupItemStatus::State(
                SettingsStartupItemState::Stale
            )),
            false,
            SettingsLanguage::ChineseSimplified,
        )
        .description,
        Some("应用位置已变化；重新开关一次即可修复")
    );
}

/// The switch is greyed out exactly where the build cannot offer login startup.
///
/// Transient states do not disable it: `action` already reports whether the
/// control can act right now.
/// node. A released build therefore keeps the switch normally available while
/// the snapshot is still loading.
#[test]
fn the_startup_switch_is_disabled_exactly_where_the_build_cannot_offer_it() {
    let statuses = [
        None,
        Some(SettingsStartupItemStatus::ReadError(
            crate::SettingsStartupItemError::StateReadFailed,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Disabled,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Enabled,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Stale,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::RequiresApproval,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::NotFound,
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Unsupported(
                SettingsStartupItemUnsupportedReason::BuildEnvironment,
            ),
        )),
        Some(SettingsStartupItemStatus::State(
            SettingsStartupItemState::Unsupported(SettingsStartupItemUnsupportedReason::Platform),
        )),
    ];

    for status in statuses {
        for blocked in [false, true] {
            let presentation =
                startup_item_presentation(status, blocked, SettingsLanguage::EnglishUnitedStates);
            let build_cannot_offer_it = matches!(
                status,
                Some(SettingsStartupItemStatus::State(
                    SettingsStartupItemState::Unsupported(
                        SettingsStartupItemUnsupportedReason::BuildEnvironment
                    )
                ))
            );
            assert_eq!(
                presentation.switch_disabled(),
                build_cannot_offer_it,
                "{status:?} blocked={blocked} disabled the switch for the wrong reason"
            );
        }
    }
}

/// A development build's switch is disabled and explains why on hover.
///
/// The row already carries the same sentence, and the tooltip is the only place
/// a user learns why the control does not respond: the reason is a property of
/// the build, so no action in the window can resolve it. Both copies come from
/// one catalog entry, which is what keeps them from drifting apart.
#[test]
fn a_development_build_disables_the_startup_switch_with_a_hover_hint() {
    let status = SettingsStartupItemStatus::State(SettingsStartupItemState::Unsupported(
        SettingsStartupItemUnsupportedReason::BuildEnvironment,
    ));

    let presentation =
        startup_item_presentation(Some(status), false, SettingsLanguage::ChineseSimplified);
    assert!(presentation.switch_disabled());
    assert_eq!(
        presentation.unavailable_hint,
        Some("开发版本不支持登录时启动")
    );
    assert_eq!(presentation.unavailable_hint, presentation.description);
    assert_eq!(presentation.action, StartupItemAction::None);
}

/// Every state a released build can produce keeps the switch operable.
///
/// The presentation knows nothing about the build environment, so this is the
/// released direction expressed where it can be checked without a released
/// build: no actionable state is greyed out and none carries a hover hint.
#[test]
fn the_startup_switch_stays_operable_in_every_actionable_state() {
    for (status, expected) in [
        (SettingsStartupItemState::Disabled, true),
        (SettingsStartupItemState::Enabled, false),
        (SettingsStartupItemState::RequiresApproval, false),
    ] {
        let presentation = startup_item_presentation(
            Some(SettingsStartupItemStatus::State(status)),
            false,
            SettingsLanguage::EnglishUnitedStates,
        );
        assert!(!presentation.switch_disabled());
        assert_eq!(presentation.unavailable_hint, None);
        assert_eq!(presentation.action, StartupItemAction::SetEnabled(expected));
    }
}

/// Deleting a model is two steps, and the first one asks nothing of the service.
///
/// The card's delete control opens a confirmation surface and the accept button
/// runs `ModelRowAction::Delete`; this pins both halves of that split — opening
/// and closing only move the confirmation, and only `Delete` reaches the
/// service. The surface itself is covered by `pop_confirm`'s own tests; what is
/// checked here is that the page hands it the right two callbacks.
#[gpui_kit::test]
fn deleting_a_model_asks_the_service_only_after_the_confirmation(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let installed = model_entry(
        "duplicate",
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    );
    let model = SettingsModelKey {
        id: installed.id.clone(),
        origin: installed.origin,
    };
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![installed];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();

    // `gpui-component` resolves its overlays through a `Root` at the top of the
    // window, so the page is built as a child of one and the handle is carried out
    // of the builder rather than taken from the root.
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::EnglishUnitedStates,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        Root::new(view, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    view.update(visual, |view, cx| {
        view.snapshot = Some(seeded);
        view.sync_model_row_focus(&entries, active.as_ref(), false, cx);
    });

    // Opening the confirmation is a state change on this page and nothing else:
    // a model that is asked about is not a model that has been deleted.
    //
    // Both negative checks below settle the executor before looking at the
    // channel. A command is sent from a spawned task, so an unsettled executor
    // would report an empty channel no matter what the page did, and the check
    // would hold even if opening the question really did delete the model.
    view.update(visual, |view, cx| {
        view.request_model_delete(model.clone(), cx);
    });
    assert_eq!(
        view.read_with(visual, |view, _| view.model_delete_confirmation.clone()),
        Some(model.clone()),
        "asking about a model must record which model the question is about"
    );
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "opening the confirmation must not reach the service"
    );

    view.update(visual, |view, cx| {
        view.cancel_model_delete(&model, cx);
    });
    assert_eq!(
        view.read_with(visual, |view, _| view.model_delete_confirmation.clone()),
        None,
        "declining must drop the question"
    );
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "declining must not reach the service"
    );

    // The accept button runs the row action, and that is what deletes.
    view.update(visual, |view, cx| {
        view.request_model_delete(model.clone(), cx);
    });
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.run_model_row_action(ModelRowAction::Delete, model.clone(), window, cx);
        });
    });
    visual.run_until_parked();
    assert!(
        matches!(
            endpoint.try_recv(),
            Ok(crate::SettingsCommand::DeleteModel { .. })
        ),
        "accepting must ask the service to delete the model it named"
    );
}

/// The models page's own content, with the import card and the model cards in it.
///
/// The page renders through `SettingItem::render`, which only runs for the page
/// the settings component has selected, so this harness calls
/// `models::content` the same way that closure does rather than trying to make
/// the component select a page.
struct ModelsPageHarness {
    view: Entity<SettingsView>,
    snapshot: Option<SettingsSnapshot>,
}

impl Render for ModelsPageHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.snapshot.clone();
        let tokens = Tokens::from_theme(cx);
        self.view
            .clone()
            .update(cx, move |view, cx| {
                super::models::content(view, window, cx, snapshot.as_ref(), tokens)
            })
            .into_any_element()
    }
}

/// The bounds an element was painted at.
fn rendered_bounds(visual: &mut VisualTestContext, id: ElementId) -> Bounds<Pixels> {
    visual.update(|window, _| {
        let drawn = id.clone();
        window
            .try_find(id)
            .unwrap_or_else(|| panic!("{drawn:?} must be drawn"))
            .bounds()
    })
}

/// The import card is exactly as tall as the model cards it is laid out beside.
///
/// This is the page's real grid, not a stand-in for it: the model card here
/// carries a status line, which is what makes it taller than the import card's
/// own floor, so a card that kept its own height would come out short. Nothing
/// else about the page is asserted — the card's own tests cover its two faces.
#[gpui_kit::test]
fn the_import_card_is_as_tall_as_the_model_cards_beside_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let mut seeded = crate::tests::snapshot(1, false, true);
    // An unavailable model is the case that makes the two heights differ: its
    // status line is what pushes a model card past the import card's floor.
    seeded.model_catalog.entries = vec![model_entry(
        "broken",
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Invalid {
            diagnostic: SettingsModelDiagnostic::ModelTextureMissing,
        },
    )];
    let entries = seeded.model_catalog.entries.clone();
    let active = seeded.active_model.clone();
    let page_snapshot = seeded.clone();

    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::EnglishUnitedStates,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        let page = cx.new(|_| ModelsPageHarness {
            view,
            snapshot: Some(page_snapshot),
        });
        // The cards own `PopConfirm` surfaces, which resolve through a `Root`.
        Root::new(page, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    view.update(visual, |view, cx| {
        view.snapshot = Some(seeded);
        view.sync_model_row_focus(&entries, active.as_ref(), false, cx);
    });
    visual.update(|window, cx| window.render_frame(cx));

    let import = rendered_bounds(
        visual,
        ElementId::from((ElementId::from("model-import-card"), "trigger")),
    );
    let model = rendered_bounds(visual, ElementId::from(("model-card", 0usize)));

    assert_eq!(
        import.origin.y, model.origin.y,
        "the two cells must share a row for their heights to be comparable"
    );
    assert_eq!(
        import.size.height, model.size.height,
        "the grid's first cell must be as tall as the model cards beside it"
    );
    assert!(
        import.size.height > px(super::models::MODEL_CARD_MIN_HEIGHT),
        "a row is as tall as its tallest cell, so the card must be taller than its own floor"
    );
}

/// The in-flight command flag never feeds the shared visual-gate predicate
/// (ADR-0053).
///
/// `select_model`, `open_model_location` and `delete_model` hold `pending` for
/// the round trip. Gating a page on it disabled and re-enabled every control
/// around each command — every click read as the page refreshing. Every page
/// now reads one predicate, `SettingsView::editing_blocked`, so this pins the
/// predicate itself (it must stay false while a command merely waits), and the
/// models page as a consumer of it: an open delete question survives a
/// re-projection rendered under `pending`, and a press on a card's control
/// during the wait still reaches nothing.
#[gpui_kit::test]
fn an_in_flight_command_never_flickers_the_models_page_gate(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, endpoint) = crate::SettingsClient::bounded(4);
    let installed = model_entry(
        "duplicate",
        SettingsModelOrigin::Installed,
        SettingsModelAvailability::Ready {
            behaviors: Vec::new(),
        },
    );
    let model = SettingsModelKey {
        id: installed.id.clone(),
        origin: installed.origin,
    };
    let mut seeded = crate::tests::snapshot(1, false, true);
    seeded.model_catalog.entries = vec![installed];
    let page_snapshot = seeded.clone();

    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::EnglishUnitedStates,
                    appearance_theme: SettingsTheme::System,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        let page = cx.new(|_| ModelsPageHarness {
            view,
            snapshot: Some(page_snapshot),
        });
        // The cards own `PopConfirm` surfaces, which resolve through a `Root`.
        Root::new(page, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");
    view.update(visual, |view, _| {
        view.snapshot = Some(seeded);
        view.pending = Some(PendingOperation::ModelSelection);
        // `request_model_delete` refuses while a command is in flight, so the
        // question is planted the way the confirm button leaves it: open, on a
        // card whose delete control the gate still draws.
        view.model_delete_confirmation = Some(model.clone());
    });
    visual.update(|window, cx| window.render_frame(cx));

    // The predicate itself: with a snapshot on hand, an idle import and no
    // picker, a merely waiting command must not block any page.
    assert!(
        !view.read_with(visual, |view, _| view
            .editing_blocked(view.snapshot.as_ref())),
        "the shared gate must not read the in-flight flag"
    );

    assert_eq!(
        view.read_with(visual, |view, _| view.model_delete_confirmation.clone()),
        Some(model.clone()),
        "the page's gate must not read the in-flight flag: the question's \
         control stays drawn while a command waits"
    );

    // Keeping the controls drawn must not reopen the door for a second
    // command: the press is refused by the command methods, not by the paint.
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.run_model_row_action(ModelRowAction::Activate, model.clone(), window, cx);
        });
    });
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "a press while a command is in flight must not reach the service"
    );
}

/// The frames before the first snapshot render the seeded appearance.
///
/// The window is created and shown while its first snapshot is still in flight, so
/// every frame in that gap has no snapshot to read. Resolving the defaults there is
/// what made a Simplified Chinese window paint one frame of English and then
/// re-render itself, which is the animation this pins down: the seed a window is
/// opened with has to answer until the snapshot replaces it, and the snapshot has
/// to win once it exists.
#[gpui_kit::test]
fn the_frames_before_the_first_snapshot_render_the_seeded_appearance(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (client, _endpoint) = crate::SettingsClient::bounded(4);
    let built: Rc<RefCell<Option<Entity<SettingsView>>>> = Rc::new(RefCell::new(None));
    let capture = Rc::clone(&built);
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| {
            SettingsView::new(
                client,
                SettingsWindowSeed {
                    language: SettingsLanguage::ChineseSimplified,
                    appearance_theme: SettingsTheme::Dark,
                },
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                window,
                cx,
            )
        });
        capture.borrow_mut().replace(view.clone());
        Root::new(view, window, cx)
    });
    let view = built
        .borrow_mut()
        .take()
        .expect("the window builder must hand the page out");

    view.update(visual, |view, cx| {
        assert!(
            view.snapshot.is_none(),
            "test premise: no snapshot has arrived yet"
        );
        assert_eq!(view.display_language(), SettingsLanguage::ChineseSimplified);
        assert_eq!(view.display_appearance_theme(), SettingsTheme::Dark);
        cx.notify();
    });
    visual.run_until_parked();

    // The snapshot is what the window renders from once it exists, seed or not.
    view.update(visual, |view, cx| {
        view.snapshot = Some(crate::tests::snapshot(1, true, false));
        cx.notify();
    });
    visual.run_until_parked();
    view.update(visual, |view, cx| {
        assert_eq!(
            view.display_language(),
            SettingsLanguage::EnglishUnitedStates,
            "an arrived snapshot must replace the seed"
        );
        cx.notify();
    });
    visual.run_until_parked();
}
