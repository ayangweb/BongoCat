use super::*;

#[test]
fn every_sound_select_publishes_localized_categories_without_changing_ids() {
    let mut plugin = KeySounds::default();
    let original_config = plugin.output().config;
    for locale in ["en-US", "zh-CN", "de-DE"] {
        let output = plugin
            .handle(HostMessage::Locale {
                locale: locale.into(),
            })
            .unwrap();
        let copy = copy_for(locale);
        assert_eq!(output.config, original_config);
        assert!(output.settings.validate());
        for node in output.settings.nodes {
            let UiNode::Select { key, options, .. } = node else {
                continue;
            };
            let category = if key.starts_with("keyboard_") {
                PackCategory::Keyboard
            } else {
                PackCategory::Mouse
            };
            for option in options {
                let pack = category
                    .packs()
                    .find(|pack| pack.id == option.value)
                    .unwrap();
                assert_eq!(option.group.as_deref(), Some(pack.brand));
                let expected_label = if !pack.supports_key_up {
                    format!("{} · {}", pack.label, copy.no_release)
                } else {
                    pack.label.to_owned()
                };
                assert_eq!(option.label, expected_label);
            }
        }
    }
}

/// The settings panel is the plugin's own copy, so switching language
/// re-labels it without touching what the user has configured.
#[test]
fn the_host_locale_relabels_the_settings_panel() {
    let mut plugin = KeySounds::default();
    let english = plugin.output();
    assert_eq!(integer_label(&plugin.output(), "volume"), "Volume");
    assert_eq!(
        select_label(&plugin.output(), "keyboard_pack"),
        "Keyboard sound"
    );
    assert_eq!(
        select_option_group(&english, "mouse_pack", "pixabay-unknown-unknown"),
        "Unknown"
    );
    assert_eq!(
        select_option_group(
            &plugin.output(),
            "keyboard_pack",
            PackCategory::Keyboard.default_pack().id
        ),
        PackCategory::Keyboard.default_pack().brand
    );

    let localized = plugin
        .handle(HostMessage::Locale {
            locale: "zh-CN".into(),
        })
        .unwrap();
    assert_eq!(integer_label(&localized, "volume"), "音量");
    assert_eq!(select_label(&localized, "mouse_pack"), "鼠标音效");
    assert_eq!(
        select_option_group(&localized, "mouse_pack", "pixabay-unknown-unknown"),
        "Unknown"
    );
    assert_eq!(
        select_option_group(
            &localized,
            "mouse_pack",
            PackCategory::Mouse.default_pack().id
        ),
        PackCategory::Mouse.default_pack().brand
    );
    assert_eq!(plugin.volume, DEFAULT_VOLUME);

    // A language this plugin does not carry reads English: the host only ever
    // sends the two tags the manifest declares, so nothing here has to resolve
    // a tag the plugin does not serve.
    for locale in ["en-US", "zh-TW", "ar-SA", "fr-FR", ""] {
        let fallback = plugin
            .handle(HostMessage::Locale {
                locale: locale.into(),
            })
            .unwrap();
        assert_eq!(
            select_label(&fallback, "mouse_pack"),
            "Mouse sound",
            "{locale}"
        );
        assert_eq!(plugin.volume, DEFAULT_VOLUME);
        assert_eq!(fallback.settings, english.settings, "{locale}");
        assert_eq!(fallback.config, english.config, "{locale}");
        assert_eq!(fallback.state, english.state, "{locale}");
    }
}

/// A playback failure is reported in the language on screen, and a language
/// change re-reads the message rather than leaving the old one behind.
#[test]
fn a_playback_failure_reads_the_language_on_screen() {
    // The failure is injected rather than provoked: this asserts the copy a
    // failure reads as, not that this machine has (or lacks) an output device.
    for (error, english, chinese) in [
        (
            AudioError::Decode,
            "The sound effect could not be decoded.",
            "无法解码音效。",
        ),
        (
            AudioError::Output,
            "No audio output device is available.",
            "没有可用的音频输出设备。",
        ),
    ] {
        let mut plugin = KeySounds {
            error: Some(error),
            ..Default::default()
        };
        assert_eq!(error_text(&plugin.output()), english);

        let localized = plugin
            .handle(HostMessage::Locale {
                locale: "zh-CN".into(),
            })
            .unwrap();
        assert_eq!(error_text(&localized), chinese);

        for locale in [
            "en-US", "zh-TW", "zh-HK", "ar-SA", "vi-VN", "pt-BR", "ko-KR", "fr-FR", "de-DE", "",
        ] {
            let fallback = plugin
                .handle(HostMessage::Locale {
                    locale: locale.into(),
                })
                .unwrap();
            assert_eq!(error_text(&fallback), english, "{locale}");
        }
    }
}

/// Auditioning is attached to each option rather than taking a separate
/// action row in the settings panel.
#[test]
fn the_settings_panel_has_no_preview_button() {
    let plugin = KeySounds::default();
    let output = plugin.output();
    assert!(
        !output
            .settings
            .nodes
            .iter()
            .any(|node| matches!(node, UiNode::Button { id, .. } if id == "preview"))
    );
}

#[test]
fn hovering_a_sound_option_previews_without_committing_configuration() {
    let mut plugin = KeySounds::default();
    plugin
        .configure(&Configuration::from([(
            "volume".into(),
            ConfigValue::Integer(0),
        )]))
        .unwrap();
    let before = plugin.output().config;
    let output = plugin
        .handle(HostMessage::Preview {
            key: "keyboard_pack".into(),
            value: PACKS
                .iter()
                .find(|pack| pack.category == PackCategory::Keyboard)
                .unwrap()
                .id
                .into(),
        })
        .unwrap();
    assert_eq!(output.config, before);
    assert!(plugin.error.is_none());
    assert!(plugin.playback.device.is_none());
    assert!(
        !plugin.library.decoded.is_empty(),
        "selected packs are preloaded"
    );
    assert!(
        plugin
            .handle(HostMessage::Preview {
                key: "mouse_pack".into(),
                value: PackCategory::Keyboard.default_pack().id.into(),
            })
            .is_err()
    );
    assert!(
        plugin
            .handle(HostMessage::Preview {
                key: "keyboard_pack".into(),
                value: "missing-pack".into(),
            })
            .is_err()
    );
}

#[test]
fn every_published_preview_resolves_to_its_device() {
    let plugin = KeySounds::default();
    for node in plugin.output().settings.nodes {
        if let UiNode::Select {
            key,
            options,
            preview,
            ..
        } = node
        {
            assert!(preview);
            let expected = Action::from_config_key(&key).unwrap();
            for option in options {
                let (action, pack) = plugin.preview_choice(&key, &option.value).unwrap();
                assert_eq!(action, expected);
                assert_eq!(pack.category, action.category());
            }
        }
    }
    assert!(plugin.preview_choice("keyboard_pack", "auto").is_err());
    assert!(
        plugin
            .preview_choice("keyboard_down_pack", "skcm-blue")
            .is_err(),
        "only the keys this plugin publishes are previewable"
    );
    assert!(plugin.preview_choice("volume", "skcm-blue").is_err());
    assert!(plugin.preview_choice("mouse_pack", "skcm-blue").is_err());
}

pub(super) fn install_test_voice(plugin: &mut KeySounds, preview: bool) {
    let (player, _output) = rodio::Player::new();
    player.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_secs(1)));
    plugin.playback.players.push(player);
    plugin.playback.previewing = preview;
}

#[test]
fn finished_preview_releases_ownership_before_the_next_input_batch() {
    let mut plugin = KeySounds::default();
    let (player, mut output) = rodio::Player::new();
    player.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_millis(1)));
    plugin.playback.players.push(player);
    plugin.playback.previewing = true;
    for _ in 0..100_000 {
        if !plugin.playback.playing() {
            break;
        }
        output.next();
    }
    assert!(!plugin.playback.playing());
    assert!(plugin.admit(Action::KeyboardDown, 100));
    assert!(!plugin.playback.previewing);
    install_test_voice(&mut plugin, false);
    assert!(
        plugin.admit(Action::KeyboardUp, 100),
        "normal press audio must not be mistaken for a preview"
    );
}

#[test]
fn preview_replaces_old_voice_and_pause_mute_reset_stop_are_safe() {
    // Starting real audio needs an output device, and not every host running
    // this suite has one, so the voice here is injected and the asserts cover
    // the state machine that must hold on any host. That a preview replaces the
    // previous voice and reuses the shared sink is covered by the real-device
    // tests below.
    let mut plugin = KeySounds::default();
    install_test_voice(&mut plugin, true);
    assert!(plugin.playback.preview_active());
    plugin.preview("keyboard_pack", "skcm-blue").unwrap();
    assert!(
        plugin.playback.players.len() <= 1,
        "a preview keeps at most the voice it started"
    );
    if plugin.playback.device.is_some() {
        assert!(plugin.playback.playing());
        assert!(plugin.error.is_none());
    }
    for message in [HostMessage::Pause, HostMessage::Reset, HostMessage::Stop] {
        install_test_voice(&mut plugin, true);
        plugin.handle(message).unwrap();
        assert!(!plugin.playback.preview_active());
        assert!(plugin.playback.players.is_empty());
    }
    plugin.handle(HostMessage::Pause).unwrap();
    plugin.preview("keyboard_pack", "skcm-blue").unwrap();
    assert!(
        plugin.playback.device.is_none(),
        "paused previews never open output"
    );
}

#[test]
#[ignore = "requires a real audio output device; run explicitly on macOS/Windows"]
fn rapid_previews_replace_voices_reuse_output_and_recover_normal_input() {
    let mut plugin = KeySounds {
        ..Default::default()
    };
    plugin.playback.last_played.insert(Action::KeyboardDown, 10);
    plugin.preview("keyboard_pack", "skcm-blue").unwrap();
    assert!(plugin.error.is_none());
    assert!(plugin.playback.preview_active());
    let device = std::ptr::from_ref(plugin.playback.device.as_ref().unwrap());
    for pack in PackCategory::Keyboard.packs().take(4) {
        plugin.preview("keyboard_pack", pack.id).unwrap();
        assert!(plugin.error.is_none());
        assert_eq!(plugin.playback.players.len(), 1);
        assert_eq!(
            device,
            std::ptr::from_ref(plugin.playback.device.as_ref().unwrap())
        );
        assert_eq!(plugin.playback.last_played[&Action::KeyboardDown], 10);
        plugin.play_events(
            vec![PluginInputEvent {
                control: InputEventControl::Keyboard,
                pressed: true,
                captured_ms: 100,
            }],
            0,
        );
        assert_eq!(plugin.playback.players.len(), 1);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while plugin.playback.preview_active() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!plugin.playback.preview_active());
    assert!(plugin.admit(Action::KeyboardDown, 200));
    plugin.play_from_pack(
        Action::KeyboardDown,
        Some(KeyGroup::Default),
        plugin.selection.keyboard,
    );
    assert!(plugin.playback.playing());
    plugin.handle(HostMessage::Stop).unwrap();
    assert!(plugin.playback.device.is_none());
}

/// The whole panel is one frame the host has to be able to draw: every select
/// publishes a value out of its own list, and the panel stays inside the node
/// budget the protocol gives it.
#[test]
fn the_published_panel_is_valid_and_bounded() {
    let plugin = KeySounds::default();
    let view = plugin.output().settings;
    assert!(view.validate());
    assert!(view.nodes.len() <= 16);
    for node in &view.nodes {
        if let UiNode::Select {
            key,
            value,
            options,
            ..
        } = node
        {
            assert!(
                options.iter().any(|option| &option.value == value),
                "{key} publishes a value its own list does not hold"
            );
        }
    }
}
