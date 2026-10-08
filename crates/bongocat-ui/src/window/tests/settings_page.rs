//! The settings controls outside the model library and shortcuts pages.

use super::*;

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
        origin: SettingsModelOrigin::BuiltIn,
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
        SettingsModelOrigin::BuiltIn,
        SettingsModelAvailability::Ready {
            behaviors: behaviors.to_vec(),
        },
    )];
    let rows = shortcut_behavior_rows(&SettingsShortcuts::default(), Some(&active), &entries, &[]);
    assert_eq!(rows.len(), behaviors.len());

    let english = rows
        .iter()
        .map(|row| row.name(SettingsLanguage::English))
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
        ShortcutCaptureTarget::ModelBehavior { model, behavior_id }
            if model.id == "standard" && !behavior_id.is_empty()
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

#[gpui_kit::test]
fn app_system_smoke_checks_visible_copy_and_bounds(cx: &mut TestAppContext) {
    let (view, visual) = settings_view(cx);
    let mut snapshot = crate::tests::snapshot(1, true, true);
    snapshot.resolved_language = SettingsLanguage::ChineseSimplified;
    view.update(visual, |view, _| view.snapshot = Some(snapshot));
    assert!(
        view.update(visual, |view, cx| view.show_app_system_for_smoke(cx))
            .is_ok()
    );

    view.update(visual, |view, _| {
        let snapshot = view.snapshot.as_mut().expect("logging snapshot");
        snapshot.logging.retention_days = 0;
    });
    assert!(
        view.update(visual, |view, cx| view.show_app_system_for_smoke(cx))
            .is_err(),
        "the smoke must reject a policy outside the visible 1..=30 day field"
    );
}

#[test]
fn logging_level_options_use_the_complete_reversible_localized_catalog() {
    let expected = [
        (
            SettingsLanguage::English,
            [
                "Errors only",
                "Errors and warnings",
                "Standard details",
                "Diagnostic details",
                "All details",
            ],
        ),
        (
            SettingsLanguage::ChineseSimplified,
            ["仅错误", "错误和警告", "常规信息", "诊断信息", "全部细节"],
        ),
    ];

    for (language, labels) in expected {
        assert_eq!(logging_level_options(language), labels);
        assert_eq!(
            labels.iter().copied().collect::<BTreeSet<_>>().len(),
            SettingsLogLevel::ALL.len()
        );
        for (level, label) in SettingsLogLevel::ALL.into_iter().zip(labels) {
            assert_eq!(logging_level_display_name(level, language), label);
            assert_eq!(
                logging_level_from_display_name(label, language),
                Some(level)
            );
        }
    }
    assert_eq!(
        logging_level_from_display_name("not a log level", SettingsLanguage::English),
        None
    );
}

#[test]
fn the_two_gamepad_auto_switch_dropdowns_offer_disjoint_model_families() {
    let ready = SettingsModelAvailability::Ready {
        behaviors: Vec::new(),
    };
    let entries = vec![
        model_entry_in_mode(
            "standard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelMode::Standard,
            ready.clone(),
        ),
        model_entry_in_mode(
            "keyboard",
            SettingsModelOrigin::BuiltIn,
            SettingsModelMode::Keyboard,
            ready.clone(),
        ),
        model_entry_in_mode(
            "gamepad",
            SettingsModelOrigin::BuiltIn,
            SettingsModelMode::Gamepad,
            ready.clone(),
        ),
        // A second gamepad model from the store, and an unusable one: neither may
        // reach the connected dropdown, and the invalid one must not reach
        // either.
        model_entry_in_mode(
            "imported-pad",
            SettingsModelOrigin::Imported,
            SettingsModelMode::Gamepad,
            ready.clone(),
        ),
        model_entry_in_mode(
            "broken-pad",
            SettingsModelOrigin::Imported,
            SettingsModelMode::Gamepad,
            SettingsModelAvailability::Invalid {
                diagnostic: SettingsModelDiagnostic::ModelJsonInvalid,
            },
        ),
    ];
    let language = SettingsLanguage::English;

    let connected =
        gamepad_auto_switch_options(&entries, GamepadConnectionState::Connected, None, language);
    let connected_targets = connected
        .iter()
        .map(|option| option.target.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        connected_targets,
        vec![
            None,
            Some(settings_model_key("gamepad", SettingsModelOrigin::BuiltIn)),
            Some(settings_model_key(
                "imported-pad",
                SettingsModelOrigin::Imported
            )),
        ],
        "the connected dropdown is the last-used choice plus the gamepad-mode models"
    );
    assert_eq!(connected[0].title(), "Last gamepad model used");

    let disconnected = gamepad_auto_switch_options(
        &entries,
        GamepadConnectionState::Disconnected,
        None,
        language,
    );
    assert_eq!(
        disconnected
            .iter()
            .map(|option| option.target.clone())
            .collect::<Vec<_>>(),
        vec![
            None,
            Some(settings_model_key("standard", SettingsModelOrigin::BuiltIn)),
            Some(settings_model_key("keyboard", SettingsModelOrigin::BuiltIn)),
        ],
        "the disconnected dropdown is the last-used choice plus the other modes"
    );
    assert_eq!(disconnected[0].title(), "Last non-gamepad model used");

    // A configured target that is no longer offered still has to be visible, or
    // the control would show nothing and the user could not change it back.
    let dangling = settings_model_key("deleted-pad", SettingsModelOrigin::Imported);
    let options = gamepad_auto_switch_options(
        &entries,
        GamepadConnectionState::Connected,
        Some(&dangling),
        language,
    );
    assert_eq!(
        options.last().map(|option| option.target.clone()),
        Some(Some(dangling))
    );
    assert_eq!(
        options.last().map(|option| option.title().to_string()),
        Some("deleted-pad".to_owned())
    );
    // A target that *is* offered is not duplicated.
    let offered = settings_model_key("gamepad", SettingsModelOrigin::BuiltIn);
    let options = gamepad_auto_switch_options(
        &entries,
        GamepadConnectionState::Connected,
        Some(&offered),
        language,
    );
    assert_eq!(options.len(), connected.len());

    // With no catalog at all, both dropdowns still offer the default choice.
    for state in [
        GamepadConnectionState::Connected,
        GamepadConnectionState::Disconnected,
    ] {
        let options = gamepad_auto_switch_options(&[], state, None, language);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].target, None);
        assert!(!options[0].title().is_empty());
    }
}

#[test]
fn update_check_interval_is_presented_as_whole_hours_from_one_to_one_year() {
    let options = check_for_updates_interval_number_field_options();
    assert_eq!(options.min, 1.0);
    assert_eq!(options.max, 8760.0);
    assert_eq!(options.step, 1.0);

    for (raw, expected) in [
        (f64::NEG_INFINITY, 1),
        (-10.2, 1),
        (0.49, 1),
        (1.0, 1),
        (23.5, 24),
        (24.0, 24),
        (47.6, 48),
        (8760.49, 8760),
        (f64::INFINITY, 8760),
        (f64::NAN, 24),
    ] {
        assert_eq!(
            normalize_check_for_updates_interval_hours(raw),
            expected,
            "{raw}"
        );
    }
}

#[test]
fn logging_retention_is_presented_as_whole_days_from_one_to_thirty() {
    let options = logging_retention_number_field_options();
    assert_eq!(options.min, 1.0);
    assert_eq!(options.max, 30.0);
    assert_eq!(options.step, 1.0);

    for (raw, expected) in [
        (f64::NEG_INFINITY, 1),
        (-10.2, 1),
        (0.49, 1),
        (1.0, 1),
        (7.4, 7),
        (7.5, 8),
        (30.49, 30),
        (31.0, 30),
        (f64::INFINITY, 30),
        (f64::NAN, 7),
    ] {
        assert_eq!(normalize_logging_retention_days(raw), expected, "{raw}");
    }
}

#[test]
fn appearance_theme_selection_has_stable_indices_and_system_projection() {
    assert_eq!(
        theme_options(SettingsLanguage::English),
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
        theme_from_display_name("Unknown", SettingsLanguage::English),
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
        hold_modifier_to_interact: Some(ModifierKey::LeftShift),
        always_on_top: false,
        scale_percent: 100,
        opacity_percent: 50,
        corner_radius_percent: 25,
        hide_on_pointer_hover: true,
        hide_on_pointer_hover_delay_seconds: 2,
        hide_on_idle: true,
        hide_on_idle_delay_seconds: 45,
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
    assert!(changed.hide_on_idle);
    assert_eq!(changed.hide_on_idle_delay_seconds, 45);
    assert!(!changed.keep_inside_screen);
    assert_eq!(
        changed.hold_modifier_to_interact,
        Some(ModifierKey::LeftShift),
        "stepping one overlay control must not drop the modifier the window reads to \
         decide the overlay is reachable"
    );
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

/// The idle hide delay follows the same rule as the hover one.
///
/// Both overlay backends arm the behaviour with `options.hide_on_idle &&
/// input_running`, so the delay row is inert while the switch is off, and
/// turning the switch off must leave the recorded delay alone.
#[test]
fn the_idle_hide_delay_only_applies_while_the_switch_is_on() {
    for (hide_on_idle, delay_seconds, expected) in [
        (false, 0, false),
        (false, 300, false),
        (true, 0, true),
        (true, 300, true),
    ] {
        let overlay = SettingsOverlay {
            hide_on_idle,
            hide_on_idle_delay_seconds: delay_seconds,
            ..SettingsOverlay::default()
        };
        assert_eq!(
            idle_hide_delay_applies(overlay),
            expected,
            "{overlay:?} must follow the switch rather than the delay"
        );
    }
}

#[test]
fn random_behavior_mode_change_keeps_a_pending_interval_in_the_same_patch() {
    let persisted = SettingsRandomBehavior {
        mode: SettingsRandomBehaviorMode::Off,
        interval_seconds: 30,
    };
    let pending = SettingsRandomBehavior {
        mode: SettingsRandomBehaviorMode::Off,
        interval_seconds: 12,
    };
    assert_eq!(
        super::settings::random_behavior_settings_after_mode_change(
            persisted,
            Some(pending),
            SettingsRandomBehaviorMode::MotionsAndExpressions,
        ),
        SettingsRandomBehavior {
            mode: SettingsRandomBehaviorMode::MotionsAndExpressions,
            interval_seconds: 12,
        }
    );
}

#[test]
fn random_behavior_mode_change_without_a_pending_value_keeps_the_persisted_interval() {
    let persisted = SettingsRandomBehavior {
        mode: SettingsRandomBehaviorMode::Motions,
        interval_seconds: 45,
    };
    assert_eq!(
        super::settings::random_behavior_settings_after_mode_change(
            persisted,
            None,
            SettingsRandomBehaviorMode::Off,
        ),
        SettingsRandomBehavior {
            mode: SettingsRandomBehaviorMode::Off,
            interval_seconds: 45,
        },
        "turning the mode off must keep the interval the user already chose"
    );
}

#[test]
fn the_random_behavior_mode_catalogue_is_reversible_in_every_language() {
    for language in SettingsLanguage::ALL {
        let options = random_behavior_mode_options(language);
        assert_eq!(options.len(), SettingsRandomBehaviorMode::ALL.len());
        assert!(options.iter().all(|option| !option.is_empty()));
        assert_eq!(
            options.iter().copied().collect::<BTreeSet<_>>().len(),
            options.len(),
            "{language:?} repeats a random behavior mode label"
        );
        for mode in SettingsRandomBehaviorMode::ALL {
            let label = random_behavior_mode_display_name(mode, language);
            assert_eq!(
                random_behavior_mode_from_display_name(label, language),
                Some(mode),
                "{language:?} labels are not reversible"
            );
        }
        assert_eq!(
            random_behavior_mode_from_display_name("not a random behavior mode", language),
            None
        );
    }
    // The order the catalogue is rendered in is the order the window offers, and
    // the first entry is the one a fresh configuration is on.
    assert_eq!(
        SettingsRandomBehaviorMode::ALL[0],
        SettingsRandomBehaviorMode::Off
    );
}

#[gpui_kit::test]
fn turning_random_behavior_on_and_off_queues_the_latest_mode(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    initial.random_behavior = SettingsRandomBehavior {
        mode: SettingsRandomBehaviorMode::Off,
        interval_seconds: 30,
    };
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_random_behavior_mode(SettingsRandomBehaviorMode::Motions, cx);
    });
    visual.run_until_parked();
    let first_command = endpoint
        .try_recv()
        .expect("first random behavior mode change");
    let crate::SettingsCommand::SetRandomBehaviorSettings {
        expected_config_revision,
        settings: first_settings,
        reply: first_reply,
    } = first_command
    else {
        panic!("the mode change must use the typed random behavior command");
    };
    assert_eq!(expected_config_revision, 7);
    assert_eq!(
        first_settings,
        SettingsRandomBehavior {
            mode: SettingsRandomBehaviorMode::Motions,
            interval_seconds: 30,
        }
    );

    view.update(visual, |view, cx| {
        view.set_random_behavior_mode(SettingsRandomBehaviorMode::Off, cx);
        view.flush_pending_settings(cx);
    });
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "the reversal must wait for the in-flight change"
    );

    let mut confirmed = crate::tests::snapshot(8, true, true);
    confirmed.config_revision = Some(8);
    confirmed.random_behavior = first_settings;
    first_reply
        .respond(Ok(confirmed))
        .expect("first mode change reply");
    view.update(visual, |view, cx| {
        view.flush_pending_settings(cx);
    });
    visual.run_until_parked();

    let second_command = endpoint
        .try_recv()
        .expect("reversal random behavior mode change");
    let crate::SettingsCommand::SetRandomBehaviorSettings {
        expected_config_revision,
        settings: second_settings,
        reply: second_reply,
    } = second_command
    else {
        panic!("the reversal must use the typed random behavior command");
    };
    assert_eq!(expected_config_revision, 8);
    assert_eq!(
        second_settings,
        SettingsRandomBehavior {
            mode: SettingsRandomBehaviorMode::Off,
            interval_seconds: 30,
        }
    );

    let mut completed = crate::tests::snapshot(9, true, true);
    completed.config_revision = Some(9);
    completed.random_behavior = second_settings;
    second_reply
        .respond(Ok(completed))
        .expect("reversal mode change reply");
    visual.run_until_parked();
    assert!(endpoint.try_recv().is_err());
}

#[gpui_kit::test]
fn the_remembered_expression_switch_sends_the_revision_it_was_rendered_from(
    cx: &mut TestAppContext,
) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(4, true, true);
    initial.config_revision = Some(4);
    assert!(
        !initial.remember_last_expression,
        "a fresh configuration restores nothing"
    );
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_remember_last_expression(true, cx);
    });
    visual.run_until_parked();
    let crate::SettingsCommand::SetRememberLastExpression {
        expected_config_revision,
        enabled,
        reply,
    } = endpoint
        .try_recv()
        .expect("the switch must reach the service as its own typed command")
    else {
        panic!("the switch must use the typed remembered-expression command");
    };
    assert_eq!(expected_config_revision, 4);
    assert!(enabled);

    // The switch is one click per write, so the confirmation is what releases the
    // next one rather than a debounce.
    let mut confirmed = crate::tests::snapshot(5, true, true);
    confirmed.config_revision = Some(5);
    confirmed.remember_last_expression = true;
    reply.respond(Ok(confirmed)).expect("switch reply");
    visual.run_until_parked();
    view.update(visual, |view, _| {
        assert_eq!(
            view.snapshot
                .as_ref()
                .map(|snapshot| snapshot.remember_last_expression),
            Some(true),
            "the confirmed snapshot is what the switch then renders"
        );
    });
    assert!(endpoint.try_recv().is_err());
}

/// The toggle is its own row and its own command, not a second field of the
/// remembered-expression switch.
///
/// They sit next to each other on the page, which is exactly why they need to stay
/// separable: remembering a face and reacting to a repeated trigger are different
/// questions, and a row that changed one while the user reached for the other is
/// the failure this guards.
#[gpui_kit::test]
fn the_expression_toggle_switch_sends_its_own_command(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(4, true, true);
    initial.config_revision = Some(4);
    assert!(
        !initial.toggle_repeated_expression,
        "a fresh configuration repeats an expression rather than turning it off"
    );
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_toggle_repeated_expression(true, cx);
    });
    visual.run_until_parked();
    let crate::SettingsCommand::SetToggleRepeatedExpression {
        expected_config_revision,
        enabled,
        reply,
    } = endpoint
        .try_recv()
        .expect("the switch must reach the service as its own typed command")
    else {
        panic!("the switch must use the typed expression-toggle command");
    };
    assert_eq!(expected_config_revision, 4);
    assert!(enabled);

    let mut confirmed = crate::tests::snapshot(5, true, true);
    confirmed.config_revision = Some(5);
    confirmed.toggle_repeated_expression = true;
    reply.respond(Ok(confirmed)).expect("switch reply");
    visual.run_until_parked();
    view.update(visual, |view, _| {
        assert_eq!(
            view.snapshot
                .as_ref()
                .map(|snapshot| snapshot.toggle_repeated_expression),
            Some(true),
            "the confirmed snapshot is what the switch then renders"
        );
        assert_eq!(
            view.snapshot
                .as_ref()
                .map(|snapshot| snapshot.remember_last_expression),
            Some(false),
            "confirming the toggle must not also move the remembered-expression switch"
        );
    });
    assert!(endpoint.try_recv().is_err());
}

#[gpui_kit::test]
fn the_random_behavior_interval_is_inert_while_the_mode_is_off(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(7, true, true);
    initial.config_revision = Some(7);
    initial.random_behavior = SettingsRandomBehavior {
        mode: SettingsRandomBehaviorMode::Off,
        interval_seconds: 30,
    };
    view.update(visual, |view, _| view.snapshot = Some(initial));

    view.update(visual, |view, cx| {
        view.set_random_behavior_interval_value(12.0, cx);
    });
    visual.run_until_parked();
    assert!(
        endpoint.try_recv().is_err(),
        "an interval nothing reads must not reach the service"
    );
}

#[gpui_kit::test]
fn motion_overlap_switch_uses_a_revision_checked_command(cx: &mut TestAppContext) {
    let (view, visual, endpoint) = settings_view_with_endpoint(cx);
    let mut initial = crate::tests::snapshot(4, true, true);
    initial.config_revision = Some(4);
    assert!(!initial.allow_motion_overlap);
    view.update(visual, |view, _| view.snapshot = Some(initial));
    view.update(visual, |view, cx| view.set_allow_motion_overlap(true, cx));
    visual.run_until_parked();
    let crate::SettingsCommand::SetAllowMotionOverlap {
        expected_config_revision,
        enabled,
        reply,
    } = endpoint.try_recv().expect("overlap command")
    else {
        panic!("expected typed overlap command");
    };
    assert_eq!(expected_config_revision, 4);
    assert!(enabled);
    let mut confirmed = crate::tests::snapshot(5, true, true);
    confirmed.config_revision = Some(5);
    confirmed.allow_motion_overlap = true;
    reply.respond(Ok(confirmed)).expect("switch reply");
    visual.run_until_parked();
    view.update(visual, |view, _| {
        assert!(
            view.snapshot
                .as_ref()
                .expect("confirmed snapshot")
                .allow_motion_overlap
        );
    });
    assert!(endpoint.try_recv().is_err());
}
