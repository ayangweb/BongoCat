//! The remembered per-model expression: recording a choice, and putting it back
//! when the same model is shown again.

use super::*;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Stand in for the overlay session.
///
/// A prepared model only becomes a committed one when something that owns the
/// render consumer reports it, and in the product that is the overlay thread.
/// Every test here switches models with `select_model`, which waits for the
/// commit, so the report has to come from somewhere else or the application would
/// wait for a frame the test itself is the only thing able to ask for. The pump
/// reports every prepared frame as prepared, which is what a working GPU path
/// does.
struct RenderPump {
    stop: Arc<AtomicBool>,
    pump: Option<thread::JoinHandle<()>>,
}

impl RenderPump {
    fn start(consumer: RenderConsumer) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let pump_stop = Arc::clone(&stop);
        let pump = thread::spawn(move || {
            while !pump_stop.load(Ordering::Acquire) {
                if let Some(frame) = consumer.take_latest()
                    && let Some(token) = frame.model_commit
                {
                    let _ = consumer.report_model_commit(ModelCommitFeedback {
                        token,
                        outcome: ModelCommitOutcome::Prepared,
                    });
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        Self {
            stop,
            pump: Some(pump),
        }
    }
}

impl Drop for RenderPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

/// The expression on screen, once the runtime has drained everything that was
/// queued behind the commit a model switch returned on.
///
/// A restore rides on the activation rather than replacing it, so it is a
/// separate command that lands just after. `None` is the settled answer for a
/// model nothing is remembered for, so a caller that expects an expression polls
/// until one appears rather than reading the first frame it sees.
fn settled_expression(application: &Application, expected: bool) -> Option<String> {
    let client = application.runtime_client();
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    loop {
        let snapshot = client.snapshot();
        if snapshot.active_expression.is_some() == expected {
            return snapshot
                .active_expression
                .map(|active| active.expression.name().to_owned());
        }
        assert!(
            Instant::now() < deadline,
            "the runtime did not settle to the expected expression state"
        );
        std::thread::yield_now();
    }
}

/// Start a rendering application, prepare its first model, and hand the render
/// consumer to a pump — which is the order the product uses: startup prepares,
/// the overlay takes over, and every later switch goes through `select_model`.
fn start_with_pumped_overlay(
    layout: &StorageLayout,
) -> (Application, RenderPump, ModelCommitToken) {
    let mut application = Application::start_with_layout_internal(
        layout.clone(),
        repository_preset_root().as_path(),
        true,
        Language::English,
    )
    .expect("start rendering application");
    let token = application
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare the startup model");
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

/// Expressions are per-model assets, so "the expression the user last chose" is
/// only ever a question about one model. Both halves of that have to hold: a
/// choice made on one model must not leak onto another, and returning to a model
/// must return to the face it was left wearing — whether that is a switch back
/// within one session or the next launch.
#[test]
fn each_model_returns_to_the_expression_the_user_last_chose_for_it() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = start_with_pumped_overlay(&layout);

    // A fresh configuration restores nothing and has nothing to restore.
    assert!(!application.config().model.remember_last_expression);
    assert!(application.config().model.last_expressions.is_empty());
    application
        .set_remember_last_expression(true)
        .expect("enable the restore");
    assert_eq!(
        settled_expression(&application, false),
        None,
        "a model nothing is remembered for shows its own default face"
    );

    // The user picks a face on the standard model. It is recorded against that
    // model whether or not the timer that copies it out of the runtime has run.
    application
        .set_expression("live2d_expression0.exp3.json")
        .expect("choose an expression on standard");
    application.persist_user_expression_memory();
    assert_eq!(
        remembered(&application),
        vec![("standard", "live2d_expression0.exp3.json")]
    );

    // A different model is a different question. `keyboard` has nothing
    // remembered for it, so it shows its own default face rather than inheriting
    // the standard model's choice.
    application
        .select_model(ModelOrigin::Preset, "keyboard")
        .expect("select the keyboard model");
    assert_eq!(
        settled_expression(&application, false),
        None,
        "one model's remembered expression must not leak onto another"
    );
    application
        .set_expression("live2d_expression1.exp3.json")
        .expect("choose an expression on keyboard");
    application.persist_user_expression_memory();
    assert_eq!(
        remembered(&application),
        vec![
            ("standard", "live2d_expression0.exp3.json"),
            ("keyboard", "live2d_expression1.exp3.json"),
        ],
        "each model keeps its own record"
    );

    // Coming back is where per-model recording earns its keep: a single global
    // record would put the keyboard model's face on the standard model.
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("return to the standard model");
    assert_eq!(
        settled_expression(&application, true),
        Some("live2d_expression0.exp3.json".to_owned()),
        "returning to a model returns to the expression it was left wearing"
    );
    application.shutdown().expect("clean shutdown");

    // The same is true across a restart, which is the case the setting names:
    // startup activates the configured model and the remembered face comes back
    // with it.
    let mut restarted = Application::start_with_layout_internal(
        layout.clone(),
        repository_preset_root().as_path(),
        true,
        Language::English,
    )
    .expect("restart rendering application");
    assert!(restarted.config().model.remember_last_expression);
    assert_eq!(
        remembered(&restarted),
        vec![
            ("standard", "live2d_expression0.exp3.json"),
            ("keyboard", "live2d_expression1.exp3.json"),
        ],
        "the records outlive the process"
    );
    let token = restarted
        .prepare_model(ModelOrigin::Preset, "standard")
        .expect("prepare the startup model again");
    let _restart_pump = RenderPump::start(
        restarted
            .take_render_consumer()
            .expect("take the restarted render consumer"),
    );
    restarted
        .runtime_client()
        .wait_for_command(token.command_sequence, RUNTIME_TIMEOUT)
        .expect("the restarted model committed");
    assert_eq!(
        settled_expression(&restarted, true),
        Some("live2d_expression0.exp3.json".to_owned()),
        "the next launch restores the expression the user last used for that model"
    );
    restarted.shutdown().expect("clean restart shutdown");
}

/// The switch decides whether a remembered expression is played; it does not
/// decide whether one is kept. Turning it off has to stop the restore without
/// discarding what the user had chosen, and turning it back on has to restore
/// that rather than start from nothing.
#[test]
fn the_switch_gates_the_restore_and_never_the_record() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = start_with_pumped_overlay(&layout);

    // A choice made with the switch off is still the user's choice, so it is
    // still recorded.
    application
        .set_expression("live2d_expression2.exp3.json")
        .expect("choose an expression");
    application.persist_user_expression_memory();
    assert_eq!(
        remembered(&application),
        vec![("standard", "live2d_expression2.exp3.json")],
        "the record does not depend on the switch"
    );

    application
        .set_remember_last_expression(false)
        .expect("turn the restore off");
    application
        .select_model(ModelOrigin::Preset, "keyboard")
        .expect("switch away and back with the restore off");
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("return to standard with the restore off");
    assert_eq!(
        settled_expression(&application, false),
        None,
        "a switched-off restore plays nothing"
    );
    assert_eq!(
        remembered(&application),
        vec![("standard", "live2d_expression2.exp3.json")],
        "a switched-off restore keeps what is remembered"
    );

    application
        .set_remember_last_expression(true)
        .expect("turn the restore back on");
    application
        .select_model(ModelOrigin::Preset, "keyboard")
        .expect("switch away again");
    application
        .select_model(ModelOrigin::Preset, "standard")
        .expect("return to standard with the restore on");
    assert_eq!(
        settled_expression(&application, true),
        Some("live2d_expression2.exp3.json".to_owned()),
        "turning the restore back on restores what was remembered"
    );
    application.shutdown().expect("clean shutdown");
}

/// A record names a model the store may no longer hold. Deleting that model must
/// take the record with it, so the list keeps describing models that exist — and
/// a build-shipped model that happens to share the id names a different model, so
/// its record stays.
#[test]
fn deleting_a_model_forgets_its_remembered_expression() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let mut application = Application::start_with_layout(layout.clone()).expect("start app");
    let imported_id = import_one(
        &mut application,
        "导入的猫",
        repository_root().join("shared/fixtures/model-fixtures/cases/非 ASCII 模型"),
    )
    .id()
    .as_str()
    .to_owned();
    application.shutdown().expect("clean shutdown");

    // Two records over one id, one per catalog entry, plus one for a model that
    // is still there. Only the imported record describes the model being deleted.
    let preset_record = ModelExpressionMemory {
        model: ModelIdentity {
            id: imported_id.clone(),
            source: ModelSource::BuiltIn,
        },
        expression: "live2d_expression0.exp3.json".to_owned(),
    };
    let imported_record = ModelExpressionMemory {
        model: ModelIdentity {
            id: imported_id.clone(),
            source: ModelSource::Imported,
        },
        expression: "live2d_expression1.exp3.json".to_owned(),
    };
    let store = ConfigStore::new(layout.clone()).expect("config store");
    let mut config = store.load_or_default().expect("stored config").config;
    config.model.last_expressions = vec![preset_record.clone(), imported_record];
    config
        .validate()
        .expect("two catalogs may hold the same id as two models");
    store
        .commit(&config)
        .expect("seed the remembered expressions");
    drop(store);

    let mut application = Application::start_with_layout(layout.clone()).expect("restart app");
    assert_eq!(application.config().model.last_expressions.len(), 2);
    application
        .delete_model(ModelOrigin::Installed, imported_id.as_str())
        .expect("delete the imported model");
    assert_eq!(
        application.config().model.last_expressions,
        vec![preset_record],
        "only the removed model's own record is dropped"
    );
    application.shutdown().expect("clean shutdown");
}

/// The records as `(model id, expression)` pairs, in the order the configuration
/// keeps them, so a test can state what it expects in one line.
fn remembered(application: &Application) -> Vec<(&str, &str)> {
    application
        .config()
        .model
        .last_expressions
        .iter()
        .map(|record| (record.model.id.as_str(), record.expression.as_str()))
        .collect()
}

/// Turning a repeated trigger into an off request is one configuration switch, and
/// it reaches the runtime as model settings rather than as a decision the
/// application makes per request.
///
/// The runtime is the only place that knows which expression is in effect, and
/// the configuration is the only place the choice belongs, so this test is about
/// the two of them staying in step: the command moves the runtime's own answer and
/// the persisted document together, and the remembered choices are untouched.
#[test]
fn the_expression_toggle_is_one_switch_that_reaches_the_runtime_and_the_document() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, _pump, _token) = start_with_pumped_overlay(&layout);

    assert!(!application.config().model.toggle_repeated_expression);
    assert!(
        !application
            .runtime_client()
            .snapshot()
            .model_settings
            .toggle_repeated_expression
    );

    application
        .set_toggle_repeated_expression(true)
        .expect("enable the toggle");
    assert!(application.config().model.toggle_repeated_expression);
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    while !application
        .runtime_client()
        .snapshot()
        .model_settings
        .toggle_repeated_expression
    {
        assert!(
            Instant::now() < deadline,
            "the runtime never received the toggle"
        );
        std::thread::yield_now();
    }

    // Turning the toggle on says nothing about where a model starts, and a repeat
    // that turns an expression off records nothing, so the remembered set is
    // exactly what it was.
    application
        .set_expression("live2d_expression0.exp3.json")
        .expect("choose an expression");
    assert_eq!(
        settled_expression(&application, true).as_deref(),
        Some("live2d_expression0.exp3.json")
    );
    application.persist_user_expression_memory();
    let remembered_before = remembered(&application)
        .into_iter()
        .map(|(model, expression)| (model.to_owned(), expression.to_owned()))
        .collect::<Vec<_>>();

    application
        .set_expression("live2d_expression0.exp3.json")
        .expect("trigger the same expression again");
    assert_eq!(
        settled_expression(&application, false),
        None,
        "the second trigger turned the expression off"
    );
    application.persist_user_expression_memory();
    let remembered_after = remembered(&application)
        .into_iter()
        .map(|(model, expression)| (model.to_owned(), expression.to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        remembered_after, remembered_before,
        "turning an expression off is not a new remembered choice"
    );

    application
        .set_toggle_repeated_expression(false)
        .expect("disable the toggle");
    application
        .set_expression("live2d_expression0.exp3.json")
        .expect("choose an expression again");
    assert_eq!(
        settled_expression(&application, true).as_deref(),
        Some("live2d_expression0.exp3.json")
    );
    application
        .set_expression("live2d_expression0.exp3.json")
        .expect("and trigger it again with the toggle off");
    assert_eq!(
        settled_expression(&application, true).as_deref(),
        Some("live2d_expression0.exp3.json"),
        "with the toggle off a repeat is the same face applied again"
    );
}

#[test]
fn motion_overlap_setting_is_persisted_and_restored_at_startup() {
    let base = tempdir().expect("temp directory");
    let layout = StorageLayout::under(base.path(), BUILD_ENVIRONMENT);
    let (mut application, pump, _token) = start_with_pumped_overlay(&layout);
    assert!(!application.config().model.allow_motion_overlap);
    application
        .set_allow_motion_overlap(true)
        .expect("enable overlap");
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    while !application
        .runtime_client()
        .snapshot()
        .model_settings
        .allow_motion_overlap
    {
        assert!(Instant::now() < deadline, "runtime setting delivery");
        std::thread::yield_now();
    }
    application.shutdown().expect("shutdown");
    drop(pump);
    let (mut restarted, _pump, _token) = start_with_pumped_overlay(&layout);
    assert!(restarted.config().model.allow_motion_overlap);
    let deadline = Instant::now() + RUNTIME_TIMEOUT;
    while !restarted
        .runtime_client()
        .snapshot()
        .model_settings
        .allow_motion_overlap
    {
        assert!(Instant::now() < deadline, "runtime startup setting");
        std::thread::yield_now();
    }
    restarted
        .set_allow_motion_overlap(false)
        .expect("disable overlap");
    assert!(!restarted.config().model.allow_motion_overlap);
    restarted.shutdown().expect("shutdown");
}
