//! The schema, its defaults, its bounds and its version gate.

use super::*;
use crate::{
    DEFAULT_HIDE_ON_IDLE_DELAY_SECONDS, MAXIMUM_MODEL_EXPRESSION_MEMORIES,
    MODEL_EXPRESSION_MEMORY_MAXIMUM_NAME_BYTES, ModelExpressionMemory,
};
use bongocat_input::ModifierKey;

#[test]
fn system_locale_resolves_to_a_shipped_language() {
    assert_eq!(
        Language::from_system_locale("zh-Hans-CN"),
        Language::ChineseSimplified
    );
    assert_eq!(
        Language::from_system_locale("zh_Hant_HK"),
        Language::ChineseTraditional
    );
    assert_eq!(Language::from_system_locale("en-GB"), Language::English);
    assert_eq!(Language::from_system_locale("de-DE"), Language::English);
    // One catalog serves every Arabic region, so the region subtag is
    // deliberately not part of the match: a machine in Saudi Arabia and one in
    // Egypt both report a locale the product reads from the same `ar-SA`
    // catalog. This is the RFC 4647 language-subtag fallback.
    for locale in ["ar", "ar-EG", "ar-SA", "ar_MA"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::Arabic,
            "{locale} is an Arabic locale"
        );
    }
    // Vietnamese takes the same shape: `vi` and `vi-VN` both reach the one
    // shipped catalog, as do the English variants that are not `en-US`.
    for locale in ["vi", "vi-VN", "vi_VN"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::Vietnamese,
            "{locale} is a Vietnamese locale"
        );
    }
    for locale in ["en", "en-GB", "en_AU", "en-CA"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::English,
            "{locale} is an English locale"
        );
    }
    // Portuguese takes the same shape: the copy was written for Brazil, and a
    // machine reporting plain `pt` or European `pt-PT` reaches it on the primary
    // subtag rather than falling back to English.
    for locale in ["pt", "pt-BR", "pt_BR", "pt-PT"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::Portuguese,
            "{locale} is a Portuguese locale"
        );
    }
    // Korean takes the same shape: the copy was written for South Korea, and a
    // machine reporting plain `ko` reaches it on the primary subtag rather than
    // falling back to English.
    for locale in ["ko", "ko-KR", "ko_KR", "ko-KP"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::Korean,
            "{locale} is a Korean locale"
        );
    }
    // Chinese shares the `zh` subtag across two scripts, so it is the one
    // language the primary subtag cannot classify. The script subtag decides
    // when the platform reports one, and the `TW`/`HK`/`MO` region subtags
    // decide when it does not; both spellings reach the `zh-TW` catalog rather
    // than reading `zh-CN` or dropping to English.
    for locale in ["zh-TW", "zh-HK", "zh-MO", "zh-Hant", "zh-Hant-HK"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::ChineseTraditional,
            "{locale} is Traditional Chinese"
        );
    }
    for locale in ["zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh_SG"] {
        assert_eq!(
            Language::from_system_locale(locale),
            Language::ChineseSimplified,
            "{locale} is Simplified Chinese"
        );
    }
    assert_eq!(
        Language::System.resolve(Language::ChineseSimplified),
        Language::ChineseSimplified
    );
    assert_eq!(
        Language::System.resolve(Language::ChineseTraditional),
        Language::ChineseTraditional
    );
    assert_eq!(Language::System.resolve(Language::Arabic), Language::Arabic);
    assert_eq!(
        Language::System.resolve(Language::Vietnamese),
        Language::Vietnamese
    );
    assert_eq!(
        Language::System.resolve(Language::Portuguese),
        Language::Portuguese
    );
    assert_eq!(Language::System.resolve(Language::Korean), Language::Korean);
    assert_eq!(
        Language::System.resolve(Language::System),
        Language::English
    );
    assert_eq!(
        Language::English.resolve(Language::ChineseSimplified),
        Language::English
    );
    assert_eq!(
        Language::ChineseSimplified.resolve(Language::English),
        Language::ChineseSimplified
    );
    // An explicit choice never follows the system, in either direction.
    assert_eq!(
        Language::English.resolve(Language::Arabic),
        Language::English
    );
    assert_eq!(
        Language::Arabic.resolve(Language::ChineseSimplified),
        Language::Arabic
    );
    assert_eq!(
        Language::Vietnamese.resolve(Language::ChineseSimplified),
        Language::Vietnamese
    );
    assert_eq!(
        Language::English.resolve(Language::Vietnamese),
        Language::English
    );
    assert_eq!(
        Language::Portuguese.resolve(Language::ChineseSimplified),
        Language::Portuguese
    );
    assert_eq!(
        Language::English.resolve(Language::Portuguese),
        Language::English
    );
    assert_eq!(
        Language::Korean.resolve(Language::ChineseSimplified),
        Language::Korean
    );
    assert_eq!(
        Language::English.resolve(Language::Korean),
        Language::English
    );
    assert_eq!(
        Language::ChineseTraditional.resolve(Language::ChineseSimplified),
        Language::ChineseTraditional
    );
    assert_eq!(
        Language::ChineseSimplified.resolve(Language::ChineseTraditional),
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

/// A document written before Arabic existed must still load and validate.
///
/// The enum only gained a variant, so every value an older build could write is
/// still one this build accepts, and a persisted configuration is not migrated
/// or rewritten by the upgrade. A document is read here verbatim rather than
/// round-tripped through `Default`, because serializing the current default
/// would prove nothing about what the old bytes contain.
#[test]
fn a_configuration_written_before_arabic_existed_still_loads() {
    let mut document = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    for language in ["system", "zh-CN", "en-US"] {
        document["appearance"]["language"] = serde_json::Value::String(language.to_owned());
        let written = serde_json::to_string(&document).expect("serialize document");
        let loaded: NativeConfig = serde_json::from_str(&written).expect("read old document");
        assert_eq!(loaded.appearance.language.code(), language);
        loaded.validate().expect("an old document still validates");
    }
}

/// A document written before the key layer could stack must still load, as the
/// compatibility mode.
///
/// `show_all_pressed_keys` carries `#[serde(default)]` for this reason: the
/// strict v1 entry point rejects anything it cannot read, so a field an older
/// build never wrote would otherwise send every existing `config.json` down the
/// backup-then-default recovery path and reset the user's settings. The document
/// is edited by removing the key from the *serialized* current default, which is
/// the shape the old bytes had.
#[test]
fn a_configuration_written_before_the_key_layer_could_stack_still_loads() {
    let mut document = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    let model = document["model"]
        .as_object_mut()
        .expect("model object")
        .remove("show_all_pressed_keys");
    assert!(
        model.is_some(),
        "the current default still writes the field"
    );
    let written = serde_json::to_string(&document).expect("serialize document");
    let loaded: NativeConfig = serde_json::from_str(&written).expect("read old document");
    assert!(
        !loaded.model.show_all_pressed_keys,
        "a document with no field loads as one image per hand, not as a new opt-in"
    );
    loaded.validate().expect("an old document still validates");
    assert!(parse_config(written.as_bytes()).is_ok());
}

/// A document written before the expression toggle existed must still load, as
/// the non-toggling behaviour.
///
/// The same reasoning as the key layer above, and the same shape of document: the
/// field is removed from the *serialized* current default, which is exactly what
/// the bytes of a build that predates the switch look like. Without the default
/// this build would refuse to read every existing `config.json`, fall back to a
/// default configuration, and reset the user's settings.
#[test]
fn a_configuration_written_before_the_expression_toggle_still_loads() {
    let mut document = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    let model = document["model"]
        .as_object_mut()
        .expect("model object")
        .remove("toggle_repeated_expression");
    assert!(
        model.is_some(),
        "the current default still writes the field"
    );
    let written = serde_json::to_string(&document).expect("serialize document");
    let loaded: NativeConfig = serde_json::from_str(&written).expect("read old document");
    assert!(
        !loaded.model.toggle_repeated_expression,
        "a document with no field loads as the repeat-is-a-no-op behaviour, not as a new opt-in"
    );
    loaded.validate().expect("an old document still validates");
    assert!(parse_config(written.as_bytes()).is_ok());
}

/// The toggle is its own switch rather than a second remembered-expression field.
///
/// Turning a repeated trigger into "turn this expression off" and restoring the
/// last chosen expression are different decisions, and one implementation detail
/// of the other: a repeat that closes an expression deliberately records nothing,
/// so what is remembered is still the last face the user chose to wear.
#[test]
fn the_expression_toggle_defaults_to_off_and_is_independent_of_the_memory_switch() {
    let default = NativeConfig::default();
    assert!(!default.model.toggle_repeated_expression);

    let mut config = default;
    config.model.remember_last_expression = true;
    assert!(
        !config.model.toggle_repeated_expression,
        "restoring the last expression says nothing about what a repeat trigger does"
    );
    config.model.toggle_repeated_expression = true;
    assert!(
        config.model.remember_last_expression,
        "toggling a repeat says nothing about where a model starts"
    );
    config
        .validate()
        .expect("both switches on is a valid document");
}

/// The pointer capture switch round-trips and an older document without the
/// namespace still loads on the shipped behaviour.
///
/// `input.mouse` is a whole new namespace rather than a new field, so the case
/// that matters is a document with no `mouse` object at all: the strict v1 entry
/// point rejects anything it cannot read, so without `#[serde(default)]` every
/// existing `config.json` would go down the backup-then-default recovery path.
#[test]
fn the_force_move_switch_round_trips_and_defaults_off_for_older_data() {
    assert!(!NativeConfig::default().input.mouse.force_move);

    let mut config = NativeConfig::default();
    config.input.mouse.force_move = true;
    let encoded = serde_json::to_string(&config).expect("serialize force move");
    let decoded: NativeConfig = serde_json::from_str(&encoded).expect("deserialize force move");
    assert!(decoded.input.mouse.force_move);
    decoded.validate().expect("the switch still validates");

    let mut document = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    let removed = document["input"]
        .as_object_mut()
        .expect("input object")
        .remove("mouse");
    assert!(
        removed.is_some(),
        "the current default still writes the namespace"
    );
    let written = serde_json::to_string(&document).expect("serialize document");
    let loaded: NativeConfig = serde_json::from_str(&written).expect("read old document");
    assert!(
        !loaded.input.mouse.force_move,
        "a document with no namespace loads on the absolute cursor, not as a new opt-in"
    );
    loaded.validate().expect("an old document still validates");
    assert!(parse_config(written.as_bytes()).is_ok());
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

    // A key release timeout was a fallback that released a still-held key once
    // a deadline passed. It is gone, and elapsed time is not a release, so the
    // whole keyboard namespace is unknown rather than ignored.
    let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    value["input"]["keyboard"] = serde_json::json!({ "release_fallback_timeout_ms": 500 });
    let error = serde_json::from_value::<NativeConfig>(value)
        .expect_err("the keyboard input namespace must not re-enter the config");
    assert!(error.to_string().contains("unknown field"));
    assert!(error.to_string().contains("keyboard"));
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
fn overlay_idle_hide_delay_accepts_the_first_version_range() {
    for accepted in [0_u32, 1, 10, 60, 599, 600] {
        let mut config = NativeConfig::default();
        config.overlay.hide_on_idle_delay_seconds = accepted;
        assert!(
            config.validate().is_ok(),
            "idle hide delay {accepted} must be accepted"
        );
    }
    for rejected in [601_u32, 1_200, u32::MAX] {
        let mut config = NativeConfig::default();
        config.overlay.hide_on_idle_delay_seconds = rejected;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue(
                "overlay.hide_on_idle_delay_seconds"
            ))
        ));
    }
}

#[test]
fn overlay_idle_hide_switch_defaults_to_off_and_round_trips() {
    let config = NativeConfig::default();
    assert!(!config.overlay.hide_on_idle);
    assert_eq!(
        config.overlay.hide_on_idle_delay_seconds,
        DEFAULT_HIDE_ON_IDLE_DELAY_SECONDS
    );

    let mut enabled = config;
    enabled.overlay.hide_on_idle = true;
    enabled.overlay.hide_on_idle_delay_seconds = 30;
    enabled.validate().expect("enabled idle hide is valid");
    let encoded = serde_json::to_string(&enabled).expect("serialize enabled idle hide");
    let decoded: NativeConfig =
        serde_json::from_str(&encoded).expect("deserialize enabled idle hide");
    assert_eq!(decoded, enabled);
}

#[test]
fn overlay_idle_hide_fields_default_when_missing_from_older_data() {
    let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    let overlay = value["overlay"]
        .as_object_mut()
        .expect("overlay is an object");
    overlay.remove("hide_on_idle");
    overlay.remove("hide_on_idle_delay_seconds");
    let decoded: NativeConfig =
        serde_json::from_value(value).expect("older config without idle hide fields must load");
    assert!(!decoded.overlay.hide_on_idle);
    assert_eq!(
        decoded.overlay.hide_on_idle_delay_seconds,
        DEFAULT_HIDE_ON_IDLE_DELAY_SECONDS
    );
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

/// The modifier is one physical key, so every one of the eight has to survive a
/// write/read cycle as itself — a document that came back naming a different key
/// would suspend the overlay for a key the user never chose.
#[test]
fn the_hold_modifier_stores_one_physical_key_and_defaults_to_none() {
    assert_eq!(
        NativeConfig::default().overlay.hold_modifier_to_interact,
        None
    );

    for modifier in ModifierKey::ALL {
        let mut config = NativeConfig::default();
        config.overlay.hold_modifier_to_interact = Some(modifier);
        config.validate().expect("a known modifier is valid");
        let encoded = serde_json::to_string(&config).expect("serialize the hold modifier");
        let decoded: NativeConfig =
            serde_json::from_str(&encoded).expect("deserialize the hold modifier");
        assert_eq!(decoded.overlay.hold_modifier_to_interact, Some(modifier));
    }
}

/// A configuration written before the field existed has to keep loading, and on
/// the side that changes nothing: the shipped default is "no key does this".
#[test]
fn the_hold_modifier_defaults_when_missing_from_older_data() {
    let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    let overlay = value["overlay"]
        .as_object_mut()
        .expect("overlay is an object");
    overlay.remove("hold_modifier_to_interact");
    let decoded: NativeConfig =
        serde_json::from_value(value).expect("older config without the hold modifier must load");
    assert_eq!(decoded.overlay.hold_modifier_to_interact, None);
}

/// The strict v1 parse is the only place an unknown name can be refused. There is
/// no fallback spelling and no silent "nearest modifier": the field would then
/// suspend the overlay for a key the document never named.
#[test]
fn an_unknown_hold_modifier_is_refused_by_the_strict_parse() {
    for refused in [
        "\"shift\"",
        "\"left\"",
        "\"left_shift_right\"",
        "\"Super\"",
        "4",
    ] {
        let mut value = serde_json::to_value(NativeConfig::default()).expect("serialize default");
        value["overlay"]["hold_modifier_to_interact"] =
            serde_json::from_str::<serde_json::Value>(refused).expect("a json literal");
        assert!(
            serde_json::from_value::<NativeConfig>(value.clone()).is_err(),
            "{refused} must not resolve to a modifier"
        );
    }
}

#[test]
fn random_behavior_settings_use_a_bounded_positive_interval() {
    let default = NativeConfig::default();
    assert_eq!(
        default.model.random_behavior.mode,
        RandomBehaviorMode::default(),
        "a configuration that never chose a mode must stay inert"
    );
    assert_eq!(default.model.random_behavior.mode, RandomBehaviorMode::Off);
    assert_eq!(
        default.model.random_behavior.interval_seconds,
        DEFAULT_RANDOM_BEHAVIOR_INTERVAL_SECONDS
    );
    for accepted in [
        MINIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
        30,
        MAXIMUM_RANDOM_BEHAVIOR_INTERVAL_SECONDS,
    ] {
        for mode in RandomBehaviorMode::ALL {
            let mut config = NativeConfig::default();
            config.model.random_behavior.mode = mode;
            config.model.random_behavior.interval_seconds = accepted;
            config
                .validate()
                .expect("random behavior interval should be accepted");
        }
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
fn every_random_behavior_mode_round_trips_through_the_document() {
    for mode in RandomBehaviorMode::ALL {
        let mut config = NativeConfig::default();
        config.model.random_behavior.mode = mode;
        let bytes = serde_json::to_vec(&config).expect("serialize random behavior mode");
        let text = String::from_utf8(bytes).expect("utf-8 document");
        let decoded: NativeConfig =
            serde_json::from_str(&text).expect("decode random behavior mode");
        assert_eq!(decoded.model.random_behavior.mode, mode);
        // The persisted spelling is the one the document documents, not a Rust
        // variant name, so a hand-edited file stays readable.
        assert!(text.contains(&format!("\"mode\":\"{}\"", mode.as_str())));
    }
}

#[test]
fn an_unknown_random_behavior_mode_is_rejected_rather_than_ignored() {
    let fixture =
        include_str!("../../../../shared/config/fixtures/invalid-random-behavior-mode.json");
    assert!(
        serde_json::from_str::<NativeConfig>(fixture).is_err(),
        "a mode outside the catalogue must not decode"
    );
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

/// A document written before behaviors could be named must still load, with every row
/// still showing its numbered label.
///
/// The field carries `#[serde(default)]` for the same reason the other optional model
/// fields do: the v1 entry point rejects anything it cannot read, so a required new
/// field would send every existing `config.json` down the backup-then-default recovery
/// path and reset the user's settings. The document is edited by removing the key from
/// the *serialized* current default, which is the shape the old bytes had.
#[test]
fn a_configuration_written_before_behaviors_could_be_named_still_loads() {
    let mut document = serde_json::to_value(NativeConfig::default()).expect("serialize default");
    let removed = document["model"]
        .as_object_mut()
        .expect("model object")
        .remove("behavior_names");
    assert!(
        removed.is_some(),
        "the current default still writes the field"
    );
    let written = serde_json::to_string(&document).expect("serialize document");
    let loaded: NativeConfig = serde_json::from_str(&written).expect("read old document");
    assert!(
        loaded.model.behavior_names.is_empty(),
        "a document with no field loads as every row numbered"
    );
    loaded.validate().expect("an old document still validates");
    assert!(parse_config(written.as_bytes()).is_ok());
}

/// A name is display text on one row, so it is bounded, printable and unique per
/// behavior — and the behavior it names has to be one the vocabulary can spell.
///
/// A row that carried an unprintable or unbounded name would be the page's problem, and
/// two rows for one behavior would leave the label up to which the parser saw last.
#[test]
fn a_behavior_name_is_parsed_bounded_and_held_once_per_behavior() {
    let mut config = NativeConfig::default();
    config.model.behavior_names = vec![
        ModelBehaviorName {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "motion:CAT_motion:0".to_owned(),
            name: "the sleepy one".to_owned(),
        },
        ModelBehaviorName {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            behavior_id: "expression:live2d_expression1.exp3.json".to_owned(),
            name: "开心".to_owned(),
        },
    ];
    config.validate().expect("well-formed names");

    for rejected in ["", "   ", "a\nb"] {
        let mut broken = config.clone();
        broken.model.behavior_names[0].name = rejected.to_owned();
        assert!(
            broken.validate().is_err(),
            "the name {rejected:?} must be rejected"
        );
    }
    let mut long = config.clone();
    long.model.behavior_names[0].name = "n".repeat(MODEL_BEHAVIOR_NAME_MAXIMUM_CHARS + 1);
    assert!(
        long.validate().is_err(),
        "a name longer than the field accepts must be rejected"
    );

    let mut wrong_behavior = config.clone();
    wrong_behavior.model.behavior_names[0].behavior_id = "not-a-behavior".to_owned();
    assert!(wrong_behavior.validate().is_err());

    let mut duplicate = config.clone();
    duplicate.model.behavior_names.push(ModelBehaviorName {
        model: ModelIdentity {
            id: "standard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: "motion:CAT_motion:0".to_owned(),
        name: "another name".to_owned(),
    });
    assert!(
        duplicate.validate().is_err(),
        "two rows for one behavior leave the label up to parse order"
    );

    // The same behavior of a *different* model is a different row, which is the whole
    // reason the model travels with the name.
    let mut per_model = config.clone();
    per_model.model.behavior_names.push(ModelBehaviorName {
        model: ModelIdentity {
            id: "keyboard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_id: "motion:CAT_motion:0".to_owned(),
        name: "keyboard's own".to_owned(),
    });
    per_model
        .validate()
        .expect("the same behavior of two models is two rows");
}

#[test]
fn remembered_expressions_default_to_off_and_hold_one_record_per_model() {
    let default = NativeConfig::default();
    assert!(!default.model.remember_last_expression);
    assert!(default.model.last_expressions.is_empty());
    default
        .validate()
        .expect("an unconfigured expression memory is valid");

    let mut config = NativeConfig::default();
    config.model.remember_last_expression = true;
    config.model.last_expressions = vec![
        ModelExpressionMemory {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            expression: "live2d_expression0.exp3.json".to_owned(),
        },
        // The same id from the other catalog is a different model, so it is a
        // second record rather than a duplicate.
        ModelExpressionMemory {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::Imported,
            },
            expression: "cat_exp0.exp3.json".to_owned(),
        },
    ];
    config
        .validate()
        .expect("one expression per model is valid");
    let encoded = serde_json::to_string(&config).expect("serialize the remembered expressions");
    let decoded: NativeConfig =
        serde_json::from_str(&encoded).expect("deserialize the remembered expressions");
    assert_eq!(decoded, config);

    // Two records for one model would leave the restored face up to which of
    // them the parser saw last, so the list has to reject the second.
    let mut duplicated = config.clone();
    duplicated
        .model
        .last_expressions
        .push(ModelExpressionMemory {
            model: ModelIdentity {
                id: "standard".to_owned(),
                source: ModelSource::BuiltIn,
            },
            expression: "live2d_expression1.exp3.json".to_owned(),
        });
    assert!(matches!(
        duplicated.validate(),
        Err(ConfigError::InvalidValue("model.last_expressions.model.id"))
    ));

    for rejected in ["", " ", "\t", "\n"] {
        let mut blank = config.clone();
        blank.model.last_expressions[0].expression = rejected.to_owned();
        assert!(
            matches!(
                blank.validate(),
                Err(ConfigError::InvalidValue(
                    "model.last_expressions.expression"
                ))
            ),
            "expression name {rejected:?} must be rejected"
        );
    }

    let mut control = config.clone();
    control.model.last_expressions[0].expression = "live2d_expression0\n.exp3.json".to_owned();
    assert!(matches!(
        control.validate(),
        Err(ConfigError::InvalidValue(
            "model.last_expressions.expression"
        ))
    ));

    let mut overlong = config.clone();
    overlong.model.last_expressions[0].expression =
        "e".repeat(MODEL_EXPRESSION_MEMORY_MAXIMUM_NAME_BYTES + 1);
    assert!(matches!(
        overlong.validate(),
        Err(ConfigError::InvalidValue(
            "model.last_expressions.expression"
        ))
    ));

    for rejected in ["", "..", "nested/model", "CON"] {
        let mut path_like = config.clone();
        path_like.model.last_expressions[0].model.id = rejected.to_owned();
        assert!(
            matches!(
                path_like.validate(),
                Err(ConfigError::InvalidValue("model.last_expressions.model.id"))
            ),
            "remembered model id {rejected:?} must be rejected"
        );
    }

    // The list is bounded, so a document cannot grow it without limit however
    // many distinct models it names.
    let mut oversized = config.clone();
    oversized.model.last_expressions = (0..=MAXIMUM_MODEL_EXPRESSION_MEMORIES)
        .map(|index| ModelExpressionMemory {
            model: ModelIdentity {
                id: format!("model-{index}"),
                source: ModelSource::Imported,
            },
            expression: "live2d_expression0.exp3.json".to_owned(),
        })
        .collect();
    assert!(matches!(
        oversized.validate(),
        Err(ConfigError::InvalidValue("model.last_expressions"))
    ));
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

#[test]
fn motion_overlap_defaults_off_for_existing_v1_documents_and_round_trips() {
    let old =
        include_bytes!("../../../../shared/config/fixtures/accept-before-motion-overlap.json");
    let (config, _) = parse_config(old).expect("existing v1 configuration");
    assert!(!config.model.allow_motion_overlap);
    assert!(!NativeConfig::default().model.allow_motion_overlap);
    let mut enabled = config;
    enabled.model.allow_motion_overlap = true;
    enabled.validate().expect("valid overlap setting");
    let bytes = serde_json::to_vec(&enabled).expect("serialize configuration");
    assert_eq!(parse_config(&bytes).expect("read configuration").0, enabled);
}
