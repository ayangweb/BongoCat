//! The Cubism Core binding.
//!
//! This is the only module in the crate that holds a Cubism pointer, and the only
//! one that calls into the native library. Everything above it reads a snapshot
//! and writes parameters; nothing above it needs to know a moc from a model.

use crate::{
    CUBISM_CORE_VERSION, CUBISM_LATEST_MOC_VERSION, Live2dError, Live2dErrorCode, ParameterRange,
    ParameterUpdate, ProductParameter, sys,
};
use bongocat_render::{
    BlendMode, CanvasInfo, DrawableDynamicFlags, DrawableId, DrawableSnapshot, ModelBounds,
    RenderSnapshot, TextureId, Vertex,
};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    collections::{BTreeMap, BTreeSet},
    ffi::CStr,
    fs,
    mem::ManuallyDrop,
    path::Path,
    ptr::NonNull,
};

mod canvas;
mod count;
mod drawable;
mod length;
mod load;
mod memory;
mod parameter;
mod resolved;
mod snapshot;
#[cfg(test)]
mod tests;

pub(crate) use canvas::*;
pub(crate) use drawable::*;
pub(crate) use length::*;
pub(crate) use memory::*;
pub(crate) use resolved::*;

// The public surface. A `pub(crate)` glob narrows everything it carries, so
// the items the adapter names are listed here rather than left to a glob.

pub(crate) struct CoreModel {
    pub(crate) model: NonNull<sys::csmModel>,
    pub(crate) parameters: [Option<ResolvedParameter>; ProductParameter::COUNT],
    pub(crate) parameters_by_id: BTreeMap<String, ResolvedParameter>,
    pub(crate) parts_by_id: BTreeMap<String, usize>,
    pub(crate) part_opacity_defaults: Vec<f32>,
    pub(crate) model_memory: ManuallyDrop<AlignedMemory>,
    pub(crate) moc_memory: ManuallyDrop<AlignedMemory>,
}

impl Drop for CoreModel {
    fn drop(&mut self) {
        // Model contains pointers into Moc-owned tables, so its allocation is
        // always released first. Neither pointer is observable after this.
        unsafe {
            ManuallyDrop::drop(&mut self.model_memory);
            ManuallyDrop::drop(&mut self.moc_memory);
        }
    }
}
