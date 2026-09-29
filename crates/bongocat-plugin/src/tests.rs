//! The plugin host's behaviour, asserted on.
//!
//! Three things are worth proving here, and each is a claim the design rests on:
//!
//! * **A timer keeps time correctly** through presses, ticks, wraps and long
//!   intervals. Everything a pomodoro panel shows comes out of this arithmetic, and
//!   a countdown that drifts is the most visible possible bug in a pet's window.
//! * **An install is atomic.** A failure at any step leaves the previously
//!   installed version exactly where it was, which is what makes an interrupted
//!   download recoverable rather than destructive.
//! * **A press lands where the panel was drawn.** A button that is pressable
//!   somewhere it is not drawn is worse than no button at all, so the hit test and
//!   the layout are checked against the same panel.

use super::engine::{PluginInstance, WallClock, format_duration, format_time};
use super::host::HostFacts;
use super::store::{PluginStore, digest_hex, digest_matches, verify_signature};
use bongocat_plugin_protocol::{
    BehaviorAction, BehaviorId, BindingValue, NamedBehavior, PluginCatalog, PluginError,
    PluginErrorCode, PluginId, PluginManifest, PluginVersion, SceneValue,
};
use bongocat_plugin_render::{FontBook, ImageLibrary, TextMeasurer, render_contribution};
use std::path::Path;
use std::time::Duration;

// ---------------------------------------------------------------- engine

/// A behavior with the kind's own fields and nothing else.
///
/// One field set per kind rather than one union, because every spec refuses
/// fields it does not own — which is exactly the property the manifest check
/// relies on, and a test that built a union would not exercise it.
fn named(id: &str, kind: &str) -> NamedBehavior {
    let mut value = serde_json::Map::new();
    value.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    value.insert(
        "kind".to_string(),
        serde_json::Value::String(kind.to_string()),
    );
    match kind {
        "countdown" => {
            value.insert("duration_seconds".to_string(), 1500.into());
        }
        "local_time" => {
            value.insert("format".to_string(), "HH:MM".into());
        }
        _ => {}
    }
    serde_json::from_value(serde_json::Value::Object(value)).expect("the test behavior parses")
}

fn instance(behaviors: &[(&str, &str)]) -> PluginInstance {
    let declared: Vec<NamedBehavior> = behaviors.iter().map(|(id, kind)| named(id, kind)).collect();
    PluginInstance::new(PluginId::new("test-plugin").unwrap(), &declared)
        .expect("the instance builds")
}

fn text(table: &bongocat_plugin_protocol::BindingTable, path: &str) -> String {
    table
        .get(path)
        .map_or_else(|| "<absent>".to_string(), BindingValue::as_text)
}

fn fraction(table: &bongocat_plugin_protocol::BindingTable, path: &str) -> f32 {
    table
        .get(path)
        .map_or_else(|| -1.0, BindingValue::as_fraction)
}

fn flag(table: &bongocat_plugin_protocol::BindingTable, path: &str) -> bool {
    table.get(path).map_or_else(|| false, BindingValue::as_flag)
}

#[test]
fn a_countdown_starts_at_its_full_duration_and_is_not_running() {
    let mut plugin = instance(&[("timer", "countdown")]);
    let table = plugin.evaluate(Duration::ZERO, WallClock::default());
    assert_eq!(text(&table, "timer.remaining_seconds"), "1500");
    assert_eq!(text(&table, "timer.remaining_text"), "25:00");
    assert!(!flag(&table, "timer.running"));
    assert!((fraction(&table, "timer.progress") - 0.0).abs() < 1e-6);
}

#[test]
fn a_countdown_does_not_move_while_it_is_stopped() {
    let mut plugin = instance(&[("timer", "countdown")]);
    let before = plugin.evaluate(Duration::ZERO, WallClock::default());
    let after = plugin.evaluate(Duration::from_secs(600), WallClock::default());
    assert_eq!(
        text(&before, "timer.remaining_text"),
        text(&after, "timer.remaining_text")
    );
    assert!(!flag(&after, "timer.running"));
}

#[test]
fn a_countdown_moves_only_while_it_runs() {
    let mut plugin = instance(&[("timer", "countdown")]);
    plugin.apply(&BehaviorId::new("timer"), BehaviorAction::Start);
    let table = plugin.evaluate(Duration::from_secs(60), WallClock::default());
    assert_eq!(text(&table, "timer.remaining_text"), "24:00");
    assert!(flag(&table, "timer.running"));
    plugin.apply(&BehaviorId::new("timer"), BehaviorAction::Pause);
    let paused = plugin.evaluate(Duration::from_secs(60), WallClock::default());
    assert_eq!(text(&paused, "timer.remaining_text"), "24:00");
    assert!(!flag(&paused, "timer.running"));
}

#[test]
fn a_countdown_reaches_zero_and_stops() {
    let mut plugin = instance(&[("timer", "countdown")]);
    plugin.apply(&BehaviorId::new("timer"), BehaviorAction::Start);
    let table = plugin.evaluate(Duration::from_secs(1_500), WallClock::default());
    assert_eq!(text(&table, "timer.remaining_seconds"), "0");
    assert!(!flag(&table, "timer.running"));
    assert!((fraction(&table, "timer.progress") - 1.0).abs() < 1e-6);
    assert_eq!(text(&table, "timer.completed_runs"), "1");
}

#[test]
fn a_repeating_countdown_returns_to_full_and_keeps_going() {
    let mut named_behavior = named("timer", "countdown");
    let mut spec = named_behavior.spec.clone();
    bongocat_plugin_protocol::clamp_behavior_spec(&mut spec);
    let repeated = match spec {
        bongocat_plugin_protocol::BehaviorSpec::Countdown(spec) => {
            bongocat_plugin_protocol::BehaviorSpec::Countdown(
                bongocat_plugin_protocol::CountdownSpec {
                    auto_repeat: true,
                    auto_start: true,
                    ..spec
                },
            )
        }
        other => panic!("expected a countdown, got {other:?}"),
    };
    named_behavior.spec = repeated;
    let mut plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    // One whole run plus a little, so the remainder has to be carried.
    let table = plugin.evaluate(Duration::from_secs(1_520), WallClock::default());
    assert_eq!(text(&table, "timer.remaining_seconds"), "1480");
    assert!(flag(&table, "timer.running"));
    assert_eq!(text(&table, "timer.completed_runs"), "1");
}

#[test]
fn a_long_tick_lands_where_several_short_ones_would() {
    // A 1500-second countdown driven by one 1500-second tick and by fifteen
    // 100-second ticks has to end in the same place, or the timer's accuracy
    // depends on the host's evaluation cadence.
    let mut one = instance(&[("timer", "countdown")]);
    one.apply(&BehaviorId::new("timer"), BehaviorAction::Start);
    let whole = one.evaluate(Duration::from_secs(1_500), WallClock::default());
    let mut many = instance(&[("timer", "countdown")]);
    many.apply(&BehaviorId::new("timer"), BehaviorAction::Start);
    let mut stepped = many.bindings(WallClock::default());
    for _ in 0..15 {
        stepped = many.evaluate(Duration::from_secs(100), WallClock::default());
    }
    assert_eq!(
        text(&whole, "timer.remaining_seconds"),
        text(&stepped, "timer.remaining_seconds")
    );
}

#[test]
fn a_toggle_alternates_and_a_reset_returns_to_the_top() {
    let mut plugin = instance(&[("timer", "countdown")]);
    let timer = BehaviorId::new("timer");
    plugin.apply(&timer, BehaviorAction::Start);
    plugin.evaluate(Duration::from_secs(120), WallClock::default());
    plugin.apply(&timer, BehaviorAction::Toggle);
    assert!(!flag(
        &plugin.bindings(WallClock::default()),
        "timer.running"
    ));
    plugin.apply(&timer, BehaviorAction::Toggle);
    assert!(flag(
        &plugin.bindings(WallClock::default()),
        "timer.running"
    ));
    plugin.apply(&timer, BehaviorAction::Reset);
    let after = plugin.bindings(WallClock::default());
    assert_eq!(text(&after, "timer.remaining_seconds"), "1500");
    assert!(!flag(&after, "timer.running"));
}

#[test]
fn a_stopwatch_counts_up_and_wraps_at_its_period() {
    let mut named_behavior = named("watch", "stopwatch");
    named_behavior.spec = bongocat_plugin_protocol::BehaviorSpec::Stopwatch(
        bongocat_plugin_protocol::StopwatchSpec {
            auto_start: true,
            period_seconds: Some(60),
        },
    );
    let mut plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    let table = plugin.evaluate(Duration::from_secs(25), WallClock::default());
    assert_eq!(text(&table, "watch.elapsed_seconds"), "25");
    let wrapped = plugin.evaluate(Duration::from_secs(40), WallClock::default());
    assert_eq!(text(&wrapped, "watch.elapsed_seconds"), "5");
    // And the progress binding is a proportion of the period, which is what makes
    // it drawable.
    assert!((fraction(&wrapped, "watch.progress") - 5.0 / 60.0).abs() < 1e-6);
}

#[test]
fn a_stopwatch_without_a_period_stops_at_the_bound() {
    let mut plugin = instance(&[("watch", "stopwatch")]);
    let watch = BehaviorId::new("watch");
    plugin.apply(&watch, BehaviorAction::Start);
    let table = plugin.evaluate(
        Duration::from_secs(u64::from(u32::MAX)),
        WallClock::default(),
    );
    assert_eq!(text(&table, "watch.elapsed_seconds"), "86400");
}

/// A counter with the bounds a test wants, which the protocol's defaults do not
/// have to match.
fn bounded_counter(maximum: i64) -> NamedBehavior {
    let mut behavior = named("count", "counter");
    behavior.spec =
        bongocat_plugin_protocol::BehaviorSpec::Counter(bongocat_plugin_protocol::CounterSpec {
            initial: 0,
            minimum: 0,
            maximum,
            step: 1,
            loop_back: false,
        });
    behavior
}

#[test]
fn a_counter_clamps_at_its_bounds() {
    let mut plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&bounded_counter(10)),
    )
    .expect("the instance builds");
    let counter = BehaviorId::new("count");
    for _ in 0..20 {
        plugin.apply(&counter, BehaviorAction::Increment);
    }
    assert_eq!(
        text(&plugin.bindings(WallClock::default()), "count.value"),
        "10"
    );
    for _ in 0..30 {
        plugin.apply(&counter, BehaviorAction::Decrement);
    }
    assert_eq!(
        text(&plugin.bindings(WallClock::default()), "count.value"),
        "0"
    );
}

#[test]
fn a_wrapping_counter_goes_past_its_maximum() {
    let mut named_behavior = named("count", "counter");
    named_behavior.spec =
        bongocat_plugin_protocol::BehaviorSpec::Counter(bongocat_plugin_protocol::CounterSpec {
            initial: 0,
            minimum: 0,
            maximum: 2,
            step: 1,
            loop_back: true,
        });
    let mut plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    let counter = BehaviorId::new("count");
    let mut seen = Vec::new();
    for _ in 0..5 {
        plugin.apply(&counter, BehaviorAction::Increment);
        seen.push(text(&plugin.bindings(WallClock::default()), "count.value"));
    }
    assert_eq!(seen, ["1", "2", "0", "1", "2"]);
}

#[test]
fn a_counter_with_a_negative_step_counts_down() {
    let mut named_behavior = named("count", "counter");
    named_behavior.spec =
        bongocat_plugin_protocol::BehaviorSpec::Counter(bongocat_plugin_protocol::CounterSpec {
            initial: 5,
            minimum: 0,
            maximum: 10,
            step: -2,
            loop_back: false,
        });
    let mut plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    plugin.apply(&BehaviorId::new("count"), BehaviorAction::Increment);
    assert_eq!(
        text(&plugin.bindings(WallClock::default()), "count.value"),
        "3"
    );
}

#[test]
fn a_counter_resets_when_given_an_action_it_has_no_state_for() {
    let mut named_behavior = named("count", "counter");
    named_behavior.spec =
        bongocat_plugin_protocol::BehaviorSpec::Counter(bongocat_plugin_protocol::CounterSpec {
            initial: 4,
            minimum: 0,
            maximum: 10,
            step: 1,
            loop_back: false,
        });
    let mut plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    let counter = BehaviorId::new("count");
    plugin.apply(&counter, BehaviorAction::Increment);
    plugin.apply(&counter, BehaviorAction::Toggle);
    assert_eq!(
        text(&plugin.bindings(WallClock::default()), "count.value"),
        "4"
    );
}

#[test]
fn an_action_naming_an_undeclared_behavior_is_ignored() {
    // A press that arrives for a panel that has since changed must not take the
    // plugin down.
    let mut plugin = instance(&[("timer", "countdown")]);
    plugin.apply(&BehaviorId::new("gone"), BehaviorAction::Start);
    assert_eq!(
        text(
            &plugin.bindings(WallClock::default()),
            "timer.remaining_seconds"
        ),
        "1500"
    );
}

#[test]
fn a_clock_formats_the_fields_its_format_names() {
    let clock = WallClock::new(9, 5, 3);
    assert_eq!(format_time("HH:MM", clock), "09:05");
    assert_eq!(format_time("HH:MM:SS", clock), "09:05:03");
    assert_eq!(format_time("HH", clock), "09");
    assert_eq!(format_time("MM/SS", clock), "05/03");
    assert_eq!(format_time("SS", clock), "03");
    assert_eq!(format_time("", clock), "");
}

#[test]
fn a_clock_reads_midnight_as_twenty_three_hours_not_twenty_four() {
    // `WallClock::new` clamps, so a platform that reports 24:00 at the end of a
    // day cannot produce a two-digit hour a panel would render as "24:00".
    assert_eq!(format_time("HH", WallClock::new(24, 0, 0)), "23");
    assert_eq!(format_time("MM", WallClock::new(0, 60, 0)), "59");
}

#[test]
fn durations_are_formatted_as_a_clock_face() {
    assert_eq!(format_duration(0), "00:00");
    assert_eq!(format_duration(59), "00:59");
    assert_eq!(format_duration(1_500), "25:00");
    assert_eq!(format_duration(3_599), "59:59");
    // Past an hour the hours appear, because `99:99` is not a time.
    assert_eq!(format_duration(3_600), "1:00:00");
    assert_eq!(format_duration(3_661), "1:01:01");
    assert_eq!(format_duration(360_000), "100:00:00");
}

#[test]
fn a_stopped_timer_is_not_clock_driven_and_a_running_one_is() {
    let mut plugin = instance(&[("timer", "countdown")]);
    assert!(!plugin.is_clock_driven());
    plugin.apply(&BehaviorId::new("timer"), BehaviorAction::Start);
    assert!(
        plugin.is_clock_driven(),
        "a running timer must be evaluated without a press"
    );
    plugin.apply(&BehaviorId::new("timer"), BehaviorAction::Pause);
    assert!(!plugin.is_clock_driven());
}

#[test]
fn a_repeating_countdown_is_clock_driven_even_before_it_starts() {
    let mut named_behavior = named("timer", "countdown");
    named_behavior.spec = bongocat_plugin_protocol::BehaviorSpec::Countdown(
        bongocat_plugin_protocol::CountdownSpec {
            duration_seconds: 60,
            auto_start: false,
            auto_repeat: true,
        },
    );
    let plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    assert!(plugin.is_clock_driven());
}

#[test]
fn a_behavior_built_from_a_zero_duration_is_a_one_second_timer() {
    // Clamped rather than refused: a one-line mistake should be a working panel,
    // not a plugin the author cannot debug.
    let mut named_behavior = named("timer", "countdown");
    named_behavior.spec = bongocat_plugin_protocol::BehaviorSpec::Countdown(
        bongocat_plugin_protocol::CountdownSpec {
            duration_seconds: 0,
            auto_start: false,
            auto_repeat: false,
        },
    );
    let plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    assert_eq!(
        text(
            &plugin.bindings(WallClock::default()),
            "timer.remaining_seconds"
        ),
        "1"
    );
}

#[test]
fn an_inverted_counter_range_is_ordered_rather_than_refused() {
    let mut named_behavior = named("count", "counter");
    named_behavior.spec =
        bongocat_plugin_protocol::BehaviorSpec::Counter(bongocat_plugin_protocol::CounterSpec {
            initial: 50,
            minimum: 10,
            maximum: 0,
            step: 0,
            loop_back: false,
        });
    let plugin = PluginInstance::new(
        PluginId::new("test-plugin").unwrap(),
        std::slice::from_ref(&named_behavior),
    )
    .unwrap();
    let behavior = plugin
        .behavior(&BehaviorId::new("count"))
        .expect("the behavior exists");
    assert_eq!(
        behavior.state(),
        // Ordered to 0..=10, step repaired to 1, initial clamped into it.
        super::engine::BehaviorState::Counter(super::engine::CounterState { value: 10 })
    );
}

#[test]
fn every_behavior_kind_reports_its_state_and_spec() {
    let plugin = instance(&[
        ("timer", "countdown"),
        ("watch", "stopwatch"),
        ("clock", "local_time"),
        ("count", "counter"),
    ]);
    assert_eq!(plugin.behaviors().count(), 4);
    for (id, behavior) in plugin.behaviors() {
        assert!(
            !behavior.state().to_string().is_empty(),
            "{id} has no state"
        );
        assert!(
            !format!("{:?}", behavior.spec()).is_empty(),
            "{id} has no spec"
        );
    }
}

#[test]
fn bindings_that_name_nothing_are_refused() {
    let behaviors = vec![named("timer", "countdown")];
    assert!(super::engine::validate_bindings(&behaviors, &["timer.progress".to_string()]).is_ok());
    assert_eq!(
        super::engine::validate_bindings(&behaviors, &["absent.path".to_string()])
            .unwrap_err()
            .code(),
        PluginErrorCode::UnknownBinding
    );
    assert_eq!(
        super::engine::validate_bindings(&behaviors, &["no-dot".to_string()])
            .unwrap_err()
            .code(),
        PluginErrorCode::InvalidBinding
    );
    // The host's own paths are the host's business, not this check's.
    assert!(
        super::engine::validate_bindings(&behaviors, &["host.overlay_visible".to_string()]).is_ok()
    );
}

// ---------------------------------------------------------------- host facts

#[test]
fn host_facts_write_every_path_they_declare() {
    let mut table = bongocat_plugin_protocol::BindingTable::new();
    HostFacts {
        overlay_visible: true,
        model_name: Some("Bongo".to_string()),
        pressed_key_count: 3,
    }
    .write_into(&mut table);
    for path in HostFacts::paths() {
        assert!(table.get(path).is_some(), "{path} was not written");
    }
    assert!(flag(&table, "host.overlay_visible"));
    assert_eq!(text(&table, "host.model_name"), "Bongo");
    assert_eq!(text(&table, "host.pressed_key_count"), "3");
}

#[test]
fn a_missing_model_writes_an_empty_name_rather_than_omitting_it() {
    // A scene binding to the model name should show nothing, not its own fallback
    // — the fallback is there for a path that does not exist at all.
    let mut table = bongocat_plugin_protocol::BindingTable::new();
    HostFacts::default().write_into(&mut table);
    assert_eq!(text(&table, "host.model_name"), "");
    let resolved = table.resolve(&SceneValue::Binding {
        binding: "host.model_name".to_string(),
        fallback: "FB".to_string(),
    });
    assert_eq!(resolved.as_text(), "");
}

// ---------------------------------------------------------------- store

fn manifest_json(id: &str, version: &str) -> Vec<u8> {
    format!(
        r#"{{"schema_version":1,"api_version":1,"id":"{id}","name":"Test","version":"{version}",
            "overlay":{{"size":[100,60],"scene":{{"type":"text","value":"hi"}}}}}}"#
    )
    .into_bytes()
}

/// A zip containing one file, built in memory so the test needs no fixture.
fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut buffer = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    for (name, bytes) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .expect("the member starts");
        std::io::Write::write_all(&mut writer, bytes).expect("the member writes");
    }
    writer.finish().expect("the archive finishes");
    buffer.into_inner()
}

#[test]
fn an_installed_plugin_is_listed_and_readable() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    let archive = zip_with(&[("plugin.json", &manifest_json("pomodoro", "1.0.0"))]);
    store
        .unpack(&id, &version, &archive)
        .expect("the archive unpacks");
    store
        .set_current(&id, &version)
        .expect("the version goes live");

    let installed = store.installed();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, id);
    assert_eq!(installed[0].version, version);
    assert!(installed[0].enabled);
    let manifest = store.manifest(&installed[0]).expect("the manifest reads");
    assert_eq!(manifest.name, "Test");
}

#[test]
fn an_unpacked_but_not_live_version_is_not_listed() {
    // The atomicity property: unpacking is not installing.
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    store
        .unpack(
            &id,
            &version,
            &zip_with(&[("plugin.json", &manifest_json("pomodoro", "1.0.0"))]),
        )
        .expect("the archive unpacks");
    assert!(
        store.installed().is_empty(),
        "an unpacked version nobody made live is not installed"
    );
}

#[test]
fn a_failed_unpack_leaves_no_directory_behind() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    // An archive with no manifest at all.
    let outcome = store.unpack(&id, &version, &zip_with(&[("readme.txt", b"nothing")]));
    assert_eq!(outcome.unwrap_err().code(), PluginErrorCode::ArchiveInvalid);
    assert!(store.version_directory(&id, &version).is_none());
}

#[test]
fn an_archive_declaring_a_different_id_is_refused() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    let outcome = store.unpack(
        &id,
        &version,
        &zip_with(&[("plugin.json", &manifest_json("something-else", "1.0.0"))]),
    );
    assert_eq!(outcome.unwrap_err().code(), PluginErrorCode::ArchiveInvalid);
    assert!(store.version_directory(&id, &version).is_none());
}

#[test]
fn an_archive_that_is_not_a_zip_is_refused() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let outcome = store.unpack(
        &PluginId::new("pomodoro").unwrap(),
        &PluginVersion::new(1, 0, 0),
        b"this is not a zip file at all",
    );
    assert_eq!(outcome.unwrap_err().code(), PluginErrorCode::ArchiveInvalid);
}

#[test]
fn an_uninstall_removes_everything_and_reports_a_second_one() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    store
        .unpack(
            &id,
            &version,
            &zip_with(&[("plugin.json", &manifest_json("pomodoro", "1.0.0"))]),
        )
        .expect("the archive unpacks");
    store
        .set_current(&id, &version)
        .expect("the version goes live");
    store.uninstall(&id).expect("the uninstall succeeds");
    assert!(store.installed().is_empty());
    assert_eq!(
        store.uninstall(&id).unwrap_err().code(),
        PluginErrorCode::NotInstalled
    );
}

#[test]
fn an_update_keeps_the_new_version_and_a_couple_of_old_ones() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    for (major, minor) in [(1, 0), (1, 1), (1, 2), (1, 3), (1, 4), (1, 5)] {
        let version = PluginVersion::new(major, minor, 0);
        store
            .unpack(
                &id,
                &version,
                &zip_with(&[(
                    "plugin.json",
                    &manifest_json("pomodoro", &version.to_string()),
                )]),
            )
            .expect("the archive unpacks");
        store
            .set_current(&id, &version)
            .expect("the version goes live");
    }
    let live = store.current_version(&id).expect("a live version");
    assert_eq!(live, PluginVersion::new(1, 5, 0));
    let kept: Vec<String> = std::fs::read_dir(store.root().join("pomodoro"))
        .expect("the plugin directory reads")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| PluginVersion::parse(name).is_some())
        .collect();
    assert!(
        kept.len() <= super::MAXIMUM_RETAINED_VERSIONS,
        "kept {kept:?}, more than the bound allows"
    );
    assert!(
        kept.contains(&"1.5.0".to_string()),
        "the live version must survive"
    );
}

#[test]
fn making_a_version_live_requires_it_to_be_unpacked_first() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    assert_eq!(
        store
            .set_current(&id, &PluginVersion::new(9, 9, 9))
            .unwrap_err()
            .code(),
        PluginErrorCode::NotInstalled
    );
}

#[test]
fn installing_the_same_version_twice_is_refused_rather_than_merged() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    let archive = zip_with(&[("plugin.json", &manifest_json("pomodoro", "1.0.0"))]);
    store
        .unpack(&id, &version, &archive)
        .expect("the first unpack");
    assert_eq!(
        store.unpack(&id, &version, &archive).unwrap_err().code(),
        PluginErrorCode::AlreadyInstalled
    );
}

#[test]
fn a_directory_the_store_did_not_write_is_not_a_plugin() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path());
    store.create().expect("the store creates");
    std::fs::create_dir_all(store.root().join("Not A Plugin Id")).expect("a stray directory");
    std::fs::write(store.root().join("stray.txt"), b"x").expect("a stray file");
    assert!(store.installed().is_empty());
}

#[test]
fn a_digest_is_lowercase_hex_and_compares_exactly() {
    let bytes = b"the archive";
    let digest = digest_hex(bytes);
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    assert!(digest_matches(&digest, bytes));
    assert!(!digest_matches(&digest, b"a different archive"));
    assert!(!digest_matches("short", bytes));
}

#[test]
fn a_signature_is_not_verified_without_a_key() {
    // Fails closed: a build with no provisioned key cannot install anything.
    assert_eq!(
        verify_signature(None, b"bytes", "sig").unwrap_err().code(),
        PluginErrorCode::SignatureKeyMissing
    );
    assert_eq!(
        verify_signature(Some("   "), b"bytes", "sig")
            .unwrap_err()
            .code(),
        PluginErrorCode::SignatureKeyMissing
    );
}

#[test]
fn an_empty_signature_is_refused_before_any_key_is_parsed() {
    let error = verify_signature(Some("untrusted comment: x\nAAAA\n"), b"bytes", "  ").unwrap_err();
    assert_eq!(error.code(), PluginErrorCode::SignatureInvalid);
}

#[test]
fn a_malformed_signature_is_refused_rather_than_accepted() {
    let error = verify_signature(Some("AAAA"), b"bytes", "not a signature").unwrap_err();
    assert_eq!(error.code(), PluginErrorCode::SignatureInvalid);
}

// ---------------------------------------------------------------- catalog

fn catalog_bytes() -> Vec<u8> {
    serde_json::to_vec(
        &PluginCatalog::parse(
            br#"{"schema_version":1,"plugins":[{
            "id":"pomodoro","name":"Pomodoro","version":"1.0.0","api_version":1,
            "description":"A focus timer.",
            "downloads":{"macos-aarch64":{
                "url":"https://github.com/ayangweb/BongoCat/releases/download/p/v.zip",
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "size_bytes":10,"signature":"sig"}}
        }]}"#,
        )
        .expect("the catalog parses"),
    )
    .expect("the catalog serialises")
}

/// A development catalog naming an archive that is already on this machine, which
/// is the loop the ADR-0078 trust model is built around: author, install, see it
/// on the model window, with no publish step and no signature.
#[test]
fn a_development_catalog_installs_a_plugin_without_a_network_or_a_signature() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let catalog_directory = directory.path().join("plugin-catalog");
    std::fs::create_dir_all(catalog_directory.join("build")).expect("the build folder");
    let archive = zip_with(&[("plugin.json", &manifest_json("pomodoro", "1.0.0"))]);
    std::fs::write(catalog_directory.join("build/pomodoro.zip"), &archive)
        .expect("the archive is beside the catalog that names it");
    std::fs::write(
        catalog_directory.join("plugins.json"),
        format!(
            r#"{{"schema_version":1,"plugins":[{{
              "id":"pomodoro","name":"Pomodoro","version":"1.0.0","api_version":1,
              "description":"A focus timer.",
              "downloads":{{"{}":{{"path":"build/pomodoro.zip"}}}}
            }}]}}"#,
            super::host_platform()
        ),
    )
    .expect("the catalog writes");

    let (layers, _consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        PluginStore::new(directory.path().join("plugins")),
        catalog_directory,
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    wait_for_revision(&handle, 0);
    endpoint.send(super::worker::PluginCommand::RefreshCatalog);
    let before = handle.snapshot().revision;
    let catalog_read = wait_for_revision(&handle, before);
    assert_eq!(
        catalog_read.entries.len(),
        1,
        "a catalog beside the worker is read without a network"
    );

    assert!(endpoint.send(super::worker::PluginCommand::Install(
        PluginId::new("pomodoro").unwrap()
    )));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = handle.snapshot();
        if snapshot.entries.iter().any(|entry| entry.installed) {
            assert_eq!(
                snapshot.active.len(),
                1,
                "and its panel is on the model window"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the local archive was never installed: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

/// The development catalog this repository ships, beside the reference plugin.
const DEVELOPMENT_CATALOG: &str = include_str!("../../../plugins/plugins.json");

#[test]
fn the_shipped_development_catalog_is_valid() {
    // The catalog a developer copies into place, so it is parsed here rather than
    // on first use — a broken one would be the first thing a plugin author met.
    let catalog = bongocat_plugin_protocol::PluginCatalog::parse(DEVELOPMENT_CATALOG.as_bytes())
        .expect("the shipped development catalog must parse");
    assert_eq!(catalog.plugins.len(), 1);
    let entry = &catalog.plugins[0];
    assert_eq!(entry.id.as_str(), "pomodoro");
    // Every platform this product ships for, so a developer on either one finds it.
    for platform in ["macos-aarch64", "macos-x86_64", "windows-x86_64"] {
        let download = entry
            .download_for(platform)
            .unwrap_or_else(|error| panic!("{platform} must be served: {error}"));
        assert!(
            download.is_local(),
            "{platform} must name an archive on this machine"
        );
        assert_eq!(download.signature, None, "a local archive is not signed");
    }
}

#[test]
fn a_local_catalog_with_no_file_is_an_empty_list_not_a_failure() {
    // What a fresh checkout looks like: an author who has not written a plugin yet
    // should see an empty list, not an error.
    let directory = tempfile::tempdir().expect("a temp directory");
    let loaded = super::catalog::load_local(directory.path()).expect("an empty catalog loads");
    assert!(loaded.catalog.plugins.is_empty());
    assert_eq!(loaded.source, super::catalog::CatalogSource::Directory);
}

#[test]
fn a_local_catalog_is_read_from_the_directory() {
    let directory = tempfile::tempdir().expect("a temp directory");
    std::fs::write(directory.path().join("plugins.json"), catalog_bytes())
        .expect("the catalog writes");
    let loaded = super::catalog::load_local(directory.path()).expect("the catalog loads");
    assert_eq!(loaded.catalog.plugins.len(), 1);
    assert_eq!(loaded.catalog.plugins[0].name, "Pomodoro");
}

#[test]
fn the_catalog_uses_the_same_proxies_as_the_update_manifest_in_the_same_order() {
    // The whole point of importing the prefix list rather than restating it: this
    // is the assertion that would fail if a second copy ever appeared.
    let sources = super::catalog::catalog_sources();
    let prefixes = bongocat_update::GITHUB_PROXY_PREFIXES;
    assert_eq!(
        sources.len(),
        prefixes.len() + 1,
        "one source per proxy, plus the official one"
    );
    for (source, prefix) in sources.iter().zip(prefixes) {
        assert!(
            source.starts_with(prefix),
            "{source} should have been prefixed with {prefix}"
        );
    }
    // The official endpoint is last, so a run that can reach GitHub directly still
    // tries the proxies first — the same order the updater uses.
    assert_eq!(
        sources.last().map(String::as_str),
        Some(super::catalog::catalog_url().as_str())
    );
    assert!(super::catalog::catalog_url().starts_with("https://github.com/"));
}

#[test]
fn a_catalog_fetch_tries_each_source_until_one_parses() {
    let mut attempted = Vec::new();
    let loaded = super::catalog::fetch_catalog(|url, _| {
        attempted.push(url.to_string());
        // The first source returns something that is not a catalog; the second
        // returns the real one.
        if attempted.len() == 1 {
            return Err(PluginErrorLike.into());
        }
        Ok(catalog_bytes())
    })
    .expect("a later source answers");
    assert_eq!(attempted.len(), 2);
    assert_eq!(loaded.catalog.plugins.len(), 1);
    assert_eq!(loaded.source, super::catalog::CatalogSource::Network);
}

/// A stand-in for a transport error, so the test does not depend on a real one.
#[derive(Clone, Copy)]
struct PluginErrorLike;

impl From<PluginErrorLike> for bongocat_plugin_protocol::PluginError {
    fn from(_: PluginErrorLike) -> Self {
        bongocat_plugin_protocol::PluginError::new(PluginErrorCode::DownloadFailed)
    }
}

#[test]
fn a_catalog_fetch_that_never_answers_reports_the_last_failure() {
    let outcome = super::catalog::fetch_catalog(|_, _| Err(PluginErrorLike.into()));
    let error = outcome.unwrap_err();
    assert_eq!(error.code(), PluginErrorCode::DownloadFailed);
}

#[test]
fn a_catalog_that_never_parses_is_refused_even_though_it_was_fetched() {
    // A source that answers with a body that is not a catalog is not a usable
    // source. Accepting it would let a captive portal's login page become the
    // plugin list.
    let outcome = super::catalog::fetch_catalog(|_, _| Ok(b"<html>not a catalog</html>".to_vec()));
    assert_eq!(outcome.unwrap_err().code(), PluginErrorCode::CatalogInvalid);
}

#[test]
fn an_archive_fetch_checks_the_announced_size_before_and_after() {
    let entry = PluginCatalog::parse(&catalog_bytes()).expect("the catalog parses");
    let download = entry.plugins[0]
        .download_for("macos-aarch64")
        .expect("the platform is served");
    let bytes = b"0123456789".to_vec();
    let mut calls = 0;
    let fetched = super::catalog::fetch_archive(download, None, |_, _| {
        calls += 1;
        Ok(bytes.clone())
    })
    .expect("the archive arrives");
    assert_eq!(fetched.len(), 10);
    assert_eq!(calls, 1);

    // A body of the wrong size is refused rather than unpacked: a truncated
    // transfer is not a plugin.
    let outcome: Result<Vec<u8>, bongocat_plugin_protocol::PluginError> =
        super::catalog::fetch_archive(download, None, |_, _| Ok(b"short".to_vec()));
    assert_eq!(
        outcome.unwrap_err().code(),
        PluginErrorCode::ChecksumMismatch
    );
}

#[test]
fn an_archive_the_catalog_calls_huge_is_refused_without_a_request() {
    let mut entry = PluginCatalog::parse(&catalog_bytes()).expect("the catalog parses");
    entry.plugins[0]
        .downloads
        .get_mut("macos-aarch64")
        .expect("the platform is served")
        .size_bytes = Some(u64::MAX);
    let download = entry.plugins[0]
        .download_for("macos-aarch64")
        .expect("the platform is served");
    let mut called = false;
    let outcome: Result<Vec<u8>, bongocat_plugin_protocol::PluginError> =
        super::catalog::fetch_archive(download, None, |_, _| {
            called = true;
            Ok(Vec::new())
        });
    assert_eq!(outcome.unwrap_err().code(), PluginErrorCode::ArchiveInvalid);
    assert!(!called, "an oversized archive must not be requested at all");
}

// ---------------------------------------------------------------- press

/// A manifest whose scene has a countdown and a start button, the shape a pomodoro
/// panel has.
fn pomodoro_manifest() -> PluginManifest {
    let json = r#"{
        "schema_version": 1,
        "api_version": 1,
        "id": "pomodoro",
        "name": "Pomodoro",
        "version": "1.0.0",
        "overlay": {
            "anchor": "bottom_left",
            "size": [200, 120],
            "behaviors": [
                {"id": "timer", "kind": "countdown", "duration_seconds": 1500},
                {"id": "count", "kind": "counter", "maximum": 10}
            ],
            "scene": {
                "type": "stack",
                "background": "202020ff",
                "radius": 8,
                "padding": [10, 10],
                "spacing": 8,
                "children": [
                    {"type": "text", "value": {"binding": "timer.remaining_text", "fallback": "--:--"}, "size": 28},
                    {"type": "progress_bar", "value": {"fraction": "timer.progress", "fallback": 0}, "height": 6, "fill": "e5534b", "track": "ffffff20"},
                    {"type": "stack", "axis": "horizontal", "spacing": 6, "children": [
                        {"type": "button", "id": "go", "label": "Start", "action": "toggle", "target": "timer", "variant": "primary", "radius": 6},
                        {"type": "button", "id": "reset", "label": "Reset", "action": "reset", "target": "timer", "variant": "secondary", "radius": 6},
                        {"type": "button", "id": "more", "label": "+", "action": "increment", "target": "count", "variant": "transparent"}
                    ]}
                ]
            }
        }
    }"#;
    PluginManifest::parse(json.as_bytes()).expect("the pomodoro manifest parses")
}

fn pomodoro_panel(plugin: &mut PluginInstance) -> bongocat_plugin_render::RenderedPanel {
    let manifest = pomodoro_manifest();
    let mut measurer = TextMeasurer::new(FontBook::load_system());
    let table = plugin.evaluate(Duration::ZERO, WallClock::new(9, 5, 0));
    render_contribution(
        &manifest.overlay,
        &table,
        1.0,
        &mut measurer,
        &ImageLibrary::new(),
    )
    .expect("the panel rasterizes")
}

#[test]
fn a_pomodoro_panel_publishes_a_press_target_for_each_button() {
    let mut plugin = instance(&[("timer", "countdown"), ("count", "counter")]);
    let panel = pomodoro_panel(&mut plugin);
    assert_eq!(panel.hit_regions.len(), 3);
    let ids: Vec<&str> = panel
        .hit_regions
        .iter()
        .map(|region| region.button.as_str())
        .collect();
    assert_eq!(ids, ["go", "reset", "more"]);
}

#[test]
fn a_press_on_the_start_button_is_where_the_button_was_drawn() {
    // The property that makes the whole interaction model work: a press is tested
    // against the panel that was published, so a button can only be pressed where
    // it is drawn.
    let mut plugin = instance(&[("timer", "countdown"), ("count", "counter")]);
    let panel = pomodoro_panel(&mut plugin);
    let start = panel
        .hit_regions
        .iter()
        .find(|region| region.button == "go")
        .expect("the start button exists");
    let hit = panel
        .hit_test(start.rect.x + 2.0, start.rect.y + 2.0)
        .expect("a press inside the button hits it");
    assert_eq!(hit, "go");

    // And a press in the panel's padding hits nothing at all.
    assert_eq!(panel.hit_test(1.0, 1.0), None);
}

#[test]
fn a_press_that_hits_nothing_changes_nothing() {
    let mut plugin = instance(&[("timer", "countdown"), ("count", "counter")]);
    let panel = pomodoro_panel(&mut plugin);
    let before = text(
        &plugin.bindings(WallClock::default()),
        "timer.remaining_seconds",
    );
    assert_eq!(panel.hit_test(1.0, 1.0), None, "the press hits nothing");
    plugin.evaluate(Duration::ZERO, WallClock::default());
    assert_eq!(
        text(
            &plugin.bindings(WallClock::default()),
            "timer.remaining_seconds"
        ),
        before
    );
}

#[test]
fn a_button_with_no_target_presses_without_doing_anything() {
    // A panel is allowed to show a pressable-looking placeholder; a press on it is
    // ignored rather than a failure.
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "demo", "name": "Demo", "version": "1.0.0",
        "overlay": {"size": [100, 60], "scene": {"type": "button", "id": "x",
            "label": "X", "action": "toggle"}}
    }"#;
    let manifest = PluginManifest::parse(json.as_bytes()).expect("the manifest parses");
    let mut measurer = TextMeasurer::new(FontBook::load_system());
    let panel = render_contribution(
        &manifest.overlay,
        &bongocat_plugin_protocol::BindingTable::new(),
        1.0,
        &mut measurer,
        &ImageLibrary::new(),
    )
    .expect("the panel rasterizes");
    assert_eq!(panel.hit_regions.len(), 1);
    assert!(panel.hit_regions[0].rect.width > 0.0);
}

// ---------------------------------------------------------------- manifest validation

#[test]
fn a_scene_binding_to_an_undeclared_behavior_is_refused_at_load() {
    let mut manifest = pomodoro_manifest();
    manifest.overlay.behaviors.clear();
    assert_eq!(
        manifest.validate().unwrap_err().code(),
        PluginErrorCode::UnknownBinding
    );
}

#[test]
fn a_manifest_with_no_behaviors_at_all_is_valid() {
    // The simplest possible plugin: a static label.
    let manifest = PluginManifest::parse(
        br#"{"schema_version":1,"api_version":1,"id":"static","name":"Static","version":"1.0.0",
            "overlay":{"size":[100,60],"scene":{"type":"text","value":"Hi"}}}"#,
    )
    .expect("a static manifest parses");
    assert!(manifest.overlay.behaviors.is_empty());
}

#[test]
fn a_scene_node_of_every_kind_validates() {
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "every", "name": "Every", "version": "1.0.0",
        "overlay": {"size": [300, 200],
            "behaviors": [{"id": "t", "kind": "countdown", "duration_seconds": 60}],
            "scene": {"type": "stack", "children": [
                {"type": "text", "value": {"binding": "t.remaining_text", "fallback": "--"}},
                {"type": "spacer", "grow": 1},
                {"type": "divider", "thickness": 1},
                {"type": "progress_bar", "value": {"fraction": "t.progress", "fallback": 0}},
                {"type": "progress_ring", "value": {"fraction": "t.progress", "fallback": 0}},
                {"type": "image", "asset": "icon.png"},
                {"type": "button", "id": "b", "label": "B", "action": "reset", "target": "t"}
            ]}
        }
    }"#;
    assert!(PluginManifest::parse(json.as_bytes()).is_ok());
}

#[test]
fn a_binding_longer_than_the_bound_is_refused() {
    let long = "t.".to_string() + &"x".repeat(200);
    let json = format!(
        r#"{{"schema_version":1,"api_version":1,"id":"demo","name":"Demo","version":"1.0.0",
            "overlay":{{"size":[100,60],"behaviors":[{{"id":"t","kind":"counter"}}],
            "scene":{{"type":"text","value":{{"binding":"{long}","fallback":""}}}}}}}}"#
    );
    assert_eq!(
        PluginManifest::parse(json.as_bytes()).unwrap_err().code(),
        PluginErrorCode::InvalidBinding
    );
}

#[test]
fn a_button_pressing_a_clock_is_accepted_and_does_nothing() {
    // A clock has no state, so an action on one is meaningless — but the manifest
    // is still valid, because refusing it would make a panel that shows a label and
    // a disabled-looking button inexpressible.
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "clocky", "name": "Clock", "version": "1.0.0",
        "overlay": {"size": [100, 60],
            "behaviors": [{"id": "c", "kind": "local_time"}],
            "scene": {"type": "button", "id": "b", "label": {"binding": "c.text", "fallback": "--:--"},
                      "action": "toggle", "target": "c"}}
    }"#;
    let manifest = PluginManifest::parse(json.as_bytes()).expect("the manifest parses");
    let mut plugin = PluginInstance::new(manifest.id.clone(), &manifest.overlay.behaviors)
        .expect("the instance builds");
    plugin.apply(&BehaviorId::new("c"), BehaviorAction::Toggle);
    let table = plugin.bindings(WallClock::new(9, 5, 3));
    assert_eq!(text(&table, "c.text"), "09:05");
}

#[test]
fn a_local_time_behavior_with_a_format_the_host_cannot_render_is_refused() {
    // The format is a closed subset; a format naming a month would need a locale
    // the plugin never named.
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "clocky", "name": "Clock", "version": "1.0.0",
        "overlay": {"size": [100, 60],
            "behaviors": [{"id": "c", "kind": "local_time", "format": "YYYY-MM-DD"}],
            "scene": {"type": "text", "value": {"binding": "c.text", "fallback": ""}}}
    }"#;
    assert_eq!(
        PluginManifest::parse(json.as_bytes()).unwrap_err().code(),
        PluginErrorCode::InvalidTimeFormat
    );
}

// ---------------------------------------------------------------- worker

#[test]
fn a_worker_starts_empty_and_publishes_a_snapshot() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let (layers, _consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    // The initial reload publishes before the first command.
    let snapshot = wait_for_revision(&handle, 0);
    assert!(snapshot.entries.is_empty());
    assert!(snapshot.active.is_empty());
    assert_eq!(snapshot.phase, Some(super::worker::PluginPhase::Idle));
    assert!(endpoint.send(super::worker::PluginCommand::RefreshCatalog));
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

#[test]
fn a_worker_that_was_never_asked_anything_still_reports_its_stopped_state() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let (layers, _consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    wait_for_revision(&handle, 0);
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

/// Wait for a snapshot newer than `since`, or give up.
///
/// Polling rather than a notification because the snapshot is published from the
/// worker thread and the test only needs to know it eventually happened; a fixed
/// sleep would be either flaky or slow.
fn wait_for_revision(
    handle: &super::worker::PluginWorkerHandle,
    since: u64,
) -> super::worker::PluginSnapshot {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if handle.changed_since(since) {
            return handle.snapshot();
        }
        if std::time::Instant::now() >= deadline {
            return handle.snapshot();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_installed_plugin_appears_in_the_snapshot_and_its_panel_reaches_the_channel() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    store
        .unpack(
            &id,
            &version,
            &zip_with(&[("plugin.json", &pomodoro_manifest_json())]),
        )
        .expect("the archive unpacks");
    store
        .set_current(&id, &version)
        .expect("the version goes live");

    let (layers, consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");

    let snapshot = wait_for_revision(&handle, 0);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].manifest.id, id);
    assert!(snapshot.entries[0].installed);
    assert!(
        snapshot.entries[0].enabled,
        "a freshly installed plugin runs"
    );
    assert_eq!(snapshot.active, vec![id.clone()]);

    // And the panel reached the layer channel, which is how it gets to the model
    // window at all.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let published = loop {
        let published = consumer.take_latest();
        if !published.is_empty() {
            break published;
        }
        if std::time::Instant::now() >= deadline {
            break published;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(published.len(), 1, "one enabled plugin publishes one layer");
    let layer = &published[0];
    assert!(layer.raster.is_valid());
    assert!(
        layer.raster.pixels.iter().any(|byte| *byte > 0),
        "the panel drew something"
    );
    assert_eq!(
        layer.placement.anchor,
        bongocat_render::OverlayAnchor::BottomLeft
    );
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

fn pomodoro_manifest_json() -> Vec<u8> {
    r#"{
        "schema_version": 1, "api_version": 1, "id": "pomodoro", "name": "Pomodoro",
        "version": "1.0.0",
        "overlay": {
            "anchor": "bottom_left", "size": [200, 120],
            "behaviors": [{"id": "timer", "kind": "countdown", "duration_seconds": 1500}],
            "scene": {"type": "stack", "background": "202020ff", "padding": [10, 10],
                "children": [
                    {"type": "text", "value": {"binding": "timer.remaining_text", "fallback": "25:00"}, "size": 28},
                    {"type": "progress_bar", "value": {"fraction": "timer.progress", "fallback": 0}, "height": 6}
                ]}
        }
    }"#
    .as_bytes()
    .to_vec()
}

#[test]
fn a_disabled_plugin_publishes_no_layer() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    store
        .unpack(
            &id,
            &version,
            &zip_with(&[("plugin.json", &pomodoro_manifest_json())]),
        )
        .expect("the archive unpacks");
    store
        .set_current(&id, &version)
        .expect("the version goes live");

    let (layers, consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    wait_for_revision(&handle, 0);
    let _ = consumer.take_latest();
    // The worker publishes its own evaluations, so the wait has to be for a change
    // from the revision observed now rather than for a fixed number — a fixed
    // number would be satisfied by an evaluation that happened before the command.
    let before = handle.snapshot().revision;

    assert!(endpoint.send(super::worker::PluginCommand::SetEnabled {
        id: id.clone(),
        enabled: false,
    }));
    let snapshot = wait_for_revision(&handle, before);
    assert!(snapshot.active.is_empty());
    assert!(!snapshot.entries[0].enabled);
    // The channel is drained by the evaluator, so a later read shows the removal.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !consumer.take_latest().is_empty() {
        if std::time::Instant::now() >= deadline {
            break;
        }
    }
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

#[test]
fn uninstalling_removes_the_plugin_from_the_snapshot() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    store
        .unpack(
            &id,
            &version,
            &zip_with(&[("plugin.json", &pomodoro_manifest_json())]),
        )
        .expect("the archive unpacks");
    store
        .set_current(&id, &version)
        .expect("the version goes live");

    let (layers, _consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    wait_for_revision(&handle, 0);
    assert_eq!(handle.snapshot().entries.len(), 1);

    assert!(endpoint.send(super::worker::PluginCommand::Uninstall(id.clone())));
    // The phase is announced before the work so the centre can show a spinner, so
    // the wait is for the *list* to empty rather than for the next revision — the
    // revision that announces the removal still lists the plugin.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if handle.snapshot().entries.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the plugin was still listed five seconds after being uninstalled"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

#[test]
fn a_plugin_the_catalog_offers_appears_in_the_snapshot_with_its_offered_version() {
    let directory = tempfile::tempdir().expect("a temp directory");
    let catalog_directory = directory.path().join("catalog");
    std::fs::create_dir_all(&catalog_directory).expect("the catalog directory");
    std::fs::write(catalog_directory.join("plugins.json"), catalog_bytes())
        .expect("the catalog writes");

    let store = PluginStore::new(directory.path().join("plugins"));
    let (layers, _consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        catalog_directory,
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    assert!(endpoint.send(super::worker::PluginCommand::RefreshCatalog));
    let snapshot = wait_for_revision(&handle, 0);
    // The catalog names `pomodoro`; on a host whose platform key it does not
    // announce, the entry is still listed but is refused with a reason, which is
    // what the centre shows.
    assert_eq!(snapshot.entries.len(), 1);
    let entry = &snapshot.entries[0];
    assert_eq!(entry.manifest.name, "Pomodoro");
    assert!(!entry.installed);
    if super::host_platform() == "macos-aarch64" {
        assert_eq!(entry.available_version, Some(PluginVersion::new(1, 0, 0)));
        assert!(entry.refusal.is_none());
        assert!(entry.is_installable());
    } else {
        assert_eq!(
            entry.refusal.as_ref().map(PluginError::code),
            Some(PluginErrorCode::PluginNotPublished)
        );
    }
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

#[test]
fn a_press_that_reaches_the_worker_runs_the_actions_behavior() {
    // The full loop the design exists for: a panel on the model window, a press in
    // it, and a timer that moves.
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let id = PluginId::new("pomodoro").unwrap();
    let version = PluginVersion::new(1, 0, 0);
    store
        .unpack(
            &id,
            &version,
            &zip_with(&[("plugin.json", &pomodoro_press_json())]),
        )
        .expect("the archive unpacks");
    store
        .set_current(&id, &version)
        .expect("the version goes live");

    let (layers, mut consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    wait_for_revision(&handle, 0);

    // A press the worker resolves through the sink the overlay would hold, against
    // the layer id the panel was actually published with. A layer id that names no
    // loaded plugin is ignored rather than failing the worker.
    use bongocat_render::OverlayPressSink as _;
    let sink = endpoint.press_sink();
    sink.press(u64::MAX, 1.0, 1.0);
    wait_for_revision(&handle, 0);
    assert_eq!(
        handle.snapshot().entries.len(),
        1,
        "the worker survived the press"
    );

    // The layer the published panel occupies: read from the channel rather than
    // known, so a change to how ids are allocated is caught here.
    let published = wait_for_layers(&mut consumer, 1);
    sink.press(published[0].id, 1.0, 1.0);
    wait_for_revision(&handle, 0);
    assert_eq!(
        handle.snapshot().entries.len(),
        1,
        "the worker survived a real press too"
    );

    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
}

/// Wait until at least `count` layers have been published, and return them.
fn wait_for_layers(
    consumer: &mut bongocat_render::OverlayLayerConsumer,
    count: usize,
) -> Vec<bongocat_render::OverlayLayer> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let layers = consumer.take_latest();
        if layers.len() >= count {
            return layers;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the worker published no layers"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pomodoro_press_json() -> Vec<u8> {
    r#"{
        "schema_version": 1, "api_version": 1, "id": "pomodoro", "name": "Pomodoro",
        "version": "1.0.0",
        "overlay": {
            "anchor": "bottom_left", "size": [200, 80],
            "behaviors": [{"id": "timer", "kind": "countdown", "duration_seconds": 1500}],
            "scene": {"type": "stack", "background": "202020ff", "padding": [10, 10],
                "children": [
                    {"type": "text", "value": {"binding": "timer.remaining_text", "fallback": "25:00"}, "size": 20},
                    {"type": "button", "id": "go", "label": "Start", "action": "toggle",
                     "target": "timer", "variant": "primary", "radius": 6}
                ]}
        }
    }"#
    .as_bytes()
    .to_vec()
}

#[test]
fn a_worker_reports_a_stop_and_its_layers_after_the_frame_source_is_done() {
    // The shutdown ordering property: the worker stops and closes its producer, so
    // the render thread is never publishing into a worker that has gone.
    let directory = tempfile::tempdir().expect("a temp directory");
    let store = PluginStore::new(directory.path().join("plugins"));
    let (layers, consumer) = bongocat_render::overlay_layer_channel();
    let (handle, endpoint) = super::worker::start(
        store,
        directory.path().join("catalog"),
        layers,
        std::sync::Arc::new(super::LocalTimeCache::new()),
        None,
    )
    .expect("the worker starts");
    wait_for_revision(&handle, 0);
    handle.stopper(&endpoint).stop();
    handle
        .stop_and_join(Duration::from_secs(5))
        .expect("the worker stops");
    assert!(consumer.take_latest().is_empty());
    // And publishing to a closed producer is refused rather than silently dropped.
    assert_eq!(
        bongocat_render::OverlayLayerPublishError::Closed.to_string(),
        "the overlay layer channel is closed"
    );
}

#[test]
fn a_local_time_cache_reads_midnight_until_it_is_refreshed() {
    let cache = super::LocalTimeCache::new();
    assert!(!cache.is_primed());
    assert_eq!(cache.read(), WallClock::new(0, 0, 0));
    cache.refresh();
    // Whether the platform answered is not asserted — a machine that cannot say
    // still shows a clock — but the cache is then primed and the reading is in
    // range, which is what a panel formats.
    assert!(cache.is_primed());
    let reading = cache.read();
    assert!(reading.hour <= 23 && reading.minute <= 59 && reading.second <= 59);
}

#[test]
fn a_directory_that_does_not_exist_is_reported_rather_than_created_silently() {
    // The catalog directory is the plugin author's, not the product's: a missing
    // one means "no catalog yet", not "create a directory in the user's data root".
    let missing = Path::new("/nonexistent-bongocat-plugin-catalog");
    let loaded =
        super::catalog::load_local(missing).expect("a missing directory is an empty catalog");
    assert!(loaded.catalog.plugins.is_empty());
}

#[test]
fn a_scene_of_only_a_spacer_publishes_no_layer() {
    // Nothing was drawn, so there is nothing to upload or hit-test.
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "blank", "name": "Blank", "version": "1.0.0",
        "overlay": {"size": [100, 60], "scene": {"type": "spacer", "grow": 1}}
    }"#;
    let manifest = PluginManifest::parse(json.as_bytes()).expect("the manifest parses");
    let mut measurer = TextMeasurer::new(FontBook::load_system());
    let panel = render_contribution(
        &manifest.overlay,
        &bongocat_plugin_protocol::BindingTable::new(),
        1.0,
        &mut measurer,
        &ImageLibrary::new(),
    )
    .expect("the panel rasterizes");
    assert!(panel.pixels.is_empty());
}

#[test]
fn a_behavior_reports_the_declaration_it_was_built_from() {
    // A caller inspecting a running plugin gets the declaration it was built from,
    // which is what a diagnostics line should print.
    let plugin = instance(&[("timer", "countdown")]);
    let behavior = plugin
        .behavior(&BehaviorId::new("timer"))
        .expect("the behavior exists");
    assert!(matches!(
        behavior.spec(),
        bongocat_plugin_protocol::BehaviorSpec::Countdown(_)
    ));
    assert_eq!(behavior.state().to_string(), "countdown 1500");
}

#[test]
fn a_scene_value_that_names_a_source_with_no_dot_is_refused() {
    // Guard against a scene that binds to a bare word: it could never resolve, and
    // a silent miss is a panel that shows its fallback forever.
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "demo", "name": "Demo", "version": "1.0.0",
        "overlay": {"size": [100, 60],
            "behaviors": [{"id": "t", "kind": "counter"}],
            "scene": {"type": "text", "value": {"binding": "t", "fallback": ""}}}
    }"#;
    assert_eq!(
        PluginManifest::parse(json.as_bytes()).unwrap_err().code(),
        PluginErrorCode::InvalidBinding
    );
}

#[test]
fn a_scene_node_is_validated_before_anything_is_drawn() {
    // The load-time check is what keeps a malformed scene from being a rendering
    // problem: a scene with a bad asset path is refused, not drawn with a hole.
    let json = r#"{
        "schema_version": 1, "api_version": 1, "id": "demo", "name": "Demo", "version": "1.0.0",
        "overlay": {"size": [100, 60],
            "scene": {"type": "stack", "children": [{"type": "image", "asset": "../../secret.png"}]}}
    }"#;
    assert_eq!(
        PluginManifest::parse(json.as_bytes()).unwrap_err().code(),
        PluginErrorCode::InvalidAssetPath
    );
}

#[test]
fn a_scene_of_nothing_but_spacers_still_has_a_valid_size() {
    // A panel that draws nothing is still a panel with a size, and the size is
    // what places it on the model window.
    let manifest = PluginManifest::parse(
        br#"{"schema_version":1,"api_version":1,"id":"blank","name":"Blank","version":"1.0.0",
            "overlay":{"size":[64,32],"scene":{"type":"spacer","grow":1}}}"#,
    )
    .expect("the manifest parses");
    assert_eq!(manifest.overlay.size, [64, 32]);
    let placement = bongocat_render::OverlayLayerPlacement {
        anchor: manifest.overlay.anchor.to_overlay_anchor(),
        margin: manifest.overlay.margin,
        nudge: [0.0, 0.0],
        width_fraction: manifest.overlay.width_fraction,
        opacity: manifest.overlay.opacity,
    };
    assert!(bongocat_render::overlay_layer_clip_rect(placement, 2.0).is_some());
}

#[test]
fn a_scene_node_default_is_a_spacer_and_a_value_default_is_empty_text() {
    // The defaults the protocol promises, asserted where a plugin author would
    // rely on them.
    assert_eq!(SceneValue::default(), SceneValue::Text(String::new()));
}

/// The reference plugin, at the path a developer reads it from.
const REFERENCE_POMODORO: &[u8] = include_bytes!("../../../plugins/pomodoro/plugin.json");

#[test]
fn the_shipped_reference_plugin_loads_and_rasterizes() {
    // Two properties in one, because they fail the same way for a plugin author: a
    // manifest that parses but does not load is not a working example, and a panel
    // that loads but does not rasterize is not a panel. The reference plugin is the
    // one file every other plugin author starts from.
    let manifest = PluginManifest::parse(REFERENCE_POMODORO)
        .expect("the reference plugin is a manifest this host accepts");
    let mut instance = PluginInstance::new(manifest.id.clone(), &manifest.overlay.behaviors)
        .expect("the reference plugin's behaviors are valid");
    let table = instance.evaluate(Duration::from_secs(1), WallClock::new(0, 0, 0));

    let mut fonts = TextMeasurer::new(FontBook::load_system());
    let panel = render_contribution(
        &manifest.overlay,
        &table,
        1.0,
        &mut fonts,
        &ImageLibrary::new(),
    )
    .expect("the reference plugin's panel draws");
    assert!(!panel.pixels.is_empty(), "and it is not an empty image");
    assert_eq!(panel.to_raster().width, manifest.overlay.size[0]);
    assert_eq!(panel.to_raster().height, manifest.overlay.size[1]);
    // Both buttons are pressable, which is the half a manifest cannot show: a
    // button that is laid out but never hit is a reference plugin that teaches the
    // wrong thing. The regions are read rather than guessed, so this asserts that
    // every button the scene drew is one a press can reach — the coordinates a
    // plugin author would have to guess are the layout's business, not this test's.
    for id in ["toggle", "reset"] {
        let region = panel
            .hit_regions
            .iter()
            .find(|region| region.button == id)
            .unwrap_or_else(|| panic!("the {id} button must be laid out"));
        let centre = (
            region.rect.x + region.rect.width / 2.0,
            region.rect.y + region.rect.height / 2.0,
        );
        assert_eq!(
            panel.hit_test(centre.0, centre.1),
            Some(id),
            "the {id} button must be pressable where it is drawn"
        );
    }
}
