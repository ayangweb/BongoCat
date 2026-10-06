//! Which behaviors a model plays on its own, end to end.
//!
//! The chain this module covers has one seam nobody else can see: the selection is
//! stored as a canonical `behavior_id` string, the runtime filters by a string it
//! builds from what the model declares, and the settings page builds the same string
//! a third time from the protocol's typed behavior. Two of those three are in
//! different crates, and a mismatch would not fail to compile — it would silently
//! filter everything out, or nothing.
//!
//! So these tests do not reach for the runtime's private helper. They write a
//! selection the way the configuration does and then watch what the runtime actually
//! draws from a real preset model.

use super::*;

use crate::config_projection::{
    random_behavior_inclusion_from_config, with_random_behavior_inclusion,
};
use bongocat_config::RandomBehaviorInclusion;

/// The behaviors the standard preset declares, as the configuration names them.
///
/// Built from the model package rather than hard-coded, so a package update that
/// renames an asset changes this list instead of quietly invalidating the selection
/// the test wrote.
fn declared_behavior_ids(model_id: &str) -> Vec<String> {
    let catalog = bongocat_model::PresetModelCatalog::open(
        repository_preset_root(),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog");
    let model = catalog
        .load(&bongocat_model::ModelId::parse(model_id).expect("model id"))
        .expect("preset model");
    model
        .snapshot()
        .behaviors
        .iter()
        .map(|behavior| match behavior {
            bongocat_model::ModelBehaviorSnapshot::Motion { group, index } => {
                format!("motion:{group}:{index}")
            }
            bongocat_model::ModelBehaviorSnapshot::Expression { name } => {
                format!("expression:{name}")
            }
        })
        .collect()
}

fn standard() -> ModelIdentity {
    ModelIdentity {
        id: "standard".to_owned(),
        source: ModelSource::BuiltIn,
    }
}

/// Start an application with the standard preset live and the overlay pumped.
///
/// The pump is what reports a prepared model as committed, so the runtime reaches
/// the state where the scheduler has a model to draw from — the same path the
/// product takes through the overlay thread.
fn started_with_standard(layout: &StorageLayout) -> (Application, RenderPump, ModelCommitToken) {
    let mut application = Application::start_with_layout_internal(
        layout.clone(),
        repository_preset_root().as_path(),
        true,
        Language::English,
    )
    .expect("start rendering application");
    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare the standard model");
    let pump = RenderPump::start(
        application
            .take_render_consumer()
            .expect("take render consumer"),
    );
    application
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("the startup model committed");
    (application, pump, token)
}

/// Automatic playback uses the upper half of the runtime's command sequence space,
/// so a sequence with the top bit set is a draw the scheduler made on its own.
const AUTOMATIC_SEQUENCE_MARKER: u64 = 1 << 63;

/// Every automatic draw the scheduler makes inside a window, as the behavior it drew.
///
/// One draw from a set of three can land on the only allowed member by luck, which
/// is exactly the mistake a single-draw test would make — so a window, not a draw.
///
/// The *sequence* is what identifies a draw rather than the snapshot's current
/// motion or expression: a motion stays in the snapshot after it completes, so
/// "something is still playing" says nothing about whether the scheduler just chose
/// it. `known` is therefore the caller's, so a test that watches two windows in a row
/// — before and after a change — does not count the draw it already saw as a new one.
/// That distinction is the whole reason "nothing is selected" can be asserted at all:
/// the previous draw is still on screen, and it must not be mistaken for a fresh one.
fn automatic_draws_over(
    application: &Application,
    known: &mut BTreeSet<u64>,
    window: Duration,
) -> Vec<String> {
    let client = application.runtime_client();
    let mut seen = Vec::new();
    let deadline = Instant::now() + window;
    while Instant::now() < deadline {
        let snapshot = client.snapshot();
        let draws = [
            snapshot.active_expression.as_ref().map(|active| {
                (
                    active.command_sequence,
                    format!("expression:{}", active.expression.name()),
                )
            }),
            snapshot.active_motion.as_ref().map(|active| {
                (
                    active.command_sequence,
                    format!("motion:{}:{}", active.motion.group(), active.motion.index()),
                )
            }),
        ];
        for draw in draws.into_iter().flatten() {
            if draw.0 < AUTOMATIC_SEQUENCE_MARKER {
                continue;
            }
            if known.insert(draw.0) {
                seen.push(draw.1);
            }
        }
        let _ = client.send(bongocat_runtime::RuntimeCommand::Tick);
        std::thread::sleep(Duration::from_millis(2));
    }
    seen
}

/// The selection reaches the runtime as the same behaviors the model declares.
///
/// This is the whole point of the module: a selection written the way the
/// configuration and the settings page write it must be honoured by the runtime's
/// own filter, over many draws, without any of the three agreeing by accident.
#[test]
fn a_written_selection_is_the_only_thing_the_scheduler_draws_from() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = started_with_standard(&layout);

    let mut known = BTreeSet::new();
    let declared = declared_behavior_ids("standard");
    assert!(
        declared.len() >= 2,
        "the fixture needs at least two behaviors to make a selection meaningful"
    );
    let allowed = declared
        .last()
        .cloned()
        .expect("at least one declared behavior");
    let excluded: Vec<String> = declared
        .iter()
        .filter(|behavior| **behavior != allowed)
        .cloned()
        .collect();

    application
        .set_random_behavior_settings(RandomBehaviorSettings {
            mode: RuntimeRandomBehaviorMode::MotionsAndExpressions,
            interval_seconds: 1,
        })
        .expect("turn random playback on");
    application
        .set_random_behavior_inclusion(standard(), vec![allowed.clone()])
        .expect("select one behavior");

    let picks = automatic_draws_over(&application, &mut known, Duration::from_millis(2500));
    assert!(
        picks.iter().all(|pick| *pick == allowed),
        "the scheduler drew from outside the selection: {picks:?} (excluded {excluded:?})"
    );
}

/// Selecting nothing stops the scheduler for that model, and it is not the same as
/// having selected nothing yet.
#[test]
fn selecting_nothing_stops_the_scheduler_while_selecting_nothing_yet_does_not() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = started_with_standard(&layout);
    let mut known = BTreeSet::new();

    application
        .set_random_behavior_settings(RandomBehaviorSettings {
            mode: RuntimeRandomBehaviorMode::MotionsAndExpressions,
            interval_seconds: 1,
        })
        .expect("turn random playback on");
    assert!(
        !automatic_draws_over(&application, &mut known, Duration::from_millis(2500)).is_empty(),
        "a model with no selection at all plays from everything it declares"
    );

    application
        .set_random_behavior_inclusion(standard(), Vec::new())
        .expect("select nothing");
    assert!(
        automatic_draws_over(&application, &mut known, Duration::from_millis(2500)).is_empty(),
        "an empty selection must leave the scheduler with nothing to play"
    );
}

/// The mode and the selection are two filters, and either can empty the set.
///
/// A model that selected only motions while the mode is expressions-only plays
/// nothing — it does not fall back to playing a motion the mode excluded.
#[test]
fn a_selection_the_mode_excludes_leaves_the_scheduler_idle() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = started_with_standard(&layout);

    let mut known = BTreeSet::new();
    let declared = declared_behavior_ids("standard");
    let motions: Vec<String> = declared
        .iter()
        .filter(|behavior| behavior.starts_with("motion:"))
        .cloned()
        .collect();
    assert!(!motions.is_empty(), "the fixture needs a motion to select");
    assert!(
        declared
            .iter()
            .any(|behavior| behavior.starts_with("expression:")),
        "the fixture needs an expression for the mode to exclude"
    );

    application
        .set_random_behavior_settings(RandomBehaviorSettings {
            mode: RuntimeRandomBehaviorMode::Expressions,
            interval_seconds: 1,
        })
        .expect("expressions only");
    application
        .set_random_behavior_inclusion(standard(), motions)
        .expect("select only motions");

    assert!(
        automatic_draws_over(&application, &mut known, Duration::from_millis(2500)).is_empty(),
        "the mode decides which kind plays; a selection cannot widen it back"
    );
}

/// The selection belongs to one model, and switching models carries the other
/// model's answer with it rather than leaking it onto the new one.
///
/// A selection that followed the application instead of the model would filter every
/// model by the last one's answer, which is the failure this whole shape exists to
/// avoid.
#[test]
fn each_model_keeps_its_own_selection_across_a_switch() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = started_with_standard(&layout);

    let mut known = BTreeSet::new();
    let keyboard = ModelIdentity {
        id: "keyboard".to_owned(),
        source: ModelSource::BuiltIn,
    };
    let standard_allowed = declared_behavior_ids("standard")
        .last()
        .cloned()
        .expect("a declared behavior");
    let keyboard_allowed = declared_behavior_ids("keyboard")
        .first()
        .cloned()
        .expect("a declared behavior");

    application
        .set_random_behavior_inclusion(standard(), vec![standard_allowed.clone()])
        .expect("select for standard");
    application
        .set_random_behavior_inclusion(keyboard.clone(), vec![keyboard_allowed.clone()])
        .expect("select for keyboard");

    // Both rows survive each other's write, which is the part a single shared field
    // could not do.
    let rows = application
        .config()
        .model
        .random_behavior
        .included
        .clone()
        .expect("a selection document");
    assert_eq!(
        rows.iter()
            .find(|row| row.model == standard())
            .map(|row| row.behavior_ids.clone()),
        Some(vec![standard_allowed.clone()])
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.model == keyboard)
            .map(|row| row.behavior_ids.clone()),
        Some(vec![keyboard_allowed.clone()])
    );

    // And switching to the other model draws from that model's answer, not the one
    // the runtime happened to be told last.
    application
        .set_random_behavior_settings(RandomBehaviorSettings {
            mode: RuntimeRandomBehaviorMode::MotionsAndExpressions,
            interval_seconds: 1,
        })
        .expect("turn random playback on");
    application
        .select_model(ModelOrigin::Preset, "keyboard")
        .expect("switch to the keyboard model");
    let picks = automatic_draws_over(&application, &mut known, Duration::from_millis(2500));
    assert!(
        picks.iter().all(|pick| *pick == keyboard_allowed),
        "the keyboard model drew from standard's selection: {picks:?}"
    );
}

/// A model with no row of its own plays nothing, once the document exists at all.
///
/// This is the rule that makes an empty row and an absent document distinguishable
/// without a separate flag, and it is why the projection seeds every model when the
/// document is created: writing a selection on one model must not decide for the
/// others. It is reached here through the document itself rather than through the
/// settings page, because the page can no longer produce it — which is the point.
#[test]
fn a_model_absent_from_a_hand_written_document_plays_nothing() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut seeded = NativeConfig::default();
    seeded.model.random_behavior.mode = bongocat_config::RandomBehaviorMode::MotionsAndExpressions;
    seeded.model.random_behavior.interval_seconds = 1;
    seeded.model.random_behavior.included = Some(vec![RandomBehaviorInclusion {
        model: ModelIdentity {
            id: "keyboard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_ids: declared_behavior_ids("keyboard"),
    }]);
    seeded.validate().expect("a seeded document is valid");
    layout.config.parent().map(fs::create_dir_all);
    std::fs::write(
        &layout.config,
        serde_json::to_vec_pretty(&seeded).expect("serialize the seeded document"),
    )
    .expect("write the seeded document");

    let (application, _pump, _token) = started_with_standard(&layout);
    assert!(
        automatic_draws_over(
            &application,
            &mut BTreeSet::new(),
            Duration::from_millis(2500)
        )
        .is_empty(),
        "a model with no row in an existing document selected nothing"
    );
}

/// The projection the runtime reads is the projection the configuration holds.
///
/// Two separate readings of the same row — one for the runtime and one for the
/// settings page — would let the page show one answer while the model plays another.
#[test]
fn the_runtime_and_the_settings_page_read_one_selection() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = started_with_standard(&layout);

    let allowed = declared_behavior_ids("standard")
        .first()
        .cloned()
        .expect("a declared behavior");
    application
        .set_random_behavior_inclusion(standard(), vec![allowed.clone()])
        .expect("select one behavior");

    let projection = random_behavior_inclusion_from_config(application.config(), &standard())
        .expect("the standard model has a row");
    assert_eq!(
        projection.behavior_ids,
        BTreeSet::from([allowed.clone()]),
        "the runtime sees exactly what was written"
    );

    // A document that names another model projects nothing for this one rather than
    // another model's answer. Written out rather than built through the helper,
    // because the helper's whole job is to *keep* every other model's row.
    let mut only_keyboard = application.config().clone();
    only_keyboard.model.random_behavior.included = Some(vec![RandomBehaviorInclusion {
        model: ModelIdentity {
            id: "keyboard".to_owned(),
            source: ModelSource::BuiltIn,
        },
        behavior_ids: Vec::new(),
    }]);
    assert_eq!(
        random_behavior_inclusion_from_config(&only_keyboard, &standard())
            .expect("a document exists")
            .behavior_ids,
        BTreeSet::new(),
        "a model with no row selects nothing"
    );

    // And an absent document projects nothing at all, which the runtime reads as
    // "no filter" rather than "nothing plays".
    let mut without_document = application.config().clone();
    without_document.model.random_behavior.included = None;
    assert!(
        random_behavior_inclusion_from_config(&without_document, &standard()).is_none(),
        "an absent document is the unfiltered answer"
    );
}

/// Writing a selection sorts it, seeds every model, and rewrites only one row.
///
/// Order is not cosmetic: the document is read by a person when a selection stops
/// behaving, and two runs over the same choice producing two different documents
/// would make that unreadable. The seeding is the other half — a model with no row
/// plays nothing, so a document created by one checkbox must not leave the models the
/// user never opened silent.
#[test]
fn writing_a_selection_sorts_it_seeds_every_model_and_rewrites_one_row() {
    let keyboard = ModelIdentity {
        id: "keyboard".to_owned(),
        source: ModelSource::BuiltIn,
    };
    let catalog = vec![
        (standard(), vec!["motion:CAT_motion:0".to_owned()]),
        (keyboard.clone(), vec!["motion:CAT_motion:1".to_owned()]),
    ];
    let mut config = NativeConfig::default();
    let first = with_random_behavior_inclusion(
        &config,
        &catalog,
        standard(),
        vec![
            "motion:CAT_motion:1".to_owned(),
            "motion:CAT_motion:0".to_owned(),
        ],
    );
    config.model.random_behavior.included = Some(first.clone());
    assert_eq!(
        config.model.random_behavior.included.as_ref().map(Vec::len),
        Some(2)
    );
    let rows = config
        .model
        .random_behavior
        .included
        .as_ref()
        .expect("rows");
    assert_eq!(
        rows.iter()
            .find(|row| row.model == standard())
            .map(|row| row.behavior_ids.clone()),
        Some(vec![
            "motion:CAT_motion:0".to_owned(),
            "motion:CAT_motion:1".to_owned()
        ]),
        "the edited model's row is sorted"
    );
    assert_eq!(
        rows.iter()
            .find(|row| row.model == keyboard)
            .map(|row| row.behavior_ids.clone()),
        Some(vec!["motion:CAT_motion:1".to_owned()]),
        "a model the user never opened keeps everything it declares, so it plays as before"
    );
    config.validate().expect("a seeded document is valid");

    let second = with_random_behavior_inclusion(&config, &catalog, standard(), Vec::new());
    config.model.random_behavior.included = Some(second);
    let rows = config
        .model
        .random_behavior
        .included
        .as_ref()
        .expect("rows");
    assert_eq!(rows.len(), 2, "a rewrite replaces the model's own row");
    assert!(rows[0].behavior_ids.is_empty());
    assert_eq!(
        rows.iter()
            .find(|row| row.model == keyboard)
            .map(|row| row.behavior_ids.clone()),
        Some(vec!["motion:CAT_motion:1".to_owned()]),
        "the other model's row is untouched by this model's write"
    );
    config
        .validate()
        .expect("selecting nothing is a valid document");

    let added = with_random_behavior_inclusion(
        &config,
        &catalog,
        ModelIdentity {
            id: "gamepad".to_owned(),
            source: ModelSource::BuiltIn,
        },
        vec!["motion:CAT_motion:2".to_owned()],
    );
    assert_eq!(
        added.len(),
        3,
        "another model joins a document that already exists"
    );
    assert!(
        added
            .iter()
            .any(|row| row.model == standard() && row.behavior_ids.is_empty()),
        "the first model's row is untouched by the second model's write"
    );
    let _: RandomBehaviorInclusion = added[0].clone();
}

/// One checkbox on one model must not change what any other model plays.
///
/// This is the regression the seeding exists for: a document created by a single
/// checkbox used to hold one row, and every model without a row plays nothing — so
/// saving a selection on the model that happened to be on screen silenced every model
/// the user had not opened.
#[test]
fn saving_a_selection_on_one_model_leaves_the_others_alone() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = started_with_standard(&layout);

    let keyboard = ModelIdentity {
        id: "keyboard".to_owned(),
        source: ModelSource::BuiltIn,
    };
    application
        .set_random_behavior_settings(RandomBehaviorSettings {
            mode: RuntimeRandomBehaviorMode::MotionsAndExpressions,
            interval_seconds: 1,
        })
        .expect("turn random playback on");
    application
        .set_random_behavior_inclusion(
            standard(),
            vec![
                declared_behavior_ids("standard")
                    .first()
                    .cloned()
                    .expect("a declared behavior"),
            ],
        )
        .expect("select for the live model");

    // The keyboard model now has a row, seeded with everything it declares, so it
    // plays exactly what it played before the write.
    let rows = application
        .config()
        .model
        .random_behavior
        .included
        .clone()
        .expect("a selection document");
    let keyboard_row = rows
        .iter()
        .find(|row| row.model == keyboard)
        .expect("every catalog model is seeded");
    let mut declared = declared_behavior_ids("keyboard");
    declared.sort();
    let mut seeded = keyboard_row.behavior_ids.clone();
    seeded.sort();
    assert_eq!(
        seeded, declared,
        "an untouched model keeps everything it declares"
    );

    application
        .select_model(ModelOrigin::Preset, "keyboard")
        .expect("switch to the keyboard model");
    assert!(
        !automatic_draws_over(
            &application,
            &mut BTreeSet::new(),
            Duration::from_millis(2500)
        )
        .is_empty(),
        "a seeded model still plays, which is the whole point of seeding it"
    );
}
