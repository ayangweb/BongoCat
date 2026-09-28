#![forbid(unsafe_code)]

//! Reading, validating and describing Live2D model packages.
//!
//! A model package is a directory tree the product did not create, so this
//! crate is mostly about refusing the ones it cannot show safely and about
//! naming the reasons in a form the settings window can act on. `schema` is what
//! a package declares, `catalog` is what the product ended up holding, and
//! `package`, `validate` and `formats` do the reading and the refusing.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

mod catalog;
mod error;
mod formats;
mod json;
mod limits;
mod package;
mod raw;
mod reference;
mod schema;
mod snapshot;
#[cfg(test)]
mod tests;
mod validate;

// Every module reaches its neighbours through this one prelude rather than
// naming each of them: the crate's items are one vocabulary, and a list per
// module would be the same list ten times. The four modules left out are the
// ones nothing reaches across a boundary for — their items are named through
// the public re-exports below instead.
pub(crate) use formats::*;
pub(crate) use json::*;
pub(crate) use limits::*;
pub(crate) use package::*;
pub(crate) use raw::*;
pub(crate) use schema::*;
pub(crate) use validate::*;

pub use catalog::{
    CommittedModel, InstalledModel, ModelCatalogEntry, ModelOrigin, PreparedModel,
    PresetModelCatalog,
};
pub use error::{ModelDiagnostic, ModelError};
pub use limits::{
    INDEX_SCHEMA_VERSION, MODEL_ID_MAXIMUM_LENGTH, ModelId, ModelPackageLimits, PACKAGE_COVER_FILE,
    PACKAGE_RESOURCES_DIRECTORY, package_cover_path,
};
pub use reference::{normalize_reference, path_from_reference};
pub use schema::{
    ImageResource, ModelGroup, ModelPackageIndex, MotionGroup, MotionResource, PhysicsChannel,
    PhysicsDefinition, PhysicsInput, PhysicsOutput, PhysicsRange, PhysicsSetting, PhysicsVector,
    PhysicsVertex,
};
pub use snapshot::{ModelBehaviorSnapshot, ModelSnapshot};
