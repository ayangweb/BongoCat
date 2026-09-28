//! The models the product holds.
//!
//! A model is `Prepared` before it is `Installed`, and `Installed` becomes
//! `Committed` only when the runtime has accepted it for a window. Keeping the
//! three apart is what makes a failed switch leave the previous model on
//! screen: a commit that is refused is never a commit at all.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedModel {
    pub(crate) id: ModelId,
    pub(crate) canonical_root: PathBuf,
    pub(crate) index: ModelPackageIndex,
    pub(crate) limits: ModelPackageLimits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledModel {
    pub(crate) prepared: PreparedModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    Preset,
    Installed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedModel {
    pub(crate) prepared: PreparedModel,
    pub(crate) origin: ModelOrigin,
}

/// One entry in the merged model catalog exposed by the model layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelCatalogEntry {
    Ready {
        origin: ModelOrigin,
        snapshot: ModelSnapshot,
    },
    Invalid {
        origin: ModelOrigin,
        id: ModelId,
        code: ModelDiagnostic,
        resource: Option<String>,
        detail: String,
    },
}

impl ModelCatalogEntry {
    pub const fn origin(&self) -> ModelOrigin {
        match self {
            Self::Ready { origin, .. } | Self::Invalid { origin, .. } => *origin,
        }
    }

    pub fn id(&self) -> &ModelId {
        match self {
            Self::Ready { snapshot, .. } => &snapshot.id,
            Self::Invalid { id, .. } => id,
        }
    }

    pub fn snapshot(&self) -> Option<&ModelSnapshot> {
        match self {
            Self::Ready { snapshot, .. } => Some(snapshot),
            Self::Invalid { .. } => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PresetModelCatalog {
    pub(crate) root: PathBuf,
    pub(crate) limits: ModelPackageLimits,
}

impl InstalledModel {
    /// Wrap a package that has already passed `PreparedModel::prepare`.
    ///
    /// The product wiring calls this only from the model-store transaction
    /// after staging validation and commit; callers must not use it to bypass
    /// store ownership.
    pub fn from_prepared(prepared: PreparedModel) -> Self {
        Self { prepared }
    }

    pub fn id(&self) -> &ModelId {
        self.prepared.id()
    }

    pub fn root(&self) -> &Path {
        self.prepared.root()
    }

    pub fn index(&self) -> &ModelPackageIndex {
        self.prepared.index()
    }

    pub fn physics_definition(&self) -> Result<Option<PhysicsDefinition>, ModelError> {
        self.prepared.physics_definition()
    }

    pub fn snapshot(&self) -> ModelSnapshot {
        self.prepared.snapshot()
    }
}

impl From<InstalledModel> for CommittedModel {
    fn from(installed: InstalledModel) -> Self {
        Self {
            prepared: installed.prepared,
            origin: ModelOrigin::Installed,
        }
    }
}

impl CommittedModel {
    pub fn origin(&self) -> ModelOrigin {
        self.origin
    }

    pub fn id(&self) -> &ModelId {
        self.prepared.id()
    }

    pub fn root(&self) -> &Path {
        self.prepared.root()
    }

    pub fn index(&self) -> &ModelPackageIndex {
        self.prepared.index()
    }

    pub fn physics_definition(&self) -> Result<Option<PhysicsDefinition>, ModelError> {
        self.prepared.physics_definition()
    }

    pub fn snapshot(&self) -> ModelSnapshot {
        self.prepared.snapshot()
    }
}

impl PresetModelCatalog {
    pub fn open(root: impl AsRef<Path>, limits: ModelPackageLimits) -> Result<Self, ModelError> {
        let root = root.as_ref();
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("preset catalog root cannot be opened: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ModelError::new(
                ModelDiagnostic::ModelSymlinkDirectoryUnsupported,
                None,
                "preset catalog root must be a real directory",
            ));
        }
        let root = root.canonicalize().map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("preset catalog root cannot be resolved: {error}"),
            )
        })?;
        Ok(Self { root, limits })
    }

    pub fn load(&self, id: &ModelId) -> Result<CommittedModel, ModelError> {
        let candidate = self.root.join(id.as_str());
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                Some(id.as_str()),
                format!("preset model cannot be opened: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ModelError::new(
                ModelDiagnostic::ModelSymlinkDirectoryUnsupported,
                Some(id.as_str()),
                "preset model must be a real directory",
            ));
        }
        let prepared = PreparedModel::prepare(id.clone(), candidate, self.limits)?;
        if prepared.root().parent() != Some(self.root.as_path()) {
            return Err(ModelError::new(
                ModelDiagnostic::ModelReferenceEscapesRoot,
                Some(id.as_str()),
                "preset model resolves outside the catalog root",
            ));
        }
        Ok(CommittedModel {
            prepared,
            origin: ModelOrigin::Preset,
        })
    }

    pub fn list(&self) -> Result<Vec<ModelCatalogEntry>, ModelError> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                None,
                format!("preset catalog cannot be listed: {error}"),
            )
        })? {
            let entry = entry.map_err(|error| {
                ModelError::new(
                    ModelDiagnostic::ModelIoError,
                    None,
                    format!("preset catalog entry cannot be read: {error}"),
                )
            })?;
            // An entry that cannot even be a model id — a `.DS_Store` a file
            // manager dropped here, for example — is not a broken model, it is
            // not a model at all. Skipping keeps one stray file from taking the
            // whole preset catalog down, matching the store scan's semantics.
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            let Ok(id) = ModelId::parse(name) else {
                continue;
            };
            let catalog_entry = match self.load(&id) {
                Ok(model) => ModelCatalogEntry::Ready {
                    origin: ModelOrigin::Preset,
                    snapshot: model.snapshot(),
                },
                Err(error) => ModelCatalogEntry::Invalid {
                    origin: ModelOrigin::Preset,
                    id,
                    code: error.code,
                    resource: error.resource,
                    detail: error.detail,
                },
            };
            entries.push(catalog_entry);
        }
        entries.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(entries)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl PreparedModel {
    pub fn prepare(
        id: ModelId,
        root: impl AsRef<Path>,
        limits: ModelPackageLimits,
    ) -> Result<Self, ModelError> {
        let (canonical_root, index) = inspect_model_package(root, limits)?;
        Ok(Self {
            id,
            canonical_root,
            index,
            limits,
        })
    }

    /// Rebind a validated package to its committed root after an atomic store
    /// rename. The package index and validation result are unchanged.
    pub fn relocate(self, root: impl Into<PathBuf>) -> Self {
        let mut prepared = self;
        prepared.canonical_root = root.into();
        prepared
    }

    pub fn id(&self) -> &ModelId {
        &self.id
    }

    pub fn root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn index(&self) -> &ModelPackageIndex {
        &self.index
    }

    pub fn physics_definition(&self) -> Result<Option<PhysicsDefinition>, ModelError> {
        let Some(reference) = self.index.physics.as_deref() else {
            return Ok(None);
        };
        let mut reader = PackageReader::new(&self.canonical_root, self.limits)?;
        let normalized = reader.resolve_physics(reference)?;
        let path = self.canonical_root.join(path_from_reference(&normalized));
        load_physics_definition(
            &path,
            &normalized,
            self.limits.maximum_json_bytes,
            self.limits.maximum_json_depth,
        )
        .map(Some)
    }

    pub fn snapshot(&self) -> ModelSnapshot {
        ModelSnapshot {
            id: self.id.clone(),
            entry: self.index.entry.clone(),
            behaviors: self
                .index
                .motion_groups
                .iter()
                .flat_map(|group| {
                    (0..group.motions.len()).map(|index| ModelBehaviorSnapshot::Motion {
                        group: group.name.clone(),
                        index,
                    })
                })
                .chain(self.index.expressions.iter().map(|expression| {
                    ModelBehaviorSnapshot::Expression {
                        name: expression.name.clone(),
                    }
                }))
                .collect(),
        }
    }
}
