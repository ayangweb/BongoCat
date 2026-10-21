use super::*;
#[test]
fn plugin_errors_are_bounded_and_reject_internal_formatting() {
    let valid = PluginOutput {
        errors: vec![PluginError {
            id: "audio_device".into(),
            message: "No audio output device is available.".into(),
        }],
        ..Default::default()
    };
    assert!(valid.validate());
    assert!(
        !PluginOutput {
            errors: vec![PluginError {
                id: "audio/device".into(),
                message: "failure".into()
            }],
            ..Default::default()
        }
        .validate()
    );
    assert!(
        !PluginOutput {
            errors: vec![
                PluginError {
                    id: "audio".into(),
                    message: "secret /Users/person/file".into()
                },
                PluginError {
                    id: "audio".into(),
                    message: "second".into()
                }
            ],
            ..Default::default()
        }
        .validate()
    );
}
#[test]
fn inline_status_icons_are_bounded_and_round_trip_without_paths() {
    assert!(StatusIcon::Rgba([0, 0, 0, 128].repeat(1024)).is_template());
    assert!(!StatusIcon::Rgba([255, 0, 0, 128].repeat(1024)).is_template());
    let multitone = [[0, 0, 0, 255].repeat(512), vec![255; 2048]].concat();
    assert!(!StatusIcon::Rgba(multitone).is_template());
    for icon in [
        StatusIcon::Rgba(vec![255; 4096]),
        StatusIcon::Badge("拼音".into()),
    ] {
        let view = PluginView {
            nodes: vec![UiNode::IconText {
                icon,
                text: "Pinyin".into(),
            }],
            ..Default::default()
        };
        assert!(view.validate());
        let json = serde_json::to_vec(&view).unwrap();
        assert_eq!(serde_json::from_slice::<PluginView>(&json).unwrap(), view);
    }
    for icon in [
        StatusIcon::Rgba(vec![]),
        StatusIcon::Rgba(vec![0; 4097]),
        StatusIcon::Badge("".into()),
        StatusIcon::Badge("abcd".into()),
        StatusIcon::Badge("\n".into()),
    ] {
        assert!(!icon.validate());
    }
    let view = PluginView {
        nodes: vec![UiNode::IconText {
            icon: StatusIcon::Badge("A".into()),
            text: "bad\nname".into(),
        }],
        ..Default::default()
    };
    assert!(!view.validate());
}

#[test]
fn rejects_oversized_and_truncated_frames() {
    assert!(
        read_frame::<Request>(&mut ((MAX_MESSAGE_BYTES as u32 + 1).to_le_bytes().as_slice()))
            .is_err()
    );
    assert!(read_frame::<Request>(&mut [4, 0, 0, 0, 1].as_slice()).is_err());
}
#[test]
fn protocol_roundtrip() {
    let mut bytes = Vec::new();
    write_frame(
        &mut bytes,
        &Request {
            sequence: 7,
            message: HostMessage::Pause,
        },
    )
    .unwrap();
    let request: Request = read_frame(&mut bytes.as_slice()).unwrap();
    assert_eq!(request.sequence, 7);
}
#[test]
fn limits_views_and_config_ids() {
    assert!(!valid_key("../escape"));
    assert!(
        !PluginView {
            nodes: vec![UiNode::Text {
                text: "x".repeat(1025)
            }],
            ..Default::default()
        }
        .validate()
    );
}

#[test]
fn rejects_duplicate_control_ids_and_select_values() {
    let duplicate_controls = PluginView {
        nodes: vec![
            UiNode::button("same", "One", ActionIcon::Play),
            UiNode::button("same", "Two", ActionIcon::Pause),
        ],
        ..Default::default()
    };
    assert!(!duplicate_controls.validate());

    let duplicate_options = PluginView {
        nodes: vec![UiNode::Select {
            key: "pack".into(),
            label: "Pack".into(),
            value: "one".into(),
            options: vec![
                SelectOption {
                    value: "one".into(),
                    label: "One".into(),
                    group: None,
                },
                SelectOption {
                    value: "one".into(),
                    label: "Also one".into(),
                    group: None,
                },
            ],
            preview: false,
        }],
        ..Default::default()
    };
    assert!(!duplicate_options.validate());
}

fn localized_metadata() -> PluginMetadata {
    PluginMetadata {
        id: "com.bongocat.pomodoro".into(),
        name: "Pomodoro".into(),
        description: "A focus timer".into(),
        version: "1.0.0".into(),
        app_versions: ">=2.3.1".into(),
        protocol_version: PROTOCOL_VERSION,
        entry: "bongocat-pomodoro".into(),
        icon: PluginIcon::Emoji("🍅".into()),
        capabilities: vec![Capability::LocalizedViewsV1],
        localizations: BTreeMap::from([
            (
                "zh-CN".to_owned(),
                PluginCopy {
                    name: "番茄时钟".into(),
                    description: "专注计时器".into(),
                },
            ),
            (
                "zh-TW".to_owned(),
                PluginCopy {
                    name: "番茄鐘".into(),
                    description: "專注計時器".into(),
                },
            ),
        ]),
    }
}

#[test]
fn plugins_serve_only_simplified_chinese_and_english() {
    let metadata = localized_metadata();
    for locale in ["zh-CN", "zh_cn", "ZH-cn"] {
        assert_eq!(plugin_locale(locale), "zh-CN");
        assert_eq!(metadata.view_locale(locale), "zh-CN");
        assert_eq!(metadata.localized_copy(locale), ("番茄时钟", "专注计时器"));
    }
    for locale in [
        "en-US",
        "en-GB",
        "zh-TW",
        "zh-Hant",
        "zh",
        "zh-Hans-CN",
        "ar-SA",
        "vi-VN",
        "pt-BR",
        "ko-KR",
        "fr-FR",
        "",
        "unknown",
    ] {
        assert_eq!(plugin_locale(locale), DEFAULT_LOCALE);
        assert_eq!(metadata.view_locale(locale), DEFAULT_LOCALE);
        assert_eq!(
            metadata.localized_copy(locale),
            ("Pomodoro", "A focus timer")
        );
    }
}

#[test]
fn an_incomplete_translation_falls_back_for_the_entire_plugin() {
    for copy in [
        PluginCopy {
            name: "番茄时钟".into(),
            description: " ".into(),
        },
        PluginCopy {
            name: "".into(),
            description: "专注计时器".into(),
        },
    ] {
        let metadata = PluginMetadata {
            localizations: [("zh-CN".into(), copy)].into(),
            ..localized_metadata()
        };
        assert_eq!(metadata.view_locale("zh-CN"), DEFAULT_LOCALE);
        assert_eq!(
            metadata.localized_copy("zh-CN"),
            ("Pomodoro", "A focus timer")
        );
    }
}

/// Copy is supplied by the plugin that publishes it, so a locale entry the
/// manifest omitted a field for is a malformed manifest rather than a
/// partial translation the host would have to complete.
#[test]
fn a_partial_locale_entry_is_not_a_valid_manifest() {
    let mut manifest = serde_json::to_value(localized_metadata()).unwrap();
    manifest["localizations"]["zh-CN"] = serde_json::json!({ "name": "番茄时钟" });
    assert!(serde_json::from_value::<PluginMetadata>(manifest.clone()).is_err());
    manifest["localizations"]["zh-CN"] = serde_json::json!({
        "name": "番茄时钟",
        "description": "专注计时器",
        "extra": "not part of the contract",
    });
    assert!(serde_json::from_value::<PluginMetadata>(manifest).is_err());
}

/// English is the manifest's own name and description, so a locale map that
/// repeats it would give one language two sources.
#[test]
fn a_locale_map_may_not_restate_the_english_copy() {
    let mut manifest = serde_json::to_value(localized_metadata()).unwrap();
    manifest["localizations"]["en-US"] = serde_json::json!({
        "name": "Pomodoro",
        "description": "A focus timer",
    });
    assert!(serde_json::from_value::<PluginMetadata>(manifest).is_err());
}

#[test]
fn a_manifest_round_trips_through_its_manifest_file() {
    let metadata = localized_metadata();
    let bytes = serde_json::to_vec(&metadata).unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded["name"], "Pomodoro");
    assert_eq!(decoded["localizations"]["zh-CN"]["name"], "番茄时钟");
    assert_eq!(
        serde_json::from_slice::<PluginMetadata>(&bytes).unwrap(),
        metadata
    );
}

/// A button always names the glyph it asks the host to draw, and may report
/// that it is carrying the action out.
#[test]
fn a_button_names_its_icon_and_may_be_in_flight() {
    let view = PluginView {
        nodes: vec![
            UiNode::button("toggle", "Start", ActionIcon::Play),
            UiNode::Button {
                id: "preview".into(),
                label: "Preview".into(),
                icon: ActionIcon::Volume,
                loading: true,
            },
        ],
        ..Default::default()
    };
    assert!(view.validate());
    // The host draws the plugin's own glyph, so a frame that omits it is a
    // malformed frame rather than one the host guesses an icon for.
    for node in [
        r#"{"type":"button","id":"toggle","label":"Start"}"#,
        r#"{"type":"button","id":"toggle","label":"Start","icon":"play"}"#,
    ] {
        let frame = format!(r#"{{"anchor":"top_right","nodes":[{node}]}}"#);
        assert!(
            serde_json::from_str::<PluginView>(&frame).is_err(),
            "{node}"
        );
    }
    let frame: PluginView = serde_json::from_str(
        r#"{"anchor":"top_right","nodes":[{"type":"button","id":"toggle","label":"Start","icon":"play","loading":false}]}"#,
    )
    .unwrap();
    assert!(matches!(
        frame.nodes.as_slice(),
        [UiNode::Button {
            icon: ActionIcon::Play,
            loading: false,
            ..
        }]
    ));
}

/// A select is valid only when the choice it publishes is one of its own
/// options, so the host never draws a selection the list cannot show.
#[test]
fn a_select_carries_its_own_options_and_a_member_value() {
    let node = |value: &str, options: Vec<SelectOption>| UiNode::Select {
        key: "keyboard_pack".into(),
        label: "Keyboard sound".into(),
        value: value.into(),
        options,
        preview: false,
    };
    let options = vec![
        SelectOption {
            value: "alps-skcm-blue".into(),
            label: "SKCM Blue · Alps".into(),
            group: None,
        },
        SelectOption {
            value: "kailh-box-navy".into(),
            label: "Box Navy · Kailh".into(),
            group: None,
        },
    ];
    let view = PluginView {
        nodes: vec![node("kailh-box-navy", options.clone())],
        ..Default::default()
    };
    assert!(view.validate());
    let ungrouped: UiNode = serde_json::from_str(
        r#"{"type":"select","key":"pack","label":"Pack","value":"one","options":[{"value":"one","label":"One","group":null}],"preview":false}"#,
    )
    .unwrap();
    assert!(matches!(ungrouped, UiNode::Select { preview: false, .. }));
    let UiNode::Select { options, .. } = ungrouped else {
        panic!("the select node round trips");
    };
    assert_eq!(options[0].group, None);

    for invalid in [
        node("unknown-pack", options.clone()),
        node("kailh-box-navy", Vec::new()),
        node(
            "kailh-box-navy",
            vec![SelectOption {
                value: "../escape".into(),
                label: "Box Navy".into(),
                group: None,
            }],
        ),
    ] {
        assert!(
            !PluginView {
                nodes: vec![invalid],
                ..Default::default()
            }
            .validate()
        );
    }

    for group in [Some(String::new()), Some(" ".into()), Some("x".repeat(129))] {
        assert!(
            !PluginView {
                nodes: vec![node(
                    "kailh-box-navy",
                    vec![SelectOption {
                        value: "kailh-box-navy".into(),
                        label: "Box Navy".into(),
                        group,
                    }],
                )],
                ..Default::default()
            }
            .validate()
        );
    }

    let grouped: UiNode = serde_json::from_str(
        r#"{"type":"select","key":"pack","label":"Pack","value":"one","options":[{"value":"one","label":"One","group":"Keyboard"}],"preview":true}"#,
    )
    .unwrap();
    // Every field of the node is part of the contract, so a frame that omits
    // one is rejected rather than completed by the host.
    assert!(
        serde_json::from_str::<UiNode>(
            r#"{"type":"select","key":"pack","label":"Pack","value":"one","options":[{"value":"one","label":"One","group":"Keyboard"}]}"#,
        )
        .is_err()
    );
    // An absent group decodes to `None`; a group that is present must name a
    // category, so an empty one is not a heading the host would have to draw.
    assert_eq!(
        serde_json::from_str::<SelectOption>(r#"{"value":"one","label":"One"}"#)
            .unwrap()
            .group,
        None
    );
    assert!(
        !PluginView {
            nodes: vec![UiNode::Select {
                key: "pack".into(),
                label: "Pack".into(),
                value: "one".into(),
                options: vec![
                    serde_json::from_str(r#"{"value":"one","label":"One","group":""}"#).unwrap()
                ],
                preview: false,
            }],
            ..Default::default()
        }
        .validate(),
        "an empty group is not a category"
    );
    let UiNode::Select { options, .. } = grouped else {
        panic!("the select node round trips");
    };
    assert_eq!(options[0].group.as_deref(), Some("Keyboard"));
}

/// The activity message carries counters and nothing else, and a plugin
/// frames it the same way on both sides of the pipe. Every counter is part
/// of the frame, so a partial one is refused rather than read as zeros.
#[test]
fn input_activity_round_trips_counters_only() {
    let mut bytes = Vec::new();
    write_frame(
        &mut bytes,
        &Request {
            sequence: 1,
            message: HostMessage::InputActivity {
                elapsed_ms: 1_000,
                activity: InputActivity {
                    keyboard_down: 3,
                    keyboard_up: 2,
                    mouse_down: 1,
                    mouse_up: 1,
                    space_down: 1,
                    space_up: 1,
                    enter_down: 1,
                    enter_up: 1,
                    backspace_down: 1,
                    backspace_up: 1,
                    tab_down: 1,
                    tab_up: 1,
                    shift_left_down: 1,
                    shift_left_up: 1,
                    shift_right_down: 1,
                    shift_right_up: 1,
                    ctrl_left_down: 1,
                    ctrl_left_up: 1,
                },
            },
        },
    )
    .unwrap();
    let request: Request = read_frame(&mut bytes.as_slice()).unwrap();
    let HostMessage::InputActivity {
        elapsed_ms,
        activity,
    } = request.message
    else {
        panic!("the frame is the activity message it was written as");
    };
    assert_eq!(elapsed_ms, 1_000);
    assert_eq!(
        activity,
        InputActivity {
            keyboard_down: 3,
            keyboard_up: 2,
            mouse_down: 1,
            mouse_up: 1,
            space_down: 1,
            space_up: 1,
            enter_down: 1,
            enter_up: 1,
            backspace_down: 1,
            backspace_up: 1,
            tab_down: 1,
            tab_up: 1,
            shift_left_down: 1,
            shift_left_up: 1,
            shift_right_down: 1,
            shift_right_up: 1,
            ctrl_left_down: 1,
            ctrl_left_up: 1,
        }
    );
    assert!(activity.advanced_since(InputActivity::default()));
    assert!(!activity.advanced_since(activity));
}
