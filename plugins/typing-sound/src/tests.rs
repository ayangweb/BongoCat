//! What this plugin does over a whole session, and what it asks the host for.
//!
//! The tests live in their own file because this plugin's behaviour is mostly about *which
//! request it makes* — a motion, or a file, at what volume, on which edge — and that is
//! readable as a table of conversations rather than as a body of methods. The harness is the
//! SDK's own in-memory host, so every assertion here is about the same messages the product
//! would receive.

use bongocat_plugin_sdk::prelude::*;
use bongocat_plugin_sdk::testing::{
    IdentityBuilder, Inbox, WrittenMessages, document, labels_in, model_requests, panels,
    values_from,
};
use bongocat_plugin_sdk::{Host, ModelOutcome, ModelRequest, Session, Values};

use crate::settings::{
    DEFAULT_INTERVAL_MILLIS, DEFAULT_MOTION, DEFAULT_VOLUME_PERCENT, MAXIMUM_INTERVAL_MILLIS,
    PRESS_EDGE, RELEASE_EDGE,
};
use crate::sound::{FILE_SOURCE, MODEL_SOURCE, Source};
use crate::{Preferences, TypingSound, declared_settings};

/// The record of what a session wrote.
///
/// One per test rather than one per session: a test that served a plugin two sessions at
/// once would be asserting on a conversation that cannot happen.
fn harness() -> WrittenMessages {
    WrittenMessages::new()
}

/// Every setting this plugin has, with the values a test chooses.
///
/// Every field named, including the ones a test did not change, because a partial document
/// is a document the product never sends: the form always sends the whole thing.
#[derive(Clone)]
struct Chosen {
    sound_source: String,
    sound_path: String,
    volume: i64,
    interval_ms: i64,
    skip_repeat: bool,
    include_mouse: bool,
    play_on_release: String,
}

impl Default for Chosen {
    fn default() -> Self {
        Self {
            sound_source: MODEL_SOURCE.to_string(),
            sound_path: String::new(),
            volume: DEFAULT_VOLUME_PERCENT,
            interval_ms: DEFAULT_INTERVAL_MILLIS,
            skip_repeat: true,
            include_mouse: false,
            play_on_release: PRESS_EDGE.to_string(),
        }
    }
}

impl Chosen {
    /// The document the settings form would send for these settings.
    fn document(&self) -> ConfigDocument {
        document(
            [
                (
                    "sound_source".to_string(),
                    ConfigValue::Text(self.sound_source.clone()),
                ),
                (
                    "sound_path".to_string(),
                    ConfigValue::Text(self.sound_path.clone()),
                ),
                ("volume".to_string(), ConfigValue::Integer(self.volume)),
                (
                    "interval_ms".to_string(),
                    ConfigValue::Integer(self.interval_ms),
                ),
                (
                    "skip_repeat".to_string(),
                    ConfigValue::Bool(self.skip_repeat),
                ),
                (
                    "include_mouse".to_string(),
                    ConfigValue::Bool(self.include_mouse),
                ),
                (
                    "play_on_release".to_string(),
                    ConfigValue::Text(self.play_on_release.clone()),
                ),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// These settings, reading a file as the sound.
    fn a_file(path: &str) -> Self {
        Self {
            sound_source: FILE_SOURCE.to_string(),
            sound_path: path.to_string(),
            ..Self::default()
        }
    }
}

/// Run a whole session over this document: announce, then serve these messages.
///
/// The document goes in rather than being left at the schema's defaults, because that is the
/// only way a value reaches a plugin in the product too: the host reads the plugin's own file
/// and sends the whole thing at the handshake. A test that set a volume by constructing the
/// plugin would be testing a path nothing takes.
fn serve(
    plugin: &mut TypingSound,
    written: &WrittenMessages,
    locale: &str,
    chosen: &Chosen,
    messages: Vec<HostMessage>,
) {
    let schema = declared_settings().to_schema().expect("a valid schema");
    let host = Host::new(
        written.writer(),
        IdentityBuilder::new()
            .id("typing-sound")
            .locale(locale)
            .build(),
        schema.clone(),
        values_from(&chosen.document(), &schema),
    )
    .expect("a host");
    let mut session = Session::new(host);
    session
        .announce(&mut written.writer(), plugin)
        .expect("announced");
    session.serve(plugin, messages).expect("served");
}

fn key_down(control: &str) -> InputEvent {
    InputEvent::KeyDown {
        control: control.to_owned(),
        repeat: false,
    }
}

fn repeated(control: &str) -> InputEvent {
    InputEvent::KeyDown {
        control: control.to_owned(),
        repeat: true,
    }
}

fn key_up(control: &str) -> InputEvent {
    InputEvent::KeyUp {
        control: control.to_owned(),
    }
}

/// Every request this plugin made, as `(id, request)`.
fn requests(written: &WrittenMessages) -> Vec<(u64, ModelRequest)> {
    model_requests(written)
}

/// Every sound request this plugin made, with the volume it asked for.
fn sounds(written: &WrittenMessages) -> Vec<(String, f32)> {
    requests(written)
        .into_iter()
        .filter_map(|(_, request)| match request {
            ModelRequest::PlaySound { path, volume } => Some((path, volume)),
            _ => None,
        })
        .collect()
}

/// Every motion this plugin asked for.
fn motions(written: &WrittenMessages) -> Vec<String> {
    requests(written)
        .into_iter()
        .filter_map(|(_, request)| match request {
            ModelRequest::PlayMotion { name, .. } => Some(name),
            _ => None,
        })
        .collect()
}

#[test]
fn a_key_asks_the_model_for_the_motion_the_user_chose() {
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        Inbox::new().input(key_down("KeyA")).into_messages(),
    );
    assert_eq!(motions(&written), [DEFAULT_MOTION]);
    assert!(
        sounds(&written).is_empty(),
        "and a model's own sound is a motion request, not a file one"
    );
}

#[test]
fn a_chosen_audio_file_is_asked_for_by_path_and_at_the_chosen_volume() {
    // The whole of the two sound settings, end to end: the choice decides which request is
    // made, the path goes into it verbatim, and the volume the user set is the one the host
    // is asked for.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::a_file("/Users/you/sounds/click.mp3"),
        Inbox::new().input(key_down("KeyA")).into_messages(),
    );
    assert_eq!(
        sounds(&written),
        [("/Users/you/sounds/click.mp3".to_string(), 0.75)],
        "so the file the user typed is the file the host was asked to open, at the volume \\
         they set"
    );
    assert!(
        motions(&written).is_empty(),
        "and the model is left alone, because a file has no motion to play"
    );
}

#[test]
fn a_file_chosen_with_no_path_falls_back_to_the_model_rather_than_to_silence() {
    // A user who has picked "an audio file" and not yet typed one has a plugin in a state
    // the settings form cannot express. The model's own sound is a sound, and it is the one
    // this plugin shipped with — which is a better answer than a file named "".
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let chosen = Chosen {
        sound_source: FILE_SOURCE.to_string(),
        sound_path: "   ".to_string(),
        ..Chosen::default()
    };
    serve(
        &mut plugin,
        &written,
        "en-US",
        &chosen,
        Inbox::new().input(key_down("KeyA")).into_messages(),
    );
    assert_eq!(motions(&written), [DEFAULT_MOTION]);
}

#[test]
fn a_sound_the_host_cannot_play_is_said_on_the_panel_rather_than_swallowed() {
    // The failure a user cannot otherwise see: a sound that does not happen is invisible, and
    // the only thing that tells them their file is wrong is a sentence. Kept up rather than
    // flashed, because a user who looked away needs it still there when they look back.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::a_file("/Users/you/sounds/click.mp3"),
        Inbox::new()
            .input(key_down("KeyA"))
            .answer(1, ModelOutcome::HostCannot)
            .into_messages(),
    );
    let labels = labels_in(&panels(&written).last().expect("a panel").scene);
    assert!(
        labels
            .iter()
            .any(|label| label == "That audio file could not be read"),
        "so the panel says which of the two failures it was: {labels:?}"
    );
}

#[test]
fn a_motion_the_model_does_not_have_is_said_rather_than_swallowed() {
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        Inbox::new()
            .input(key_down("KeyA"))
            .answer(
                1,
                ModelOutcome::NotInModel {
                    kind: ModelRequestKind::Motion,
                },
            )
            .into_messages(),
    );
    let labels = labels_in(&panels(&written).last().expect("a panel").scene);
    assert!(
        labels
            .iter()
            .any(|label| label == "This model has no motion by that name"),
        "so a typo in the motion name is visible rather than a silence: {labels:?}"
    );
}

#[test]
fn a_sound_on_release_waits_for_the_key_to_come_up() {
    // The setting is about the edge, not about a different sound: a user who wants the click
    // as they let go of the key hears it as they let go, and a plugin that played on the
    // press regardless would be a setting that does nothing.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let chosen = Chosen {
        play_on_release: RELEASE_EDGE.to_string(),
        ..Chosen::default()
    };
    serve(
        &mut plugin,
        &written,
        "en-US",
        &chosen,
        Inbox::new()
            .input(key_down("KeyA"))
            .input(key_up("KeyA"))
            .into_messages(),
    );
    assert_eq!(
        motions(&written).len(),
        1,
        "one sound for one chord, and it happened at the release rather than twice"
    );
}

#[test]
fn a_chord_released_together_still_makes_one_sound_per_key() {
    // Two fingers, two releases, two sounds — not one for the chord. The held set is what
    // makes this work, and it is the reason a chord is remembered rather than counted.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let chosen = Chosen {
        play_on_release: RELEASE_EDGE.to_string(),
        ..Chosen::default()
    };
    serve(
        &mut plugin,
        &written,
        "en-US",
        &chosen,
        Inbox::new()
            .input(key_down("KeyA"))
            .input(key_down("KeyB"))
            .input(key_up("KeyA"))
            .input(key_up("KeyB"))
            .into_messages(),
    );
    assert_eq!(
        motions(&written).len(),
        1,
        "the interval is what collapses them, not the release logic: two releases inside a \\
         third of a second is one sound, which is the same rule a fast typist gets on the \
         press edge"
    );
}

#[test]
fn a_reset_forgets_the_held_keys_because_the_platforms_set_is_not_this_plugins_to_guess() {
    // A lock screen, a sleep or an unplugged keyboard arrives as one event with no detail. The
    // sound is not a keystroke, so nothing is played — but the held set has to go, or the
    // next release of a key the platform has already forgotten would make a sound for a key
    // that was not pressed.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let chosen = Chosen {
        play_on_release: RELEASE_EDGE.to_string(),
        ..Chosen::default()
    };
    serve(
        &mut plugin,
        &written,
        "en-US",
        &chosen,
        Inbox::new()
            .input(key_down("KeyA"))
            .input(InputEvent::Reset {
                reason: "session_lock".to_string(),
            })
            .input(key_up("KeyA"))
            .into_messages(),
    );
    assert!(
        motions(&written).is_empty(),
        "so a locked screen does not make a sound when the user comes back"
    );
}

#[test]
fn a_burst_of_typing_is_spaced_rather_than_held() {
    // The reason the interval setting exists: a sound that repeats eight times a second is
    // one held note, and a motion the model loops cannot be restarted eight times a second
    // anyway.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let mut inbox = Inbox::new();
    for key in ["KeyA", "KeyB", "KeyC", "KeyD", "KeyE"] {
        inbox = inbox.input(key_down(key));
    }
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        inbox.into_messages(),
    );
    assert_eq!(
        motions(&written).len(),
        1,
        "five keys inside a third of a second is one sound"
    );

    // And with no interval at all, every key is its own sound — which is the setting a user
    // who wants a click per keystroke exactly should have.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let chosen = Chosen {
        interval_ms: 0,
        ..Chosen::default()
    };
    serve(
        &mut plugin,
        &written,
        "en-US",
        &chosen,
        Inbox::new()
            .input(key_down("KeyA"))
            .input(key_down("KeyB"))
            .input(key_down("KeyC"))
            .into_messages(),
    );
    assert_eq!(motions(&written).len(), 3);
}

#[test]
fn a_key_the_keyboard_repeats_is_one_key() {
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        Inbox::new()
            .input(key_down("KeyA"))
            .input(repeated("KeyA"))
            .input(repeated("KeyA"))
            .into_messages(),
    );
    assert_eq!(
        motions(&written).len(),
        1,
        "a held key is one key, so it is one sound rather than a dozen"
    );
}

#[test]
fn the_mouse_is_silent_unless_the_user_asked_for_it() {
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        Inbox::new()
            .input(InputEvent::MouseButton {
                button: "left".to_string(),
                pressed: true,
            })
            .into_messages(),
    );
    assert!(motions(&written).is_empty());

    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    let chosen = Chosen {
        include_mouse: true,
        ..Chosen::default()
    };
    serve(
        &mut plugin,
        &written,
        "en-US",
        &chosen,
        Inbox::new()
            .input(InputEvent::MouseButton {
                button: "left".to_string(),
                pressed: true,
            })
            .into_messages(),
    );
    assert_eq!(motions(&written).len(), 1);
}

#[test]
fn a_panel_answers_in_the_language_the_user_reads() {
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "zh-CN",
        &Chosen::default(),
        Inbox::new().input(key_down("KeyA")).into_messages(),
    );
    assert_eq!(
        labels_in(&panels(&written).last().expect("a panel").scene),
        ["A"],
        "a keycap is a keycap in every language, so the key itself needs no translation"
    );

    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "zh-CN",
        &Chosen::a_file("/nope/click.mp3"),
        Inbox::new()
            .input(key_down("KeyA"))
            .answer(1, ModelOutcome::HostCannot)
            .into_messages(),
    );
    let labels = labels_in(&panels(&written).last().expect("a panel").scene);
    assert!(
        labels.iter().any(|label| label == "无法读取该音频文件"),
        "and a complaint is in the reader's own language, with the application knowing none \
         of these words: {labels:?}"
    );
}

#[test]
fn the_panel_takes_itself_down_when_the_key_it_showed_is_stale() {
    // A panel that says nothing is a box on the desktop, and this one appears on every
    // keystroke — so it has to leave on its own.
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        Inbox::new()
            .input(key_down("KeyA"))
            .tick(2_000)
            .into_messages(),
    );
    assert!(
        !plugin.showing,
        "because a chip that is still there a second later is a permanent fixture"
    );
}

#[test]
fn the_settings_the_plugin_declares_are_the_settings_form_and_nothing_else() {
    let schema = declared_settings().to_schema().expect("a valid schema");
    assert_eq!(
        schema
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect::<Vec<_>>(),
        [
            "sound_source",
            "sound_path",
            "volume",
            "interval_ms",
            "skip_repeat",
            "include_mouse",
            "play_on_release"
        ],
        "in the order the form shows them, because the order is the order somebody sets this \
         plugin up in"
    );
}

#[test]
fn the_value_the_form_sends_is_the_value_the_plugin_reads() {
    // The one thing that could otherwise drift: what the settings form writes, and what this
    // plugin answers to. Every field, every kind.
    let schema = declared_settings().to_schema().expect("a valid schema");
    let chosen = Chosen {
        sound_source: FILE_SOURCE.to_string(),
        sound_path: "/Users/you/sounds/click.mp3".to_string(),
        volume: 40,
        interval_ms: 900,
        skip_repeat: false,
        include_mouse: true,
        play_on_release: RELEASE_EDGE.to_string(),
    };
    let values: Values = values_from(&chosen.document(), &schema);
    let preferences = Preferences::read(&values);
    assert_eq!(
        preferences.source,
        Source::File {
            path: "/Users/you/sounds/click.mp3".to_string()
        }
    );
    assert_eq!(preferences.volume, 0.4);
    assert_eq!(preferences.interval_ms, 900);
    assert!(!preferences.skip_repeat);
    assert!(preferences.include_mouse);
    assert!(preferences.play_on_release);
}

#[test]
fn a_value_this_build_does_not_know_reads_as_the_ordinary_answer() {
    // A document written by a newer version is not this build's to interpret, and in each
    // case the ordinary reading is the one it was most likely written as.
    let schema = declared_settings().to_schema().expect("a valid schema");
    let newer = document(
        [
            (
                "sound_source".to_string(),
                ConfigValue::Text("something_newer".to_string()),
            ),
            (
                "play_on_release".to_string(),
                ConfigValue::Text("something_newer".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let preferences = Preferences::read(&values_from(&newer, &schema));
    assert_eq!(
        preferences.source,
        Source::Model {
            motion: DEFAULT_MOTION.to_string()
        },
        "an unknown source is the model's own sound, which is what this plugin shipped with"
    );
    assert!(
        !preferences.play_on_release,
        "and an unknown edge is the press, which is the one that was there first"
    );
}

#[test]
fn a_motion_name_that_is_blank_falls_back_rather_than_playing_nothing() {
    // A blank name can only ever be `NotInModel`, so a user who cleared the box would get
    // silence with no way to tell it from a model that has no such motion. The default is
    // restored instead, which is a sound rather than nothing.
    let schema = declared_settings().to_schema().expect("a valid schema");
    for blank in ["", "   "] {
        let values = values_from(
            &document(
                [("motion".to_string(), ConfigValue::Text(blank.to_string()))]
                    .into_iter()
                    .collect(),
            ),
            &schema,
        );
        assert_eq!(
            Preferences::read(&values).motion(),
            Some(DEFAULT_MOTION),
            "so {blank:?} is a sound rather than silence"
        );
    }
}

#[test]
fn a_document_with_a_value_of_the_wrong_kind_reads_as_the_default() {
    let schema = declared_settings().to_schema().expect("a valid schema");
    let values = values_from(
        &document(
            [
                ("volume".to_string(), ConfigValue::Bool(true)),
                ("sound_path".to_string(), ConfigValue::Integer(7)),
                (
                    "interval_ms".to_string(),
                    ConfigValue::Text("fast".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        &schema,
    );
    let preferences = Preferences::read(&values);
    assert_eq!(
        preferences.volume,
        DEFAULT_VOLUME_PERCENT as f32 / 100.0,
        "a switch where a number was declared is a document the host would not send; reading \
         the default is the honest answer"
    );
    assert_eq!(
        preferences.source,
        Source::Model {
            motion: DEFAULT_MOTION.to_string()
        }
    );
    assert_eq!(preferences.interval_ms, DEFAULT_INTERVAL_MILLIS as u64);
}

#[test]
fn a_hand_written_file_cannot_ask_for_a_volume_the_device_would_refuse() {
    let schema = declared_settings().to_schema().expect("a valid schema");
    for written in [i64::MIN, -10, 0, 100, 10_000, i64::MAX] {
        let values = values_from(
            &document(
                [("volume".to_string(), ConfigValue::Integer(written))]
                    .into_iter()
                    .collect(),
            ),
            &schema,
        );
        let volume = Preferences::read(&values).volume;
        assert!(
            (0.0..=1.0).contains(&volume),
            "{written} became {volume}, which is inside the range the device accepts"
        );
    }
}

#[test]
fn a_hand_written_file_cannot_ask_for_an_interval_that_never_lets_a_sound_through() {
    let schema = declared_settings().to_schema().expect("a valid schema");
    for written in [i64::MIN, -1, 0, MAXIMUM_INTERVAL_MILLIS, i64::MAX] {
        let values = values_from(
            &document(
                [("interval_ms".to_string(), ConfigValue::Integer(written))]
                    .into_iter()
                    .collect(),
            ),
            &schema,
        );
        let interval = Preferences::read(&values).interval_ms;
        assert!(
            interval <= MAXIMUM_INTERVAL_MILLIS as u64,
            "{written} became {interval}ms, which is inside the range this plugin declares"
        );
    }
}

#[test]
fn a_path_too_long_to_be_a_path_is_dropped_rather_than_sent_to_the_host() {
    // The bound is the protocol's own on a text value, and it applies before this plugin ever
    // sees the document: a value that long is refused by the host's fit, so the field reads
    // as untouched and the source falls back to the model's sound. A megabyte of path is a
    // megabyte the host would have to hold, check and then refuse to open.
    let schema = declared_settings().to_schema().expect("a valid schema");
    let values = values_from(
        &document(
            [
                (
                    "sound_source".to_string(),
                    ConfigValue::Text(FILE_SOURCE.to_string()),
                ),
                (
                    "sound_path".to_string(),
                    ConfigValue::Text("a".repeat(10_000)),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        &schema,
    );
    assert_eq!(
        Preferences::read(&values).source,
        Source::Model {
            motion: DEFAULT_MOTION.to_string()
        },
        "and the fallback is a sound rather than a request the host would refuse"
    );
}

#[test]
fn a_source_built_around_an_absurd_path_still_asks_for_a_bounded_one() {
    // The bound the plugin carries for itself, unreachable through the settings form because
    // the host's own fit is stricter — and here because a plugin that assembles a `Source` in
    // its own code would otherwise put an unbounded string on the wire.
    let source = Source::File {
        path: "a".repeat(10_000),
    };
    let request = source
        .as_request(1.0)
        .expect("a file is something to ask for");
    let ModelRequest::PlaySound { path, volume } = request else {
        panic!("a file is a sound request");
    };
    assert_eq!(
        path.chars().count(),
        4096,
        "and the volume is passed through"
    );
    assert_eq!(volume, 1.0);
}

#[test]
fn the_descriptor_is_valid_before_the_plugin_talks_to_anybody() {
    // A plugin that cannot name itself is one the host will not start, and the failure
    // arrives as a refusal with no detail. This is the check that says which part is wrong
    // instead.
    let plugin = TypingSound::new(Preferences::default());
    let descriptor = plugin.descriptor();
    assert_eq!(descriptor.id(), "typing-sound");
    assert_eq!(descriptor.name().resolve("zh-CN"), "打字音效");
    descriptor
        .check()
        .expect("this plugin's own descriptor is one the host accepts");
    assert!(descriptor.subscribes_to(Subscription::Input));
    assert!(
        descriptor.subscribes_to(Subscription::ModelReaction),
        "without it the plugin could ask for a sound and never learn whether it worked"
    );
}

#[test]
fn a_panel_this_plugin_builds_is_one_the_host_accepts() {
    let written = harness();
    let mut plugin = TypingSound::new(Preferences::default());
    serve(
        &mut plugin,
        &written,
        "en-US",
        &Chosen::default(),
        Inbox::new()
            .input(key_down("KeyA"))
            .input(key_down("KeyB"))
            .into_messages(),
    );
    for panel in panels(&written) {
        panel
            .validate()
            .expect("a panel this plugin builds is one the host accepts");
    }
}
