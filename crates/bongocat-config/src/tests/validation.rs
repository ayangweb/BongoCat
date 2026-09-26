//! The schema, its defaults, its bounds and its version gate.

use super::*;

#[test]
fn system_locale_resolves_to_simplified_chinese_or_english() {
    assert_eq!(
        Language::from_system_locale("zh-Hans-CN"),
        Language::ChineseSimplified
    );
    assert_eq!(
        Language::from_system_locale("zh_Hant_HK"),
        Language::EnglishUnitedStates
    );
    assert_eq!(
        Language::from_system_locale("en-GB"),
        Language::EnglishUnitedStates
    );
    assert_eq!(
        Language::from_system_locale("de-DE"),
        Language::EnglishUnitedStates
    );
    assert_eq!(
        Language::System.resolve(Language::ChineseSimplified),
        Language::ChineseSimplified
    );
    assert_eq!(
        Language::System.resolve(Language::System),
        Language::EnglishUnitedStates
    );
    assert_eq!(
        Language::EnglishUnitedStates.resolve(Language::ChineseSimplified),
        Language::EnglishUnitedStates
    );
    assert_eq!(
        Language::ChineseSimplified.resolve(Language::EnglishUnitedStates),
        Language::ChineseSimplified
    );
}

#[test]
fn language_codes_round_trip_and_unsupported_values_are_rejected() {
    for language in Language::ALL {
        let json = serde_json::to_string(&language).expect("serialize language");
        assert_eq!(json, format!("\"{}\"", language.code()));
        assert_eq!(
            serde_json::from_str::<Language>(&json).expect("deserialize language"),
            language
        );
    }
    assert!(serde_json::from_str::<Language>("\"de-DE\"").is_err());

    let fixture =
        include_str!("../../../../shared/config/fixtures/invalid-unsupported-language.json");
    assert!(parse_config(fixture.as_bytes()).is_err());
}

#[test]
fn unknown_and_legacy_fields_are_rejected() {
    let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    value["general"] = serde_json::json!({ "old_pinia_field": true });
    let error = serde_json::from_value::<NativeConfig>(value).expect_err("unknown field");
    assert!(error.to_string().contains("unknown field"));

    let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    value["application"]["launch_at_login"] = serde_json::Value::Bool(true);
    let error = serde_json::from_value::<NativeConfig>(value)
        .expect_err("platform startup state must not enter config");
    assert!(error.to_string().contains("unknown field"));

    let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    value["overlay"]["hideOnHover"] = serde_json::Value::Bool(true);
    let error = serde_json::from_value::<NativeConfig>(value)
        .expect_err("legacy store spelling must not enter the initial v1 config");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn overlay_hover_hide_delay_accepts_the_first_version_range() {
    for accepted in [0_u32, 1, 2, 30, 59, 60] {
        let mut config = NativeConfig::default();
        config.overlay.hide_on_pointer_hover_delay_seconds = accepted;
        assert!(
            config.validate().is_ok(),
            "hover hide delay {accepted} must be accepted"
        );
    }
    for rejected in [61_u32, 120, u32::MAX] {
        let mut config = NativeConfig::default();
        config.overlay.hide_on_pointer_hover_delay_seconds = rejected;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue(
                "overlay.hide_on_pointer_hover_delay_seconds"
            ))
        ));
    }
}

#[test]
fn overlay_hover_hide_switch_defaults_to_off_and_round_trips() {
    let config = NativeConfig::default();
    assert!(!config.overlay.hide_on_pointer_hover);
    assert_eq!(config.overlay.hide_on_pointer_hover_delay_seconds, 0);

    let mut enabled = config;
    enabled.overlay.hide_on_pointer_hover = true;
    enabled.overlay.hide_on_pointer_hover_delay_seconds = 3;
    enabled.validate().expect("enabled hover hide is valid");
    let encoded = serde_json::to_string(&enabled).expect("serialize enabled hover hide");
    let decoded: NativeConfig =
        serde_json::from_str(&encoded).expect("deserialize enabled hover hide");
    assert_eq!(decoded, enabled);
}

#[test]
fn random_behavior_settings_use_a_bounded_positive_interval() {
    let default = NativeConfig::default();
    assert!(!default.model.random_behavior.enabled);
    assert_eq!(
        default.model.random_behavior.interval_seconds,
        DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS
    );
    for accepted in [
        MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
        30,
        MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
    ] {
        let mut config = NativeConfig::default();
        config.model.random_behavior.enabled = true;
        config.model.random_behavior.interval_seconds = accepted;
        config
            .validate()
            .expect("random behavior interval should be accepted");
    }
    for rejected in [0, MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS + 1, u32::MAX] {
        let mut config = NativeConfig::default();
        config.model.random_behavior.interval_seconds = rejected;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue(
                "model.random_behavior.interval_seconds"
            ))
        ));
    }
}

#[test]
fn gamepad_auto_switch_defaults_to_off_without_targets_and_validates_target_ids() {
    let default = NativeConfig::default();
    assert!(!default.model.gamepad_auto_switch.enabled);
    // Both targets default to `None`, which is "the last model activated for
    // this input family" rather than "no target".
    assert_eq!(default.model.gamepad_auto_switch.connected_model, None);
    assert_eq!(default.model.gamepad_auto_switch.disconnected_model, None);
    default
        .validate()
        .expect("an unconfigured gamepad auto switch is valid");

    for accepted in ["gamepad", "my-cat_1.2", "A"] {
        let mut config = NativeConfig::default();
        config.model.gamepad_auto_switch.enabled = true;
        config.model.gamepad_auto_switch.connected_model = Some(ModelIdentity {
            id: accepted.to_owned(),
            source: ModelSource::BuiltIn,
        });
        config.model.gamepad_auto_switch.disconnected_model = Some(ModelIdentity {
            id: accepted.to_owned(),
            source: ModelSource::Imported,
        });
        config
            .validate()
            .expect("a portable gamepad auto switch target is accepted");
        let encoded = serde_json::to_string(&config).expect("serialize the auto switch");
        let decoded: NativeConfig =
            serde_json::from_str(&encoded).expect("deserialize the auto switch");
        assert_eq!(decoded, config);
    }

    // A target is a complete identity: the same rules that guard
    // `model.selected_model.id` guard both directions here.
    for rejected in ["", "..", "nested/model", "CON", "nul.json"] {
        let mut config = NativeConfig::default();
        config.model.gamepad_auto_switch.disconnected_model = Some(ModelIdentity {
            id: rejected.to_owned(),
            source: ModelSource::Imported,
        });
        assert!(
            matches!(
                config.validate(),
                Err(ConfigError::InvalidValue(
                    "model.gamepad_auto_switch.disconnected_model.id"
                ))
            ),
            "gamepad auto switch target {rejected:?} must be rejected"
        );
    }
}

#[test]
fn overlay_corner_radius_accepts_the_legacy_percentage_range() {
    for accepted in [0_u8, 1, 12, 25, 49, 50] {
        let mut config = NativeConfig::default();
        config.overlay.corner_radius_percent = accepted;
        assert!(
            config.validate().is_ok(),
            "corner radius {accepted} must be accepted"
        );
    }
    for rejected in [51_u8, 100, u8::MAX] {
        let mut config = NativeConfig::default();
        config.overlay.corner_radius_percent = rejected;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("overlay.corner_radius_percent"))
        ));
    }
}

#[test]
fn check_for_updates_interval_defaults_to_24_hours_and_accepts_whole_hours() {
    let default = NativeConfig::default();
    assert_eq!(
        default.updates.check_interval_hours,
        DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS
    );

    for accepted in [
        1,
        DEFAULT_CHECK_FOR_UPDATES_INTERVAL_HOURS,
        48,
        MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS,
    ] {
        let mut config = NativeConfig::default();
        config.updates.check_interval_hours = accepted;
        assert!(
            config.validate().is_ok(),
            "check interval {accepted} hours must be accepted"
        );
    }

    for rejected in [0, MAXIMUM_CHECK_FOR_UPDATES_INTERVAL_HOURS + 1] {
        let mut config = NativeConfig::default();
        config.updates.check_interval_hours = rejected;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("updates.check_interval_hours"))
        ));
    }
}

#[test]
fn logging_settings_use_the_closed_level_set_and_bounded_retention() {
    assert_eq!(LoggingConfig::default().level, LoggingLevel::Info);
    assert_eq!(
        LoggingConfig::default().retention_days,
        DEFAULT_LOG_RETENTION_DAYS
    );
    for level in LoggingLevel::ALL {
        let value = serde_json::to_value(level).expect("serialize logging level");
        assert_eq!(value, serde_json::Value::String(level.as_str().to_owned()));
        assert_eq!(
            serde_json::from_value::<LoggingLevel>(value).expect("deserialize logging level"),
            level
        );
    }

    for accepted in [1_u8, DEFAULT_LOG_RETENTION_DAYS, 30] {
        let mut config = NativeConfig::default();
        config.logging.retention_days = accepted;
        assert!(config.validate().is_ok(), "retention {accepted}");
    }
    for rejected in [0_u8, MAXIMUM_LOG_RETENTION_DAYS + 1, u8::MAX] {
        let mut config = NativeConfig::default();
        config.logging.retention_days = rejected;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("logging.retention_days"))
        ));
    }
}

#[test]
fn default_matches_the_shared_configuration_fixture() {
    assert!(!NativeConfig::default().updates.check_automatically);
    let fixture = include_str!("../../../../shared/config/fixtures/default.json");
    let expected: NativeConfig = serde_json::from_str(fixture).expect("shared fixture");
    assert_eq!(NativeConfig::default(), expected);

    let expected: serde_json::Value = serde_json::from_str(fixture).expect("fixture value");
    let actual = serde_json::to_value(NativeConfig::default()).expect("default value");
    assert_eq!(actual, expected);
}

#[test]
fn shared_configuration_fixture_manifest_matches_parser_contract() {
    #[derive(Deserialize)]
    struct FixtureManifest {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        cases: Vec<FixtureCase>,
    }

    #[derive(Deserialize)]
    struct FixtureCase {
        file: String,
        expected: String,
    }

    let manifest: FixtureManifest = serde_json::from_str(include_str!(
        "../../../../shared/config/fixtures/manifest.json"
    ))
    .expect("configuration fixture manifest");
    assert_eq!(manifest.schema_version, 1);

    for case in manifest.cases {
        let fixture = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../shared/config/fixtures")
                .join(&case.file),
        )
        .unwrap_or_else(|error| panic!("read fixture {}: {error}", case.file));
        let result = parse_config(&fixture);
        match case.expected.as_str() {
            "accept" => assert!(result.is_ok(), "fixture {} must be accepted", case.file),
            "reject" => assert!(result.is_err(), "fixture {} must be rejected", case.file),
            expected => panic!("fixture {} has unknown expectation {expected}", case.file),
        }
    }
}

#[test]
fn invalid_values_never_replace_last_valid_config() {
    let base = tempdir().expect("temp directory");
    let store = ConfigStore::new(StorageLayout::under(
        base.path(),
        BuildEnvironment::Production,
    ))
    .expect("config store");
    let config = store.load_or_default().expect("default config").config;
    let original = fs::read(&store.layout().config).expect("original bytes");

    let mut invalid = config.clone();
    invalid.overlay.opacity_percent = 0;
    assert!(matches!(
        store.commit(&invalid),
        Err(ConfigError::InvalidValue("overlay.opacity_percent"))
    ));
    assert_eq!(
        fs::read(&store.layout().config).expect("config bytes"),
        original
    );

    let mut invalid = config.clone();
    invalid.overlay.corner_radius_percent = 51;
    assert!(matches!(
        store.commit(&invalid),
        Err(ConfigError::InvalidValue("overlay.corner_radius_percent"))
    ));
    assert_eq!(
        fs::read(&store.layout().config).expect("config bytes"),
        original
    );

    let mut invalid = config.clone();
    invalid.overlay.hide_on_pointer_hover_delay_seconds = 61;
    assert!(matches!(
        store.commit(&invalid),
        Err(ConfigError::InvalidValue(
            "overlay.hide_on_pointer_hover_delay_seconds"
        ))
    ));
    assert_eq!(
        fs::read(&store.layout().config).expect("config bytes"),
        original
    );
}

#[test]
fn gamepad_dead_zones_must_be_finite_and_below_one() {
    for (stick, trigger, field) in [
        (-0.01, 0.0, "input.gamepad.stick_dead_zone"),
        (1.0, 0.0, "input.gamepad.stick_dead_zone"),
        (f64::NAN, 0.0, "input.gamepad.stick_dead_zone"),
        (0.15, -0.01, "input.gamepad.trigger_dead_zone"),
        (0.15, 1.0, "input.gamepad.trigger_dead_zone"),
        (0.15, f64::INFINITY, "input.gamepad.trigger_dead_zone"),
    ] {
        let mut config = NativeConfig::default();
        config.input.gamepad.stick_dead_zone = stick;
        config.input.gamepad.trigger_dead_zone = trigger;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue(actual)) if actual == field
        ));
    }
}

#[test]
fn selected_model_requires_a_complete_identity() {
    let mut config = NativeConfig::default();
    config.model.selected_model = Some(ModelIdentity {
        id: "standard".to_owned(),
        source: ModelSource::BuiltIn,
    });
    assert!(config.validate().is_ok());

    let mut invalid_id = config.clone();
    invalid_id
        .model
        .selected_model
        .as_mut()
        .expect("selection")
        .id = "   ".to_owned();
    assert!(matches!(
        invalid_id.validate(),
        Err(ConfigError::InvalidValue("model.selected_model.id"))
    ));

    let mut invalid_source = serde_json::to_value(&config).expect("config value");
    invalid_source["model"]["selected_model"]["source"] = serde_json::json!("installed");
    assert!(serde_json::from_value::<NativeConfig>(invalid_source).is_err());
}

#[test]
fn model_ids_are_portable_store_keys() {
    let mut config = NativeConfig::default();
    for invalid_id in [
        "../outside",
        "has space",
        "CON",
        "COM1",
        ".leading",
        "trailing.",
        "é",
    ] {
        config.model.selected_model = Some(ModelIdentity {
            id: invalid_id.to_owned(),
            source: ModelSource::Imported,
        });
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue("model.selected_model.id"))
        ));
    }

    let mut invalid_metadata = NativeConfig::default();
    invalid_metadata.model.imported_models.clear();
    invalid_metadata
        .model
        .imported_models
        .push(ImportedModelMetadata {
            id: "NUL".to_owned(),
            title: "Invalid".to_owned(),
            input_mode: ModelInputMode::Standard,
        });
    assert!(matches!(
        invalid_metadata.validate(),
        Err(ConfigError::InvalidValue("model.imported_models.id"))
    ));
}

#[test]
fn parser_requires_nullable_v1_fields_to_be_present() {
    let mut value = serde_json::to_value(NativeConfig::default()).expect("default value");
    value["model"]
        .as_object_mut()
        .expect("model object")
        .remove("selected_model");
    assert!(matches!(
        parse_config(&serde_json::to_vec(&value).expect("missing selected bytes")),
        Err(ConfigError::InvalidValue("model.selected_model"))
    ));
}
