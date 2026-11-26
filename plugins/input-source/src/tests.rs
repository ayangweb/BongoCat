use super::*;

fn source(id: &str) -> Result<InputSource, InputSourceError> {
    Ok(InputSource {
        id: id.into(),
        name: format!("Source {id}"),
        icon_rgba: None,
        label: None,
    })
}

fn switch_config() -> Configuration {
    [
        ("mode".into(), ConfigValue::Text("switch".into())),
        ("seconds".into(), ConfigValue::Integer(2)),
    ]
    .into()
}

#[test]
fn settings_omit_source_preview_and_default_to_always_show() {
    let mut plugin = InputSourceIndicator::default();
    for icon_rgba in [None, Some(vec![255; 4096])] {
        plugin.observe(Ok(InputSource {
            id: "test".into(),
            name: "Source test".into(),
            icon_rgba,
            label: None,
        }));
        let output = plugin.output();
        assert_eq!(output.config["mode"], ConfigValue::Text("always".into()));
        assert!(matches!(
            output.settings.nodes.as_slice(),
            [UiNode::Select { key, value, .. }] if key == "mode" && value == "always"
        ));
        assert!(matches!(
            output.model.nodes.as_slice(),
            [UiNode::IconText { text, .. }] if text == "Source test"
        ));
    }
    plugin.configure(&switch_config()).unwrap();
    let output = plugin.output();
    assert!(matches!(
        output.settings.nodes.as_slice(),
        [UiNode::Select { value, .. }, UiNode::Integer { key, .. }]
            if value == "switch" && key == "seconds"
    ));
}

#[test]
fn saved_display_modes_are_preserved_and_missing_mode_defaults_to_always() {
    let mut plugin = InputSourceIndicator::default();
    for mode in ["switch", "always"] {
        let saved: Configuration = serde_json::from_value(serde_json::json!({
            "mode": { "type": "text", "value": mode },
            "seconds": { "type": "integer", "value": 4 }
        }))
        .unwrap();
        plugin.configure(&saved).unwrap();
        assert_eq!(plugin.output().config, saved);
    }
    plugin.configure(&switch_config()).unwrap();
    plugin
        .configure(&[("seconds".into(), ConfigValue::Integer(4))].into())
        .unwrap();
    let output = plugin.output();
    assert_eq!(output.config["mode"], ConfigValue::Text("always".into()));
    assert_eq!(output.config["seconds"], ConfigValue::Integer(4));
}

#[test]
fn always_on_and_switch_only_initial_baseline() {
    let mut plugin = InputSourceIndicator::default();
    plugin.observe(source("a"));
    assert_eq!(plugin.output().model.nodes.len(), 1);
    plugin.configure(&switch_config()).unwrap();
    assert!(plugin.output().model.nodes.is_empty());
    plugin.now_ms = 100;
    plugin.observe(source("a"));
    assert!(plugin.output().model.nodes.is_empty());
    plugin.observe(source("b"));
    assert_eq!(plugin.output().model.nodes.len(), 1);
    plugin.now_ms = 2099;
    plugin.observe(source("b"));
    assert_eq!(plugin.output().model.nodes.len(), 1);
    plugin.now_ms = 2100;
    assert!(plugin.output().model.nodes.is_empty());
}

#[test]
fn rapid_switches_replace_status_and_restart_deadline_not_repeated_snapshots() {
    let mut plugin = InputSourceIndicator::default();
    plugin.configure(&switch_config()).unwrap();
    plugin.observe(source("a"));
    plugin.now_ms = 50;
    plugin.observe(source("b"));
    plugin.now_ms = 100;
    plugin.observe(source("c"));
    assert_eq!(plugin.visible_until, Some(2100));
    assert!(
        matches!(&plugin.output().model.nodes[0], UiNode::IconText { text, .. } if text == "Source c")
    );
    plugin.observe(source("c"));
    assert_eq!(plugin.visible_until, Some(2100));
    plugin.now_ms = u64::MAX - 5;
    plugin.observe(source("d"));
    assert_eq!(plugin.visible_until, Some(u64::MAX));
}

#[test]
fn unavailable_hides_stale_source_and_recovery_rebaselines() {
    let mut plugin = InputSourceIndicator::default();
    plugin.configure(&switch_config()).unwrap();
    plugin.observe(source("a"));
    plugin.observe(source("b"));
    plugin.observe(Err(InputSourceError));
    assert!(plugin.output().model.nodes.is_empty());
    assert!(plugin.output().errors[0].message.contains("Retrying"));
    plugin.observe(source("c"));
    assert!(plugin.output().errors.is_empty());
    assert!(plugin.output().model.nodes.is_empty());
    plugin.observe(source("d"));
    assert!(!plugin.output().model.nodes.is_empty());
}

#[test]
fn configuration_is_atomic_bounded_and_old_empty_configuration_works() {
    let mut plugin = InputSourceIndicator::default();
    plugin.configure(&switch_config()).unwrap();
    for config in [
        [("mode".into(), ConfigValue::Text("unknown".into()))].into(),
        [("mode".into(), ConfigValue::Boolean(true))].into(),
        [("seconds".into(), ConfigValue::Integer(0))].into(),
        [("seconds".into(), ConfigValue::Integer(11))].into(),
        [("seconds".into(), ConfigValue::Text("2".into()))].into(),
        [("unexpected".into(), ConfigValue::Integer(2))].into(),
    ] {
        let before = plugin.output().config;
        assert!(plugin.configure(&config).is_err());
        assert_eq!(plugin.output().config, before);
    }
    plugin.configure(&Configuration::new()).unwrap();
    assert!(!plugin.switch_only);
    assert_eq!(plugin.seconds, 2);
}

#[test]
fn locale_changes_and_reset_preserve_system_name_and_locale_contract() {
    let mut plugin = InputSourceIndicator::default();
    plugin.observe(source("拼音"));
    for (locale, label) in [
        ("zh-CN", "显示方式"),
        ("en-US", "Display mode"),
        ("zh-TW", "Display mode"),
    ] {
        let output = plugin
            .handle(HostMessage::Locale {
                locale: locale.into(),
            })
            .unwrap();
        assert!(
            matches!(&output.settings.nodes[0], UiNode::Select { label: actual, .. } if actual == label)
        );
        assert!(
            matches!(&output.model.nodes[0], UiNode::IconText { text, .. } if text == "Source 拼音")
        );
    }
    plugin.locale = "zh-CN".into();
    plugin.configure(&switch_config()).unwrap();
    let output = plugin.handle(HostMessage::Reset).unwrap();
    assert_eq!(plugin.locale, "zh-CN");
    assert!(!output.model.nodes.is_empty());
}

#[test]
fn native_icons_badges_long_names_and_published_frames_are_bounded() {
    let mut plugin = InputSourceIndicator::default();
    for icon in [None, Some(vec![255; 4096]), Some(vec![1; 5])] {
        plugin.observe(Ok(InputSource {
            id: "test".into(),
            name: "输\n入法".repeat(1000),
            icon_rgba: icon,
            label: None,
        }));
        let output = plugin.output();
        assert!(output.validate());
        assert!(
            matches!(&output.model.nodes[0], UiNode::IconText { text, .. } if !text.contains('\n') && text.len() <= 512)
        );
        bongocat_plugin_sdk::write_frame(&mut Vec::new(), &output).unwrap();
    }
    plugin.observe(Ok(InputSource {
        id: "bad".into(),
        name: " \n ".into(),
        icon_rgba: None,
        label: None,
    }));
    assert!(plugin.output().model.nodes.is_empty());
}

#[test]
fn a_system_label_is_drawn_when_the_source_has_no_icon() {
    let mut plugin = InputSourceIndicator::default();
    plugin.observe(Ok(InputSource {
        id: "layout".into(),
        name: "美式键盘".into(),
        icon_rgba: None,
        label: Some("ENG".into()),
    }));

    let output = plugin.output();

    assert!(matches!(
        &output.model.nodes[0],
        UiNode::IconText { icon: StatusIcon::Badge(label), text } if label == "ENG" && text == "美式键盘"
    ));
}

#[test]
fn an_unusable_label_falls_back_to_the_name_badge() {
    let mut plugin = InputSourceIndicator::default();
    for label in [None, Some(String::new()), Some("TOOLONG".into())] {
        plugin.observe(Ok(InputSource {
            id: "layout".into(),
            name: "美式键盘".into(),
            icon_rgba: None,
            label,
        }));

        let output = plugin.output();

        assert!(matches!(
            &output.model.nodes[0],
            UiNode::IconText { icon: StatusIcon::Badge(label), .. } if label == "美式"
        ));
    }
}

#[test]
fn manifest_identity_and_locale_fallback_match_the_binary() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    let metadata: bongocat_plugin_sdk::PluginMetadata =
        serde_json::from_value(manifest["metadata"].clone()).unwrap();
    assert_eq!(metadata.id, InputSourceIndicator::default().id());
    assert_eq!(metadata.entry, env!("CARGO_PKG_NAME"));
    assert_eq!(metadata.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(metadata.localized_copy("zh-CN").0, "输入法状态");
    for locale in ["en-US", "de-DE", "zh-TW", "unknown"] {
        assert_eq!(metadata.localized_copy(locale).0, "Input Source");
    }
}

#[test]
fn pause_stop_and_backwards_ticks_do_not_leave_stale_display() {
    let mut plugin = InputSourceIndicator::default();
    plugin.observe(source("a"));
    assert!(
        plugin
            .handle(HostMessage::Pause)
            .unwrap()
            .model
            .nodes
            .is_empty()
    );
    assert!(plugin.monitor.is_none());
    plugin.now_ms = 5000;
    let output = plugin
        .handle(HostMessage::Tick { elapsed_ms: 100 })
        .unwrap();
    assert_eq!(plugin.now_ms, 5000);
    assert!(output.model.nodes.is_empty());
    let stopped = plugin.handle(HostMessage::Stop).unwrap();
    assert!(plugin.source.is_none() && plugin.visible_until.is_none());
    assert!(
        stopped.errors.is_empty(),
        "stopping is not an unavailable input source"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ticks_only_advance_display_time_without_querying_the_system() {
    let mut plugin = InputSourceIndicator::default();
    plugin.observe(source("fixture"));
    let before = plugin.source.clone();
    plugin
        .handle(HostMessage::Tick { elapsed_ms: 1000 })
        .unwrap();
    assert_eq!(plugin.source, before);
    assert!(plugin.monitor.is_none());
    assert_eq!(plugin.now_ms, 1000);
}

#[cfg(target_os = "windows")]
#[test]
fn windows_ticks_observe_the_source_because_no_notification_reaches_this_process() {
    // The host's clock is the only trigger on Windows: a switch inside the window
    // the user is already in produces no notification this process can receive.
    // A tick therefore has to read, which is what re-creating a released monitor
    // proves — an event-driven tick would leave it released.
    let mut plugin = InputSourceIndicator::default();
    plugin
        .handle(HostMessage::Start {
            config: Configuration::new(),
            state: vec![],
        })
        .unwrap();
    plugin.monitor = None;
    let output = plugin
        .handle(HostMessage::Tick { elapsed_ms: 1000 })
        .unwrap();
    assert!(plugin.monitor.is_some(), "a tick must read the source");
    assert_eq!(plugin.now_ms, 1000);
    assert!(output.validate());
}
