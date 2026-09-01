#[cfg(test)]
use super::BuildEnvironment;
use super::{StorageLayout, WriterLock, write_atomic_io};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    fs::{OpenOptions, TryLockError},
    io::{self, ErrorKind},
};

pub const STATE_SCHEMA_VERSION: u32 = 1;
const MIN_WINDOW_WIDTH: u32 = 640;
const MIN_WINDOW_HEIGHT: u32 = 480;
const MAX_WINDOW_DIMENSION: u32 = 16_384;
const MAX_WINDOW_COORDINATE: i32 = 1_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl WindowPlacement {
    pub fn new(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        maximized: bool,
    ) -> Result<Self, StateError> {
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

    fn validate(self) -> Result<(), StateError> {
        if !(-MAX_WINDOW_COORDINATE..=MAX_WINDOW_COORDINATE).contains(&self.x) {
            return Err(StateError::InvalidValue("settings_window.x"));
        }
        if !(-MAX_WINDOW_COORDINATE..=MAX_WINDOW_COORDINATE).contains(&self.y) {
            return Err(StateError::InvalidValue("settings_window.y"));
        }
        if !(MIN_WINDOW_WIDTH..=MAX_WINDOW_DIMENSION).contains(&self.width) {
            return Err(StateError::InvalidValue("settings_window.width"));
        }
        if !(MIN_WINDOW_HEIGHT..=MAX_WINDOW_DIMENSION).contains(&self.height) {
            return Err(StateError::InvalidValue("settings_window.height"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationState {
    pub schema_version: u32,
    pub settings_window: Option<WindowPlacement>,
}

impl Default for ApplicationState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            settings_window: None,
        }
    }
}

impl ApplicationState {
    pub fn with_settings_window(settings_window: Option<WindowPlacement>) -> Self {
        Self {
            settings_window,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), StateError> {
        if self.schema_version != STATE_SCHEMA_VERSION {
            return Err(StateError::UnsupportedSchema(u64::from(
                self.schema_version,
            )));
        }
        if let Some(placement) = self.settings_window {
            placement.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateLoadStatus {
    Loaded,
    Missing,
    IgnoredInvalid,
    IgnoredUnsupportedSchema(u64),
    IgnoredIo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateLoadOutcome {
    pub state: ApplicationState,
    pub status: StateLoadStatus,
}

#[derive(Debug)]
pub enum StateError {
    Io(io::Error),
    Json(serde_json::Error),
    LockUnavailable,
    UnsupportedSchema(u64),
    InvalidValue(&'static str),
    VerificationFailed,
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "state I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "state JSON failed: {error}"),
            Self::LockUnavailable => formatter.write_str("state writer lock is unavailable"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported state schema_version {version}")
            }
            Self::InvalidValue(field) => write!(formatter, "invalid state value: {field}"),
            Self::VerificationFailed => formatter.write_str("state commit failed verification"),
        }
    }
}

impl std::error::Error for StateError {}

impl From<io::Error> for StateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InjectedStateWriteFailure {
    VerificationCorruption,
}

pub struct StateStore {
    layout: StorageLayout,
    #[cfg(test)]
    injected_write_failure: Option<InjectedStateWriteFailure>,
}

impl StateStore {
    pub fn new(layout: StorageLayout) -> Self {
        Self {
            layout,
            #[cfg(test)]
            injected_write_failure: None,
        }
    }

    pub fn load_or_default(&self) -> StateLoadOutcome {
        match fs::read(&self.layout.state) {
            Ok(bytes) => match parse_state(&bytes) {
                Ok(state) => StateLoadOutcome {
                    state,
                    status: StateLoadStatus::Loaded,
                },
                Err(StateError::UnsupportedSchema(version)) => StateLoadOutcome {
                    state: ApplicationState::default(),
                    status: StateLoadStatus::IgnoredUnsupportedSchema(version),
                },
                Err(StateError::Json(_) | StateError::InvalidValue(_)) => StateLoadOutcome {
                    state: ApplicationState::default(),
                    status: StateLoadStatus::IgnoredInvalid,
                },
                Err(
                    StateError::Io(_)
                    | StateError::LockUnavailable
                    | StateError::VerificationFailed,
                ) => StateLoadOutcome {
                    state: ApplicationState::default(),
                    status: StateLoadStatus::IgnoredIo,
                },
            },
            Err(error) if error.kind() == ErrorKind::NotFound => StateLoadOutcome {
                state: ApplicationState::default(),
                status: StateLoadStatus::Missing,
            },
            Err(_) => StateLoadOutcome {
                state: ApplicationState::default(),
                status: StateLoadStatus::IgnoredIo,
            },
        }
    }

    pub fn commit(&self, state: &ApplicationState) -> Result<(), StateError> {
        state.validate()?;
        fs::create_dir_all(&self.layout.root)?;
        super::set_private_directory(&self.layout.root)?;
        fs::create_dir_all(&self.layout.locks)?;
        super::set_private_directory(&self.layout.locks)?;
        let _lock = self.acquire_writer_lock()?;
        if let Ok(current) = fs::read(&self.layout.state)
            && let Err(StateError::UnsupportedSchema(version)) = parse_state(&current)
        {
            return Err(StateError::UnsupportedSchema(version));
        }
        let bytes = serde_json::to_vec_pretty(state)?;
        let previous = match fs::read(&self.layout.state) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        write_atomic_io(&self.layout.state, &bytes)?;
        #[cfg(test)]
        if self.injected_write_failure == Some(InjectedStateWriteFailure::VerificationCorruption) {
            fs::write(&self.layout.state, b"corrupt-after-state-replace")?;
        }
        let verified = fs::read(&self.layout.state)
            .map_err(StateError::from)
            .and_then(|bytes| parse_state(&bytes));
        if !matches!(verified, Ok(ref verified_state) if verified_state == state) {
            restore_state_bytes(&self.layout.state, previous.as_deref())?;
            return Err(StateError::VerificationFailed);
        }
        Ok(())
    }

    fn acquire_writer_lock(&self) -> Result<WriterLock, StateError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.layout.locks.join("state.writer.lock"))?;
        super::set_private_file(&file)?;
        match file.try_lock() {
            Ok(()) => Ok(WriterLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(StateError::LockUnavailable),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    #[cfg(test)]
    fn inject_write_failure(&mut self, failure: InjectedStateWriteFailure) {
        self.injected_write_failure = Some(failure);
    }
}

fn parse_state(bytes: &[u8]) -> Result<ApplicationState, StateError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(StateError::InvalidValue("schema_version"))?;
    if schema_version != u64::from(STATE_SCHEMA_VERSION) {
        return Err(StateError::UnsupportedSchema(schema_version));
    }
    let state: ApplicationState = serde_json::from_value(value)?;
    state.validate()?;
    Ok(state)
}

fn restore_state_bytes(path: &std::path::Path, previous: Option<&[u8]>) -> Result<(), StateError> {
    match previous {
        Some(bytes) => write_atomic_io(path, bytes).map_err(StateError::from),
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
    use std::fs::File;
    use std::path::Path;
    use tempfile::TempDir;

    fn state_store(root: &TempDir, environment: BuildEnvironment) -> StateStore {
        StateStore::new(StorageLayout::under(root.path(), environment))
    }

    fn placement(x: i32, y: i32) -> WindowPlacement {
        WindowPlacement::new(x, y, 800, 600, false).expect("valid placement")
    }

    fn state_fixture(name: &str) -> Vec<u8> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        fs::read(repository.join("shared/config/state-fixtures").join(name)).expect("state fixture")
    }

    #[test]
    fn rust_state_contract_matches_shared_accept_and_reject_fixtures() {
        for fixture in ["default.json", "negative-coordinate.json"] {
            parse_state(&state_fixture(fixture)).expect("accepted state fixture");
        }
        for fixture in [
            "invalid-future-schema.json",
            "invalid-out-of-range.json",
            "invalid-unknown-field.json",
        ] {
            assert!(parse_state(&state_fixture(fixture)).is_err());
        }
    }

    #[test]
    fn missing_corrupt_and_future_state_fall_back_without_touching_config() {
        let root = TempDir::new().expect("tempdir");
        let store = state_store(&root, BuildEnvironment::Development);
        assert_eq!(store.load_or_default().status, StateLoadStatus::Missing);

        fs::create_dir_all(&store.layout.root).expect("state root");
        fs::write(&store.layout.config, b"config-sentinel").expect("config sentinel");
        fs::write(&store.layout.state, b"not-json").expect("corrupt state");
        assert_eq!(
            store.load_or_default().status,
            StateLoadStatus::IgnoredInvalid
        );
        assert_eq!(
            fs::read(&store.layout.config).expect("config preserved"),
            b"config-sentinel"
        );

        fs::write(
            &store.layout.state,
            br#"{"schema_version":2,"settings_window":null}"#,
        )
        .expect("future state");
        assert_eq!(
            store.load_or_default().status,
            StateLoadStatus::IgnoredUnsupportedSchema(2)
        );
        assert!(matches!(
            store.commit(&ApplicationState::default()),
            Err(StateError::UnsupportedSchema(2))
        ));

        let oversized_future = br#"{"schema_version":4294967296,"settings_window":null}"#;
        fs::write(&store.layout.state, oversized_future).expect("oversized future state");
        assert_eq!(
            store.load_or_default().status,
            StateLoadStatus::IgnoredUnsupportedSchema(4_294_967_296)
        );
        assert!(matches!(
            store.commit(&ApplicationState::default()),
            Err(StateError::UnsupportedSchema(4_294_967_296))
        ));
        assert_eq!(
            fs::read(&store.layout.state).expect("oversized future state preserved"),
            oversized_future
        );
    }

    #[test]
    fn placement_validation_rejects_unbounded_coordinates_and_dimensions() {
        assert!(matches!(
            WindowPlacement::new(MAX_WINDOW_COORDINATE + 1, 0, 800, 600, false),
            Err(StateError::InvalidValue("settings_window.x"))
        ));
        assert!(matches!(
            WindowPlacement::new(0, 0, MIN_WINDOW_WIDTH - 1, 600, false),
            Err(StateError::InvalidValue("settings_window.width"))
        ));
        assert!(matches!(
            WindowPlacement::new(0, 0, 800, MAX_WINDOW_DIMENSION + 1, false),
            Err(StateError::InvalidValue("settings_window.height"))
        ));
    }

    #[test]
    fn commit_is_atomic_verified_and_restart_readable() {
        let root = TempDir::new().expect("tempdir");
        let store = state_store(&root, BuildEnvironment::Development);
        let first = ApplicationState::with_settings_window(Some(placement(-120, 48)));
        store.commit(&first).expect("first commit");
        assert_eq!(store.load_or_default().state, first);

        let original = fs::read(&store.layout.state).expect("original state");
        let mut faulting = state_store(&root, BuildEnvironment::Development);
        faulting.inject_write_failure(InjectedStateWriteFailure::VerificationCorruption);
        let second = ApplicationState::with_settings_window(Some(placement(240, 180)));
        assert!(matches!(
            faulting.commit(&second),
            Err(StateError::VerificationFailed)
        ));
        assert_eq!(
            fs::read(&faulting.layout.state).expect("restored state"),
            original
        );
        assert_eq!(
            state_store(&root, BuildEnvironment::Development)
                .load_or_default()
                .state,
            first
        );
    }

    #[test]
    fn environments_use_independent_state_and_writer_locks() {
        let root = TempDir::new().expect("tempdir");
        let development = state_store(&root, BuildEnvironment::Development);
        let production = state_store(&root, BuildEnvironment::Production);
        let development_state = ApplicationState::with_settings_window(Some(placement(-300, 40)));
        let production_state = ApplicationState::with_settings_window(Some(placement(900, 60)));
        development
            .commit(&development_state)
            .expect("development commit");
        production
            .commit(&production_state)
            .expect("production commit");

        assert_eq!(development.load_or_default().state, development_state);
        assert_eq!(production.load_or_default().state, production_state);
        assert_ne!(development.layout.state, production.layout.state);
        assert_ne!(
            development.layout.locks.join("state.writer.lock"),
            production.layout.locks.join("state.writer.lock")
        );
    }

    #[test]
    fn concurrent_writer_is_rejected_without_changing_state() {
        let root = TempDir::new().expect("tempdir");
        let store = state_store(&root, BuildEnvironment::Development);
        let original = ApplicationState::with_settings_window(Some(placement(10, 20)));
        store.commit(&original).expect("initial commit");

        let lock_path = store.layout.locks.join("state.writer.lock");
        let lock = File::options()
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("state lock");
        lock.lock().expect("hold state lock");
        assert!(matches!(
            store.commit(&ApplicationState::with_settings_window(Some(placement(
                30, 40
            )))),
            Err(StateError::LockUnavailable)
        ));
        assert_eq!(store.load_or_default().state, original);
    }

    #[cfg(unix)]
    #[test]
    fn state_storage_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().expect("tempdir");
        let store = state_store(&root, BuildEnvironment::Development);
        store
            .commit(&ApplicationState::with_settings_window(Some(placement(
                10, 20,
            ))))
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
            store.layout.state.clone(),
            store.layout.locks.join("state.writer.lock"),
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
