//! Test fixtures shared by the runtime test modules.
//!
//! The tests live in this directory rather than beside the code they cover, so a
//! production module holds only production code. The fixtures here are the parts
//! every runtime test needs: a clock it drives by hand, a model package it can
//! point at, and the two waits a render-driven test shares.

use crate::owner::ShutdownSignal;
use crate::pacing::{record_work_budget, runtime_tick_work_budget};
use crate::transport::{
    CommandSequenceDisposition, CommandSequenceTracker, CommandTransportCounters, Producer,
    RuntimeInputSubmitter, ShutdownDiagnosticsCounters, WorkerCommand, sequence_reached,
};
use crate::worker::renderer::update_renderer_health;
use crate::*;
use bongocat_model::{CommittedModel, ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_model_store::ModelStore;
use bongocat_render::{
    ModelCommitErrorCode, ModelCommitFeedback, ModelCommitOutcome, RenderConsumer, RenderFrame,
    RenderSnapshot,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tempfile::tempdir;

mod input;
mod lifecycle;
mod model;
mod motions;
mod pacing;
mod rendering;
mod transport;

const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct ManualClock {
    now: Mutex<Duration>,
}

impl ManualClock {
    fn set(&self, now: Duration) {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = now;
    }
}

impl MonotonicClock for ManualClock {
    fn now(&self) -> Duration {
        *self
            .now
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root")
        .to_owned()
}

fn cursor_sample(x: f64, y: f64, at: u64) -> CursorSample {
    CursorSample::new(
        CursorPosition { x, y },
        CursorViewport {
            origin: CursorPosition { x: 0.0, y: 0.0 },
            width: 100.0,
            height: 100.0,
        },
        MonotonicMillis::new(at),
    )
    .expect("valid cursor sample")
}

fn preset_model(id: &str) -> CommittedModel {
    PresetModelCatalog::open(
        repository_root().join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog")
    .load(&ModelId::parse(id).expect("model id"))
    .expect("preset model")
}

fn preset_model_without_expressions() -> (TempDir, CommittedModel) {
    let catalog = tempdir().expect("temporary preset catalog");
    let source = repository_root().join("resources/models/standard");
    let destination = catalog.path().join("motion-only-model");
    clone_model_tree(&source, &destination);

    let model3_path = destination.join("cat.model3.json");
    let mut model3: serde_json::Value =
        serde_json::from_slice(&fs::read(&model3_path).expect("read copied model3"))
            .expect("parse copied model3");
    model3
        .get_mut("FileReferences")
        .and_then(serde_json::Value::as_object_mut)
        .expect("model3 file references")
        .remove("Expressions");
    fs::write(
        &model3_path,
        serde_json::to_vec_pretty(&model3).expect("serialize motion-only model3"),
    )
    .expect("write motion-only model3");

    let model = PresetModelCatalog::open(catalog.path(), ModelPackageLimits::default())
        .expect("temporary preset catalog")
        .load(&ModelId::parse("motion-only-model").expect("model id"))
        .expect("temporary preset model");
    (catalog, model)
}

fn preset_model_with_motion_fade_out(fade_out_seconds: f64) -> (TempDir, CommittedModel) {
    let catalog = tempdir().expect("temporary preset catalog");
    let source = repository_root().join("resources/models/standard");
    let destination = catalog.path().join("fade-out-model");
    clone_model_tree(&source, &destination);

    let model3_path = destination.join("cat.model3.json");
    let mut model3: serde_json::Value =
        serde_json::from_slice(&fs::read(&model3_path).expect("read copied model3"))
            .expect("parse copied model3");
    let motions = model3
        .get_mut("FileReferences")
        .and_then(|references| references.get_mut("Motions"))
        .and_then(serde_json::Value::as_object_mut)
        .expect("model3 motion groups");
    for group in motions.values_mut() {
        for motion in group.as_array_mut().expect("motion array") {
            motion.as_object_mut().expect("motion object").insert(
                "FadeOutTime".to_owned(),
                serde_json::Value::from(fade_out_seconds),
            );
        }
    }
    fs::write(
        &model3_path,
        serde_json::to_vec_pretty(&model3).expect("serialize copied model3"),
    )
    .expect("write copied model3");

    let model = PresetModelCatalog::open(catalog.path(), ModelPackageLimits::default())
        .expect("temporary preset catalog")
        .load(&ModelId::parse("fade-out-model").expect("model id"))
        .expect("temporary preset model");
    (catalog, model)
}

fn clone_model_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create copied model directory");
    for entry in fs::read_dir(source).expect("read source model directory") {
        let entry = entry.expect("source model entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("source entry type").is_dir() {
            clone_model_tree(&source_path, &destination_path);
        } else if source_path.extension().and_then(|value| value.to_str()) == Some("json") {
            fs::copy(&source_path, &destination_path).expect("copy model JSON");
        } else if fs::hard_link(&source_path, &destination_path).is_err() {
            fs::copy(&source_path, &destination_path).expect("copy model resource");
        }
    }
}

fn wait_for_render_frame(
    consumer: &RenderConsumer,
    predicate: impl Fn(&RenderFrame) -> bool,
) -> RenderFrame {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(frame) = consumer.take_latest()
            && predicate(&frame)
        {
            return frame;
        }
        assert!(Instant::now() < deadline, "render frame timed out");
        thread::sleep(Duration::from_millis(2));
    }
}

fn assert_same_render_content(before: &RenderSnapshot, after: &RenderSnapshot) {
    let mut before = before.clone();
    let mut after = after.clone();
    for drawable in &mut before.drawables {
        drawable.dynamic_flags = Default::default();
    }
    for drawable in &mut after.drawables {
        drawable.dynamic_flags = Default::default();
    }
    assert_eq!(before, after);
}

fn wait_for_prepared_model(
    client: &RuntimeClient,
    consumer: &RenderConsumer,
    command_sequence: u64,
) -> RenderFrame {
    let prepared = client
        .wait_for_model_preparation(command_sequence, TIMEOUT)
        .expect("model prepared");
    assert_eq!(prepared.last_command_failure, None);
    let frame = wait_for_render_frame(consumer, |frame| {
        frame
            .model_commit
            .is_some_and(|token| token.command_sequence == command_sequence)
    });
    assert_eq!(
        prepared.pending_model.as_ref().map(|pending| pending.token),
        frame.model_commit
    );
    frame
}

fn report_model_prepared(
    client: &RuntimeClient,
    consumer: &RenderConsumer,
    frame: &RenderFrame,
) -> RuntimeSnapshot {
    let token = frame.model_commit.expect("model commit token");
    consumer
        .report_model_commit(ModelCommitFeedback {
            token,
            outcome: ModelCommitOutcome::Prepared,
        })
        .expect("report prepared model");
    client
        .wait_for_command(token.command_sequence, TIMEOUT)
        .expect("model committed")
}
