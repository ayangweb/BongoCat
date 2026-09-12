#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const SCHEMA_VERSION: u32 = 1;
const GAMEPAD_BUTTON_THRESHOLD: f64 = 0.5;
const SNAPSHOT_PRECISION: f64 = 1_000_000.0;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    pub sequences: usize,
    pub events: usize,
    pub checkpoints: usize,
    pub audio_triggers: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotDifference {
    pub path: String,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
}

#[derive(Debug)]
pub enum RunnerError {
    Io {
        path: PathBuf,
        message: String,
    },
    Json {
        path: PathBuf,
        message: String,
    },
    InvalidFixture {
        sequence_id: String,
        message: String,
    },
    PairMismatch {
        missing_expected: Vec<String>,
        orphan_expected: Vec<String>,
    },
    SnapshotMismatch {
        sequence_id: String,
        at_ms: u64,
        differences: Vec<SnapshotDifference>,
    },
}

impl Display for RunnerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => write!(formatter, "{}: {message}", path.display()),
            Self::Json { path, message } => {
                write!(formatter, "{}: invalid JSON: {message}", path.display())
            }
            Self::InvalidFixture {
                sequence_id,
                message,
            } => write!(formatter, "fixture {sequence_id}: {message}"),
            Self::PairMismatch {
                missing_expected,
                orphan_expected,
            } => write!(
                formatter,
                "fixture pairs differ: missing_expected={missing_expected:?} orphan_expected={orphan_expected:?}"
            ),
            Self::SnapshotMismatch {
                sequence_id,
                at_ms,
                differences,
            } => {
                write!(
                    formatter,
                    "fixture {sequence_id} checkpoint {at_ms}ms mismatch"
                )?;
                for difference in differences {
                    write!(
                        formatter,
                        "\n  {}: expected={} actual={}",
                        difference.path,
                        display_value(difference.expected.as_ref()),
                        display_value(difference.actual.as_ref())
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl Error for RunnerError {}

fn display_value(value: Option<&Value>) -> String {
    value.map_or_else(|| "<missing>".to_owned(), Value::to_string)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InputSequence {
    schema_version: u32,
    id: String,
    description: String,
    context: FixtureContext,
    events: Vec<FixtureEvent>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureContext {
    model_mode: ModelMode,
    key_sides: BTreeMap<String, Side>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModelMode {
    Standard,
    Keyboard,
    Gamepad,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum InputSource {
    Capture,
    Reconciliation,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButton {
    const fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
            Self::Back => "back",
            Self::Forward => "forward",
        }
    }

    const fn parameter(self) -> &'static str {
        match self {
            Self::Left => "ParamMouseLeftDown",
            Self::Right => "ParamMouseRightDown",
            Self::Middle => "ParamMouseMiddleDown",
            Self::Back => "ParamMouseBackDown",
            Self::Forward => "ParamMouseForwardDown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DeviceKind {
    Keyboard,
    Mouse,
    Gamepad,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ResetReason {
    SessionLock,
    Sleep,
    DeviceRemoved,
    ServiceRestart,
    QueueOverflow,
    PermissionChanged,
    Test,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MotionPriority {
    Idle,
    Normal,
    Force,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum FixtureEvent {
    KeyDown {
        at_ms: u64,
        key: String,
        repeat: bool,
        source: InputSource,
    },
    KeyUp {
        at_ms: u64,
        key: String,
        source: InputSource,
    },
    MouseDown {
        at_ms: u64,
        button: MouseButton,
        source: InputSource,
    },
    MouseUp {
        at_ms: u64,
        button: MouseButton,
        source: InputSource,
    },
    CursorMoved {
        at_ms: u64,
        position: Point,
    },
    GamepadButton {
        at_ms: u64,
        device_id: String,
        button: String,
        value: f64,
    },
    GamepadAxis {
        at_ms: u64,
        device_id: String,
        axis: String,
        value: f64,
    },
    DeviceConnected {
        at_ms: u64,
        device_id: String,
        device_kind: DeviceKind,
    },
    DeviceDisconnected {
        at_ms: u64,
        device_id: String,
        device_kind: DeviceKind,
    },
    Reset {
        at_ms: u64,
        reason: ResetReason,
    },
    MotionStart {
        at_ms: u64,
        motion_id: String,
        priority: MotionPriority,
    },
    MotionStop {
        at_ms: u64,
        motion_id: String,
    },
    ExpressionSet {
        at_ms: u64,
        expression_id: String,
    },
    ModelSwitch {
        at_ms: u64,
        model_id: String,
    },
    AudioTrigger {
        at_ms: u64,
        cue_id: String,
    },
}

impl FixtureEvent {
    const fn at_ms(&self) -> u64 {
        match self {
            Self::KeyDown { at_ms, .. }
            | Self::KeyUp { at_ms, .. }
            | Self::MouseDown { at_ms, .. }
            | Self::MouseUp { at_ms, .. }
            | Self::CursorMoved { at_ms, .. }
            | Self::GamepadButton { at_ms, .. }
            | Self::GamepadAxis { at_ms, .. }
            | Self::DeviceConnected { at_ms, .. }
            | Self::DeviceDisconnected { at_ms, .. }
            | Self::Reset { at_ms, .. }
            | Self::MotionStart { at_ms, .. }
            | Self::MotionStop { at_ms, .. }
            | Self::ExpressionSet { at_ms, .. }
            | Self::ModelSwitch { at_ms, .. }
            | Self::AudioTrigger { at_ms, .. } => *at_ms,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExpectedFixture {
    schema_version: u32,
    sequence_id: String,
    provenance: Provenance,
    checkpoints: Vec<Checkpoint>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Provenance {
    LegacyObservation,
    ProductDecision,
    BugFix,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    at_ms: u64,
    input: InputSnapshot,
    model: ModelSnapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InputSnapshot {
    pressed_keys: Vec<String>,
    pressed_mouse_buttons: Vec<String>,
    connected_devices: Vec<String>,
    last_reset_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor_position: Option<Point>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_model_id: Option<String>,
    left_hand_down: bool,
    right_hand_down: bool,
    parameters: BTreeMap<String, f64>,
    active_motion: Option<String>,
    active_expression: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TrackedParameter {
    LeftHand,
    RightHand,
    Mouse(MouseButton),
    GamepadButton { key: String, button: String },
}

impl TrackedParameter {
    fn name(&self) -> String {
        match self {
            Self::LeftHand => "CatParamLeftHandDown".to_owned(),
            Self::RightHand => "CatParamRightHandDown".to_owned(),
            Self::Mouse(button) => button.parameter().to_owned(),
            Self::GamepadButton { key, .. } => format!("{key}Down"),
        }
    }
}

#[derive(Default)]
struct RuntimeState {
    pressed_keys: BTreeSet<String>,
    pressed_mouse_buttons: BTreeSet<MouseButton>,
    connected_devices: BTreeMap<String, DeviceKind>,
    gamepad_buttons: BTreeSet<(String, String)>,
    gamepad_axes: BTreeMap<(String, String), f64>,
    cursor_position: Option<Point>,
    last_reset_reason: Option<ResetReason>,
    selected_model_id: Option<String>,
    active_motion: Option<String>,
    motion_priority: Option<MotionPriority>,
    active_expression: Option<String>,
    audio_trigger_count: u64,
}

impl RuntimeState {
    fn apply(&mut self, event: &FixtureEvent, sequence_id: &str) -> Result<(), RunnerError> {
        match event {
            FixtureEvent::KeyDown {
                key,
                repeat,
                source,
                ..
            } => {
                if !matches!(source, InputSource::Capture) {
                    return invalid(sequence_id, "key_down must come from capture");
                }
                if !*repeat || self.pressed_keys.contains(key) {
                    self.pressed_keys.insert(key.clone());
                } else {
                    return invalid(
                        sequence_id,
                        "repeat key_down arrived before the initial edge",
                    );
                }
            }
            FixtureEvent::KeyUp { key, source, .. } => {
                let _reconciled = matches!(source, InputSource::Reconciliation);
                self.pressed_keys.remove(key);
            }
            FixtureEvent::MouseDown { button, source, .. } => {
                let _captured = matches!(source, InputSource::Capture);
                self.pressed_mouse_buttons.insert(*button);
            }
            FixtureEvent::MouseUp { button, source, .. } => {
                let _reconciled = matches!(source, InputSource::Reconciliation);
                self.pressed_mouse_buttons.remove(button);
            }
            FixtureEvent::CursorMoved { position, .. } => {
                if !position.x.is_finite() || !position.y.is_finite() {
                    return invalid(sequence_id, "cursor position must be finite");
                }
                self.cursor_position = Some(*position);
            }
            FixtureEvent::GamepadButton {
                device_id,
                button,
                value,
                ..
            } => {
                self.require_gamepad(sequence_id, device_id)?;
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    return invalid(sequence_id, "gamepad button value is outside [0, 1]");
                }
                let key = (device_id.clone(), button.clone());
                if *value >= GAMEPAD_BUTTON_THRESHOLD {
                    self.gamepad_buttons.insert(key);
                } else {
                    self.gamepad_buttons.remove(&key);
                }
            }
            FixtureEvent::GamepadAxis {
                device_id,
                axis,
                value,
                ..
            } => {
                self.require_gamepad(sequence_id, device_id)?;
                if !value.is_finite() || !(-1.0..=1.0).contains(value) {
                    return invalid(sequence_id, "gamepad axis value is outside [-1, 1]");
                }
                self.gamepad_axes
                    .insert((device_id.clone(), axis.clone()), *value);
            }
            FixtureEvent::DeviceConnected {
                device_id,
                device_kind,
                ..
            } => {
                if self
                    .connected_devices
                    .insert(device_id.clone(), *device_kind)
                    .is_some()
                {
                    return invalid(sequence_id, format!("device {device_id} connected twice"));
                }
            }
            FixtureEvent::DeviceDisconnected {
                device_id,
                device_kind,
                ..
            } => {
                let Some(connected_kind) = self.connected_devices.remove(device_id) else {
                    return invalid(
                        sequence_id,
                        format!("device {device_id} disconnected before connect"),
                    );
                };
                if connected_kind != *device_kind {
                    return invalid(
                        sequence_id,
                        format!("device {device_id} kind changed before disconnect"),
                    );
                }
                self.gamepad_buttons
                    .retain(|(connected_id, _)| connected_id != device_id);
                self.gamepad_axes
                    .retain(|(connected_id, _), _| connected_id != device_id);
            }
            FixtureEvent::Reset { reason, .. } => {
                self.pressed_keys.clear();
                self.pressed_mouse_buttons.clear();
                self.gamepad_buttons.clear();
                self.gamepad_axes.clear();
                self.cursor_position = None;
                self.last_reset_reason = Some(*reason);
            }
            FixtureEvent::MotionStart {
                motion_id,
                priority,
                ..
            } => {
                if self
                    .motion_priority
                    .is_none_or(|active| *priority >= active)
                {
                    self.active_motion = Some(motion_id.clone());
                    self.motion_priority = Some(*priority);
                }
            }
            FixtureEvent::MotionStop { motion_id, .. } => {
                if self.active_motion.as_ref() == Some(motion_id) {
                    self.active_motion = None;
                    self.motion_priority = None;
                }
            }
            FixtureEvent::ExpressionSet { expression_id, .. } => {
                self.active_expression = Some(expression_id.clone());
            }
            FixtureEvent::ModelSwitch { model_id, .. } => {
                self.selected_model_id = Some(model_id.clone());
                self.active_motion = None;
                self.motion_priority = None;
                self.active_expression = None;
            }
            FixtureEvent::AudioTrigger { cue_id, .. } => {
                if cue_id.is_empty() {
                    return invalid(sequence_id, "audio cue id must not be empty");
                }
                self.audio_trigger_count = self.audio_trigger_count.saturating_add(1);
            }
        }
        Ok(())
    }

    fn require_gamepad(&self, sequence_id: &str, device_id: &str) -> Result<(), RunnerError> {
        if self.connected_devices.get(device_id) == Some(&DeviceKind::Gamepad) {
            Ok(())
        } else {
            invalid(
                sequence_id,
                format!("gamepad event references disconnected device {device_id}"),
            )
        }
    }

    fn snapshot(
        &self,
        context: &FixtureContext,
        tracked: &BTreeSet<TrackedParameter>,
    ) -> (InputSnapshot, ModelSnapshot) {
        let left_hand_down = self.hand_down(context, Side::Left);
        let right_hand_down = self.hand_down(context, Side::Right);
        let parameters = tracked
            .iter()
            .map(|parameter| {
                let value = match parameter {
                    TrackedParameter::LeftHand => left_hand_down,
                    TrackedParameter::RightHand => right_hand_down,
                    TrackedParameter::Mouse(button) => self.pressed_mouse_buttons.contains(button),
                    TrackedParameter::GamepadButton { button, .. } => self
                        .gamepad_buttons
                        .iter()
                        .any(|(_, pressed_button)| pressed_button == button),
                };
                (parameter.name(), normalize_number(f64::from(value)))
            })
            .collect();
        let input = InputSnapshot {
            pressed_keys: self.pressed_keys.iter().cloned().collect(),
            pressed_mouse_buttons: self
                .pressed_mouse_buttons
                .iter()
                .map(|button| button.name().to_owned())
                .collect(),
            connected_devices: self.connected_devices.keys().cloned().collect(),
            last_reset_reason: self
                .last_reset_reason
                .map(reset_reason_name)
                .map(str::to_owned),
            cursor_position: self.cursor_position.map(|point| Point {
                x: normalize_number(point.x),
                y: normalize_number(point.y),
            }),
        };
        let model = ModelSnapshot {
            selected_model_id: self.selected_model_id.clone(),
            left_hand_down,
            right_hand_down,
            parameters,
            active_motion: self.active_motion.clone(),
            active_expression: self.active_expression.clone(),
        };
        (input, model)
    }

    fn hand_down(&self, context: &FixtureContext, side: Side) -> bool {
        self.pressed_keys
            .iter()
            .any(|key| context.key_sides.get(key) == Some(&side))
            || self
                .gamepad_buttons
                .iter()
                .any(|(_, button)| context.key_sides.get(&gamepad_key(button)) == Some(&side))
    }
}

pub fn run_fixture_directory(root: &Path) -> Result<RunReport, RunnerError> {
    let input_dir = root.join("input-sequences");
    let expected_dir = root.join("expected-state");
    let input_files = fixture_files(&input_dir)?;
    let expected_files = fixture_files(&expected_dir)?;
    let input_ids = input_files.keys().cloned().collect::<BTreeSet<_>>();
    let expected_ids = expected_files.keys().cloned().collect::<BTreeSet<_>>();
    if input_ids != expected_ids {
        return Err(RunnerError::PairMismatch {
            missing_expected: input_ids.difference(&expected_ids).cloned().collect(),
            orphan_expected: expected_ids.difference(&input_ids).cloned().collect(),
        });
    }
    if input_ids.is_empty() {
        return Err(RunnerError::InvalidFixture {
            sequence_id: "<directory>".to_owned(),
            message: "no fixture pairs found".to_owned(),
        });
    }

    let mut report = RunReport::default();
    for id in input_ids {
        let input_path = &input_files[&id];
        let expected_path = &expected_files[&id];
        let sequence = parse_json::<InputSequence>(input_path)?;
        let expected = parse_json::<ExpectedFixture>(expected_path)?;
        let pair_report = run_fixture(sequence, expected)?;
        report.sequences += 1;
        report.events += pair_report.events;
        report.checkpoints += pair_report.checkpoints;
        report.audio_triggers += pair_report.audio_triggers;
    }
    Ok(report)
}

fn fixture_files(directory: &Path) -> Result<BTreeMap<String, PathBuf>, RunnerError> {
    let entries = fs::read_dir(directory).map_err(|error| RunnerError::Io {
        path: directory.to_owned(),
        message: error.to_string(),
    })?;
    let mut files = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|error| RunnerError::Io {
            path: directory.to_owned(),
            message: error.to_string(),
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json")
            || path.file_name().and_then(|value| value.to_str()) == Some("schema.json")
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        files.insert(stem.to_owned(), path);
    }
    Ok(files)
}

fn parse_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, RunnerError> {
    let bytes = fs::read(path).map_err(|error| RunnerError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| RunnerError::Json {
        path: path.to_owned(),
        message: error.to_string(),
    })
}

fn run_fixture(
    sequence: InputSequence,
    expected: ExpectedFixture,
) -> Result<RunReport, RunnerError> {
    validate_fixture(&sequence, &expected)?;
    let tracked = tracked_parameters(&sequence);
    let mut state = RuntimeState::default();
    let mut event_index = 0usize;
    for checkpoint in &expected.checkpoints {
        while event_index < sequence.events.len()
            && sequence.events[event_index].at_ms() <= checkpoint.at_ms
        {
            state.apply(&sequence.events[event_index], &sequence.id)?;
            event_index += 1;
        }
        let (actual_input, actual_model) = state.snapshot(&sequence.context, &tracked);
        let actual = serde_json::json!({ "input": actual_input, "model": actual_model });
        let wanted = serde_json::json!({ "input": checkpoint.input, "model": checkpoint.model });
        if actual != wanted {
            let mut differences = Vec::new();
            collect_differences("$", &wanted, &actual, &mut differences);
            return Err(RunnerError::SnapshotMismatch {
                sequence_id: sequence.id.clone(),
                at_ms: checkpoint.at_ms,
                differences,
            });
        }
    }
    while event_index < sequence.events.len() {
        state.apply(&sequence.events[event_index], &sequence.id)?;
        event_index += 1;
    }
    Ok(RunReport {
        sequences: 1,
        events: sequence.events.len(),
        checkpoints: expected.checkpoints.len(),
        audio_triggers: state.audio_trigger_count,
    })
}

fn validate_fixture(
    sequence: &InputSequence,
    expected: &ExpectedFixture,
) -> Result<(), RunnerError> {
    if sequence.schema_version != SCHEMA_VERSION || expected.schema_version != SCHEMA_VERSION {
        return invalid(&sequence.id, "schema_version must be 1");
    }
    if sequence.id != expected.sequence_id {
        return invalid(
            &sequence.id,
            format!("expected sequence id is {}", expected.sequence_id),
        );
    }
    if sequence.description.trim().is_empty() {
        return invalid(&sequence.id, "description must not be blank");
    }
    let _mode = sequence.context.model_mode;
    let _provenance = expected.provenance;

    let mut previous = None;
    let event_times = sequence
        .events
        .iter()
        .map(FixtureEvent::at_ms)
        .collect::<BTreeSet<_>>();
    for event in &sequence.events {
        if previous.is_some_and(|at_ms| event.at_ms() < at_ms) {
            return invalid(&sequence.id, "events are not ordered by monotonic at_ms");
        }
        previous = Some(event.at_ms());
    }
    previous = None;
    for checkpoint in &expected.checkpoints {
        if previous.is_some_and(|at_ms| checkpoint.at_ms < at_ms) {
            return invalid(
                &sequence.id,
                "checkpoints are not ordered by monotonic at_ms",
            );
        }
        if !event_times.contains(&checkpoint.at_ms) {
            return invalid(
                &sequence.id,
                format!("checkpoint {}ms is not an event time", checkpoint.at_ms),
            );
        }
        previous = Some(checkpoint.at_ms);
    }
    Ok(())
}

fn tracked_parameters(sequence: &InputSequence) -> BTreeSet<TrackedParameter> {
    let mut tracked = BTreeSet::new();
    for (key, side) in &sequence.context.key_sides {
        if let Some(button) = key.strip_prefix("Gamepad") {
            tracked.insert(TrackedParameter::GamepadButton {
                key: key.clone(),
                button: lower_camel_to_snake(button),
            });
        } else {
            tracked.insert(match side {
                Side::Left => TrackedParameter::LeftHand,
                Side::Right => TrackedParameter::RightHand,
            });
        }
    }
    for event in &sequence.events {
        match event {
            FixtureEvent::MouseDown { button, .. } | FixtureEvent::MouseUp { button, .. } => {
                tracked.insert(TrackedParameter::Mouse(*button));
            }
            _ => {}
        }
    }
    tracked
}

fn lower_camel_to_snake(value: &str) -> String {
    let mut result = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            result.push('_');
        }
        result.push(character.to_ascii_lowercase());
    }
    result
}

fn gamepad_key(button: &str) -> String {
    let mut result = String::from("Gamepad");
    for part in button.split('_') {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            result.push(first.to_ascii_uppercase());
            result.extend(characters);
        }
    }
    result
}

fn reset_reason_name(reason: ResetReason) -> &'static str {
    match reason {
        ResetReason::SessionLock => "session_lock",
        ResetReason::Sleep => "sleep",
        ResetReason::DeviceRemoved => "device_removed",
        ResetReason::ServiceRestart => "service_restart",
        ResetReason::QueueOverflow => "queue_overflow",
        ResetReason::PermissionChanged => "permission_changed",
        ResetReason::Test => "test",
    }
}

fn normalize_number(value: f64) -> f64 {
    (value * SNAPSHOT_PRECISION).round() / SNAPSHOT_PRECISION
}

fn invalid<T>(sequence_id: &str, message: impl Into<String>) -> Result<T, RunnerError> {
    Err(RunnerError::InvalidFixture {
        sequence_id: sequence_id.to_owned(),
        message: message.into(),
    })
}

fn collect_differences(
    path: &str,
    expected: &Value,
    actual: &Value,
    differences: &mut Vec<SnapshotDifference>,
) {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (expected.get(&key), actual.get(&key)) {
                    (Some(expected), Some(actual)) => {
                        collect_differences(&child_path, expected, actual, differences);
                    }
                    (expected, actual) => differences.push(SnapshotDifference {
                        path: child_path,
                        expected: expected.cloned(),
                        actual: actual.cloned(),
                    }),
                }
            }
        }
        _ if expected != actual => differences.push(SnapshotDifference {
            path: path.to_owned(),
            expected: Some(expected.clone()),
            actual: Some(actual.clone()),
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../shared/fixtures")
    }

    #[test]
    fn all_repository_fixtures_match_rust_snapshots() {
        let report = run_fixture_directory(&repository_fixture_root()).unwrap();
        assert_eq!(report.sequences, 9);
        assert_eq!(report.events, 51);
        assert_eq!(report.checkpoints, 24);
        assert_eq!(report.audio_triggers, 1);
    }

    #[test]
    fn gamepad_name_conversion_preserves_canonical_identity() {
        assert_eq!(gamepad_key("left_stick"), "GamepadLeftStick");
        assert_eq!(lower_camel_to_snake("LeftStick"), "left_stick");
    }

    #[test]
    fn field_differences_are_structured_and_stable() {
        let expected = serde_json::json!({"input": {"pressedKeys": ["KeyA"]}});
        let actual = serde_json::json!({"input": {"pressedKeys": []}});
        let mut differences = Vec::new();
        collect_differences("$", &expected, &actual, &mut differences);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].path, "$.input.pressedKeys");
    }

    #[test]
    fn repeat_without_initial_down_is_rejected() {
        let mut state = RuntimeState::default();
        let error = state
            .apply(
                &FixtureEvent::KeyDown {
                    at_ms: 0,
                    key: "KeyA".to_owned(),
                    repeat: true,
                    source: InputSource::Capture,
                },
                "repeat",
            )
            .unwrap_err();
        assert!(error.to_string().contains("before the initial edge"));
    }

    #[test]
    fn disconnected_gamepad_input_is_rejected() {
        let mut state = RuntimeState::default();
        let error = state
            .apply(
                &FixtureEvent::GamepadButton {
                    at_ms: 0,
                    device_id: "pad".to_owned(),
                    button: "south".to_owned(),
                    value: 1.0,
                },
                "gamepad",
            )
            .unwrap_err();
        assert!(error.to_string().contains("disconnected device"));
    }

    #[test]
    fn lower_priority_motion_cannot_replace_active_motion() {
        let mut state = RuntimeState::default();
        state
            .apply(
                &FixtureEvent::MotionStart {
                    at_ms: 0,
                    motion_id: "walk".to_owned(),
                    priority: MotionPriority::Normal,
                },
                "motion",
            )
            .unwrap();
        state
            .apply(
                &FixtureEvent::MotionStart {
                    at_ms: 1,
                    motion_id: "idle".to_owned(),
                    priority: MotionPriority::Idle,
                },
                "motion",
            )
            .unwrap();
        assert_eq!(state.active_motion.as_deref(), Some("walk"));
    }

    #[test]
    fn deleting_a_tracked_parameter_from_expected_snapshot_fails() {
        let root = repository_fixture_root();
        let sequence_path = root
            .join("input-sequences")
            .join("gamepad-reconnect-reset.json");
        let expected_path = root
            .join("expected-state")
            .join("gamepad-reconnect-reset.json");
        let sequence = parse_json::<InputSequence>(&sequence_path).unwrap();
        let mut expected_value = parse_json::<Value>(&expected_path).unwrap();
        expected_value["checkpoints"][0]["model"]["parameters"]
            .as_object_mut()
            .unwrap()
            .remove("GamepadEastDown");
        let expected = serde_json::from_value::<ExpectedFixture>(expected_value).unwrap();

        let error = run_fixture(sequence, expected).unwrap_err();
        let RunnerError::SnapshotMismatch { differences, .. } = error else {
            panic!("expected snapshot mismatch");
        };
        assert!(
            differences
                .iter()
                .any(|difference| difference.path == "$.model.parameters.GamepadEastDown")
        );
    }

    #[test]
    fn unknown_json_fields_are_rejected() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "unknown-field",
            "description": "must fail",
            "context": {"modelMode": "standard", "keySides": {}},
            "events": [],
            "extra": true
        }"#;
        let error = serde_json::from_str::<InputSequence>(json).unwrap_err();
        assert!(error.to_string().contains("unknown field `extra`"));
    }
}
