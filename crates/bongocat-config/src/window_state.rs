#[cfg(test)]
use super::BuildEnvironment;
use super::{StorageLayout, WriterLock};
use bongocat_storage::{create_private_dir_all, set_private_file, write_private_atomic};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::{OpenOptions, TryLockError},
    io::{self, ErrorKind},
};

pub const WINDOW_STATE_SCHEMA_VERSION: u32 = 1;
pub const WINDOW_STATE_WRITER_LOCK_FILE_NAME: &str = "window-state.writer.lock";
const MIN_WINDOW_WIDTH: u32 = 640;
const MIN_WINDOW_HEIGHT: u32 = 480;
const MIN_OVERLAY_DIMENSION: u32 = 64;
const MAX_WINDOW_DIMENSION: u32 = 16_384;
const MAX_WINDOW_COORDINATE: i32 = 1_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowPlacement {
    #[schemars(range(min = -1_000_000, max = 1_000_000))]
    pub x: i32,
    #[schemars(range(min = -1_000_000, max = 1_000_000))]
    pub y: i32,
    #[schemars(range(min = 640, max = 16_384))]
    pub width: u32,
    #[schemars(range(min = 480, max = 16_384))]
    pub height: u32,
    pub maximized: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OverlayWindowPlacement {
    #[schemars(range(min = -1_000_000, max = 1_000_000))]
    pub x: i32,
    #[schemars(range(min = -1_000_000, max = 1_000_000))]
    pub y: i32,
    #[schemars(range(min = 64, max = 16_384))]
    pub width: u32,
    #[schemars(range(min = 64, max = 16_384))]
    pub height: u32,
}

impl OverlayWindowPlacement {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self, WindowStateError> {
        let placement = Self {
            x,
            y,
            width,
            height,
        };
        placement.validate()?;
        Ok(placement)
    }

    fn validate(self) -> Result<(), WindowStateError> {
        if !(-MAX_WINDOW_COORDINATE..=MAX_WINDOW_COORDINATE).contains(&self.x) {
            return Err(WindowStateError::InvalidValue("overlay_window.x"));
        }
        if !(-MAX_WINDOW_COORDINATE..=MAX_WINDOW_COORDINATE).contains(&self.y) {
            return Err(WindowStateError::InvalidValue("overlay_window.y"));
        }
        if !(MIN_OVERLAY_DIMENSION..=MAX_WINDOW_DIMENSION).contains(&self.width) {
            return Err(WindowStateError::InvalidValue("overlay_window.width"));
        }
        if !(MIN_OVERLAY_DIMENSION..=MAX_WINDOW_DIMENSION).contains(&self.height) {
            return Err(WindowStateError::InvalidValue("overlay_window.height"));
        }
        Ok(())
    }
}

impl WindowPlacement {
    pub fn new(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        maximized: bool,
    ) -> Result<Self, WindowStateError> {
        let placement = Self {
            x,
            y,
            width,
            height,
            maximized,
        };
        placement.validate()?;
        Ok(placement)
    }

    fn validate(self) -> Result<(), WindowStateError> {
        if !(-MAX_WINDOW_COORDINATE..=MAX_WINDOW_COORDINATE).contains(&self.x) {
            return Err(WindowStateError::InvalidValue("settings_window.x"));
        }
        if !(-MAX_WINDOW_COORDINATE..=MAX_WINDOW_COORDINATE).contains(&self.y) {
            return Err(WindowStateError::InvalidValue("settings_window.y"));
        }
        if !(MIN_WINDOW_WIDTH..=MAX_WINDOW_DIMENSION).contains(&self.width) {
            return Err(WindowStateError::InvalidValue("settings_window.width"));
        }
        if !(MIN_WINDOW_HEIGHT..=MAX_WINDOW_DIMENSION).contains(&self.height) {
            return Err(WindowStateError::InvalidValue("settings_window.height"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowState {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub settings_window: Option<WindowPlacement>,
    pub overlay_window: Option<OverlayWindowPlacement>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            settings_window: None,
            overlay_window: None,
        }
    }
}

impl WindowState {
    pub fn with_settings_window(settings_window: Option<WindowPlacement>) -> Self {
        Self {
            settings_window,
            ..Self::default()
        }
    }

    pub fn with_windows(
        settings_window: Option<WindowPlacement>,
        overlay_window: Option<OverlayWindowPlacement>,
    ) -> Self {
        Self {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            settings_window,
            overlay_window,
        }
    }

    fn validate(&self) -> Result<(), WindowStateError> {
        if self.schema_version != WINDOW_STATE_SCHEMA_VERSION {
            return Err(WindowStateError::UnsupportedSchema(u64::from(
                self.schema_version,
            )));
        }
        if let Some(placement) = self.settings_window {
            placement.validate()?;
        }
        if let Some(placement) = self.overlay_window {
            placement.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowStateLoadStatus {
    Loaded,
    Missing,
    IgnoredInvalid,
    IgnoredUnsupportedSchema(u64),
    IgnoredIo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowStateLoadOutcome {
    pub state: WindowState,
    pub status: WindowStateLoadStatus,
}

#[derive(Debug, thiserror::Error)]
pub enum WindowStateError {
    #[error("window state I/O failed: {0}")]
    Io(io::Error),
    #[error("window state JSON failed: {0}")]
    Json(serde_json::Error),
    #[error("window state writer lock is unavailable")]
    LockUnavailable,
    #[error("unsupported window state schema_version {0}")]
    UnsupportedSchema(u64),
    #[error("invalid window state value: {0}")]
    InvalidValue(&'static str),
    #[error("window state commit failed verification")]
    VerificationFailed,
}

impl From<io::Error> for WindowStateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for WindowStateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InjectedWindowStateWriteFailure {
    VerificationCorruption,
}

pub struct WindowStateStore {
    layout: StorageLayout,
    #[cfg(test)]
    injected_write_failure: Option<InjectedWindowStateWriteFailure>,
}

impl WindowStateStore {
    pub fn new(layout: StorageLayout) -> Self {
        Self {
            layout,
            #[cfg(test)]
            injected_write_failure: None,
        }
    }

    pub fn load_or_default(&self) -> WindowStateLoadOutcome {
        match fs::read(&self.layout.window_state) {
            Ok(bytes) => match parse_window_state(&bytes) {
                Ok(state) => WindowStateLoadOutcome {
                    state,
                    status: WindowStateLoadStatus::Loaded,
                },
                Err(WindowStateError::UnsupportedSchema(version)) => WindowStateLoadOutcome {
                    state: WindowState::default(),
                    status: WindowStateLoadStatus::IgnoredUnsupportedSchema(version),
                },
                Err(WindowStateError::Json(_) | WindowStateError::InvalidValue(_)) => {
                    WindowStateLoadOutcome {
                        state: WindowState::default(),
                        status: WindowStateLoadStatus::IgnoredInvalid,
                    }
                }
                Err(
                    WindowStateError::Io(_)
                    | WindowStateError::LockUnavailable
                    | WindowStateError::VerificationFailed,
                ) => WindowStateLoadOutcome {
                    state: WindowState::default(),
                    status: WindowStateLoadStatus::IgnoredIo,
                },
            },
            Err(error) if error.kind() == ErrorKind::NotFound => WindowStateLoadOutcome {
                state: WindowState::default(),
                status: WindowStateLoadStatus::Missing,
            },
            Err(_) => WindowStateLoadOutcome {
                state: WindowState::default(),
                status: WindowStateLoadStatus::IgnoredIo,
            },
        }
    }

    pub fn commit(&self, state: &WindowState) -> Result<(), WindowStateError> {
        state.validate()?;
        create_private_dir_all(&self.layout.root)?;
        create_private_dir_all(&self.layout.locks)?;
        let _lock = self.acquire_writer_lock()?;
        if let Ok(current) = fs::read(&self.layout.window_state)
            && let Err(WindowStateError::UnsupportedSchema(version)) = parse_window_state(&current)
        {
            return Err(WindowStateError::UnsupportedSchema(version));
        }
        let bytes = serde_json::to_vec_pretty(state)?;
        let previous = match fs::read(&self.layout.window_state) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        write_private_atomic(&self.layout.window_state, &bytes)?;
        #[cfg(test)]
        if self.injected_write_failure
            == Some(InjectedWindowStateWriteFailure::VerificationCorruption)
        {
            fs::write(
                &self.layout.window_state,
                b"corrupt-after-window-state-replace",
            )?;
        }
        let verified = fs::read(&self.layout.window_state)
            .map_err(WindowStateError::from)
            .and_then(|bytes| parse_window_state(&bytes));
        if !matches!(verified, Ok(ref verified_state) if verified_state == state) {
            restore_window_state_bytes(&self.layout.window_state, previous.as_deref())?;
            return Err(WindowStateError::VerificationFailed);
        }
        Ok(())
    }

    fn acquire_writer_lock(&self) -> Result<WriterLock, WindowStateError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.layout.locks.join(WINDOW_STATE_WRITER_LOCK_FILE_NAME))?;
        set_private_file(&file)?;
        match file.try_lock() {
            Ok(()) => Ok(WriterLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(WindowStateError::LockUnavailable),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    #[cfg(test)]
    fn inject_write_failure(&mut self, failure: InjectedWindowStateWriteFailure) {
        self.injected_write_failure = Some(failure);
    }
}

fn parse_window_state(bytes: &[u8]) -> Result<WindowState, WindowStateError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(WindowStateError::InvalidValue("schema_version"))?;
    if schema_version != u64::from(WINDOW_STATE_SCHEMA_VERSION) {
        return Err(WindowStateError::UnsupportedSchema(schema_version));
    }
    let state: WindowState = serde_json::from_value(value)?;
    state.validate()?;
    Ok(state)
}

fn restore_window_state_bytes(
    path: &std::path::Path,
    previous: Option<&[u8]>,
) -> Result<(), WindowStateError> {
    match previous {
        Some(bytes) => write_private_atomic(path, bytes).map_err(WindowStateError::from),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::{collections::BTreeSet, fs::File};
    use tempfile::TempDir;

    fn window_state_store(root: &TempDir, environment: BuildEnvironment) -> WindowStateStore {
        WindowStateStore::new(StorageLayout::under(root.path(), environment))
    }

    fn placement(x: i32, y: i32) -> WindowPlacement {
        WindowPlacement::new(x, y, 800, 600, false).expect("valid placement")
    }

    fn window_state_fixture(name: &str) -> Vec<u8> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repository root");
        fs::read(
            repository
                .join("shared/config/window-state-fixtures")
                .join(name),
        )
        .expect("window state fixture")
    }

    #[test]
    fn rust_window_state_contract_matches_shared_accept_and_reject_fixtures() {
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
            "../../../shared/config/window-state-fixtures/manifest.json"
        ))
        .expect("window state fixture manifest");
        assert_eq!(manifest.schema_version, 1);
        let mut declared_files = BTreeSet::new();
        for case in manifest.cases {
            assert!(
                declared_files.insert(case.file.clone()),
                "duplicate fixture {}",
                case.file
            );
            let result = parse_window_state(&window_state_fixture(&case.file));
            match case.expected.as_str() {
                "accept" => assert!(result.is_ok(), "fixture {} must be accepted", case.file),
                "reject" => assert!(result.is_err(), "fixture {} must be rejected", case.file),
                expected => panic!("fixture {} has unknown expectation {expected}", case.file),
            }
        }
    }

    #[test]
    fn missing_corrupt_and_future_window_state_fall_back_without_touching_config() {
        let root = TempDir::new().expect("tempdir");
        let store = window_state_store(&root, BuildEnvironment::Development);
        assert_eq!(
            store.load_or_default().status,
            WindowStateLoadStatus::Missing
        );

        fs::create_dir_all(&store.layout.root).expect("state root");
        fs::write(&store.layout.config, b"config-sentinel").expect("config sentinel");
        fs::write(&store.layout.window_state, b"not-json").expect("corrupt state");
        assert_eq!(
            store.load_or_default().status,
            WindowStateLoadStatus::IgnoredInvalid
        );
        assert_eq!(
            fs::read(&store.layout.config).expect("config preserved"),
            b"config-sentinel"
        );

        fs::write(
            &store.layout.window_state,
            br#"{"schema_version":2,"settings_window":null,"overlay_window":null}"#,
        )
        .expect("future state");
        assert_eq!(
            store.load_or_default().status,
            WindowStateLoadStatus::IgnoredUnsupportedSchema(2)
        );
        assert!(matches!(
            store.commit(&WindowState::default()),
            Err(WindowStateError::UnsupportedSchema(2))
        ));

        let oversized_future = br#"{"schema_version":4294967296,"settings_window":null}"#;
        fs::write(&store.layout.window_state, oversized_future).expect("oversized future state");
        assert_eq!(
            store.load_or_default().status,
            WindowStateLoadStatus::IgnoredUnsupportedSchema(4_294_967_296)
        );
        assert!(matches!(
            store.commit(&WindowState::default()),
            Err(WindowStateError::UnsupportedSchema(4_294_967_296))
        ));
        assert_eq!(
            fs::read(&store.layout.window_state).expect("oversized future state preserved"),
            oversized_future
        );
    }

    #[test]
    fn placement_validation_rejects_unbounded_coordinates_and_dimensions() {
        assert!(matches!(
            WindowPlacement::new(MAX_WINDOW_COORDINATE + 1, 0, 800, 600, false),
            Err(WindowStateError::InvalidValue("settings_window.x"))
        ));
        assert!(matches!(
            WindowPlacement::new(0, 0, MIN_WINDOW_WIDTH - 1, 600, false),
            Err(WindowStateError::InvalidValue("settings_window.width"))
        ));
        assert!(matches!(
            WindowPlacement::new(0, 0, 800, MAX_WINDOW_DIMENSION + 1, false),
            Err(WindowStateError::InvalidValue("settings_window.height"))
        ));
        assert!(matches!(
            OverlayWindowPlacement::new(0, 0, MIN_OVERLAY_DIMENSION - 1, 600),
            Err(WindowStateError::InvalidValue("overlay_window.width"))
        ));
    }

    #[test]
    fn commit_is_atomic_verified_and_restart_readable() {
        let root = TempDir::new().expect("tempdir");
        let store = window_state_store(&root, BuildEnvironment::Development);
        let first = WindowState::with_settings_window(Some(placement(-120, 48)));
        store.commit(&first).expect("first commit");
        assert_eq!(store.load_or_default().state, first);

        let original = fs::read(&store.layout.window_state).expect("original state");
        let mut faulting = window_state_store(&root, BuildEnvironment::Development);
        faulting.inject_write_failure(InjectedWindowStateWriteFailure::VerificationCorruption);
        let second = WindowState::with_settings_window(Some(placement(240, 180)));
        assert!(matches!(
            faulting.commit(&second),
            Err(WindowStateError::VerificationFailed)
        ));
        assert_eq!(
            fs::read(&faulting.layout.window_state).expect("restored state"),
            original
        );
        assert_eq!(
            window_state_store(&root, BuildEnvironment::Development)
                .load_or_default()
                .state,
            first
        );
    }

    #[test]
    fn environments_use_independent_window_state_and_writer_locks() {
        let root = TempDir::new().expect("tempdir");
        let development = window_state_store(&root, BuildEnvironment::Development);
        let production = window_state_store(&root, BuildEnvironment::Production);
        let development_state = WindowState::with_settings_window(Some(placement(-300, 40)));
        let production_state = WindowState::with_settings_window(Some(placement(900, 60)));
        development
            .commit(&development_state)
            .expect("development commit");
        production
            .commit(&production_state)
            .expect("production commit");

        assert_eq!(development.load_or_default().state, development_state);
        assert_eq!(production.load_or_default().state, production_state);
        assert_ne!(
            development.layout.window_state,
            production.layout.window_state
        );
        assert_ne!(
            development.layout.locks.join("window-state.writer.lock"),
            production.layout.locks.join("window-state.writer.lock")
        );
    }

    #[test]
    fn concurrent_writer_is_rejected_without_changing_window_state() {
        let root = TempDir::new().expect("tempdir");
        let store = window_state_store(&root, BuildEnvironment::Development);
        let original = WindowState::with_settings_window(Some(placement(10, 20)));
        store.commit(&original).expect("initial commit");

        let lock_path = store.layout.locks.join("window-state.writer.lock");
        let lock = File::options()
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("state lock");
        lock.lock().expect("hold state lock");
        assert!(matches!(
            store.commit(&WindowState::with_settings_window(Some(placement(30, 40)))),
            Err(WindowStateError::LockUnavailable)
        ));
        assert_eq!(store.load_or_default().state, original);
    }

    #[cfg(unix)]
    #[test]
    fn window_state_storage_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("tempdir");
        let store = window_state_store(&root, BuildEnvironment::Development);
        store
            .commit(&WindowState::with_settings_window(Some(placement(10, 20))))
            .expect("state commit");
        assert_eq!(
            fs::metadata(&store.layout.root)
                .expect("root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&store.layout.locks)
                .expect("locks metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for path in [
            store.layout.window_state.clone(),
            store.layout.locks.join("window-state.writer.lock"),
        ] {
            assert_eq!(
                fs::metadata(path)
                    .expect("state file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
