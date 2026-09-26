//! Importing a model: the request, the progress, and the cancellation.
//!
//! An import is a long operation with its own identity, so it gets an id and a
//! typed progress record rather than a stream of unrelated events. Cancelling is
//! a command on the operation, not a flag on the import, so a cancelled import
//! still reports what it managed to do.

use super::*;

/// The archive extension a suggested title should not repeat.
///
/// Nothing in the product reads a model archive today — the picker only returns
/// folders and the store only ingests them (ADR-0036 已撤回) — so this rule only
/// reaches a path that is *not* a directory: a folder may legitimately be called
/// `something.zip` and keeps its name, while anything else drops the suffix so
/// that `名字.zip` and the folder it was made from suggest the same title. It
/// stays because that naming rule is shared with the settings service's fallback,
/// and dropping it would leave the two entrances disagreeing once archive import
/// is built again.
pub(crate) const ARCHIVE_EXTENSION: &str = ".zip";

/// The display name a chosen model source suggests.
///
/// The page pre-fills this name and the settings service falls back to it, so one
/// rule answers both entrances. The source is the folder a user picked, and a
/// folder keeps its full name — because a folder may legitimately be called
/// `something.zip`. A path that is *not* a directory drops the archive extension
/// instead, so the folder a user exports and the `名字.zip` made from it suggest
/// the same title; that branch has no caller in the product today (ADR-0036
/// 已撤回) and is kept so the rule is not lost with the feature.
///
/// The name is display-only: the portable store key stays a service-generated
/// UUID and is never derived from it.
///
/// `is_directory` is supplied by the adapter that already owns the source
/// selection. Keeping filesystem inspection out of this contract prevents a
/// GPUI executor from accidentally blocking on a model-picker path.
pub fn model_source_display_name(source_root: &Path, is_directory: bool) -> Option<String> {
    let name = source_root.file_name()?.to_str()?;
    let name = if is_directory {
        name
    } else {
        strip_archive_extension(name)
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Drop a trailing `.zip`, whatever its case.
///
/// The suffix is compared and removed through the bounds-checked `str` accessors
/// rather than by slicing at a byte offset, because a name whose last four bytes
/// are only *part* of a multi-byte character must leave the name untouched
/// instead of panicking.
pub(crate) fn strip_archive_extension(name: &str) -> &str {
    let Some(offset) = name.len().checked_sub(ARCHIVE_EXTENSION.len()) else {
        return name;
    };
    match name.get(offset..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(ARCHIVE_EXTENSION) => {
            name.get(..offset).unwrap_or(name)
        }
        _ => name,
    }
}

/// One selectable conversion mode of a BongoCat Mver source.
///
/// Mirrors the model crate's mode set at the UI boundary so the settings page
/// can offer choices without depending on the model crate. A source that
/// supports conversion in any of these carries a matching converted model; the
/// order here is the order a request reports modes in.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SettingsMverMode {
    Standard,
    Keyboard,
    Gamepad,
}

/// The input family projected onto one model card.
///
/// This is separate from [`SettingsMverMode`]: the latter is a conversion choice
/// for an inspected Mver source, while this value is the mode already resolved
/// and stored for an individual model.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SettingsModelMode {
    Standard,
    Keyboard,
    Gamepad,
}

/// What inspecting a user-picked source turned out to be.
///
/// `Package` is a single BongoCat model package: nothing converts and any
/// selection is ignored. `Mver` is a BongoCat Mver source, and `modes` is
/// exactly the conversions it actually carries, in report order — it is the
/// set the UI shows checkboxes for and the set a request can ask for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsModelSourceContent {
    Package,
    Mver { modes: Vec<SettingsMverMode> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsModelImportRequest {
    /// User-chosen display name for the imported model; the store key is a
    /// service-generated UUID and never derived from this value.
    pub title: String,
    pub source_root: PathBuf,
    /// The legacy modes to convert when the source turns out to be a BongoCat
    /// Mver source, in the caller's own selection order.
    ///
    /// Ignored for package sources. The request only ever names modes the
    /// inspection reported; the store still refuses a mode the source does not
    /// actually carry.
    pub selected_mver_modes: Vec<SettingsMverMode>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SettingsOperationId(pub(crate) u64);

impl SettingsOperationId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SettingsModelImportStage {
    Preparing,
    Copying,
    Validating,
    Committing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsModelImportProgress {
    pub stage: SettingsModelImportStage,
    pub files_copied: u64,
    pub bytes_copied: u64,
}

pub struct SettingsModelImportFinalResult {
    pub operation_id: SettingsOperationId,
    pub result: Result<SettingsSnapshot, SettingsError>,
}

#[derive(Clone)]
pub struct SettingsModelImportControl {
    pub(crate) operation_id: SettingsOperationId,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) progress: Arc<Mutex<SettingsModelImportProgress>>,
}

impl SettingsModelImportControl {
    pub const fn operation_id(&self) -> SettingsOperationId {
        self.operation_id
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn report_progress(&self, progress: SettingsModelImportProgress) -> bool {
        let mut current = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.stage < current.stage
            || progress.files_copied < current.files_copied
            || progress.bytes_copied < current.bytes_copied
        {
            return false;
        }
        *current = progress;
        true
    }
}

pub struct SettingsModelImportOperation {
    pub(crate) control: SettingsModelImportControl,
    pub(crate) result: Receiver<Result<SettingsSnapshot, SettingsError>>,
}

#[derive(Clone)]
pub struct SettingsModelImportMonitor {
    pub(crate) control: SettingsModelImportControl,
}

impl SettingsModelImportMonitor {
    pub const fn operation_id(&self) -> SettingsOperationId {
        self.control.operation_id()
    }

    pub fn progress(&self) -> SettingsModelImportProgress {
        *self
            .control
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn cancel(&self) -> bool {
        !self.control.cancelled.swap(true, Ordering::AcqRel)
    }

    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }
}

impl SettingsModelImportOperation {
    pub const fn operation_id(&self) -> SettingsOperationId {
        self.control.operation_id()
    }

    pub fn progress(&self) -> SettingsModelImportProgress {
        self.monitor().progress()
    }

    pub fn cancel(&self) -> bool {
        self.monitor().cancel()
    }

    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }

    pub fn monitor(&self) -> SettingsModelImportMonitor {
        SettingsModelImportMonitor {
            control: self.control.clone(),
        }
    }

    pub async fn final_result(self) -> SettingsModelImportFinalResult {
        let operation_id = self.operation_id();
        let result = self
            .result
            .recv()
            .await
            .unwrap_or_else(|_| Err(SettingsError::new(SettingsErrorCode::ServiceUnavailable)));
        SettingsModelImportFinalResult {
            operation_id,
            result,
        }
    }

    pub fn final_result_blocking(self) -> SettingsModelImportFinalResult {
        let operation_id = self.operation_id();
        let result = self
            .result
            .recv_blocking()
            .unwrap_or_else(|_| Err(SettingsError::new(SettingsErrorCode::ServiceUnavailable)));
        SettingsModelImportFinalResult {
            operation_id,
            result,
        }
    }
}
