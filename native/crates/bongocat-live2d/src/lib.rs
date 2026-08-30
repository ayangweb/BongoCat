#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    forbid(unsafe_code)
)]

use bongocat_model::PreparedModel;
use std::{fmt, path::PathBuf};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod core;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod sys;

pub const CUBISM_SDK_RELEASE: &str = "5-r.5";
pub const CUBISM_CORE_VERSION: u32 = 0x0600_0001;
pub const CUBISM_LATEST_MOC_VERSION: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasInfo {
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub pixels_per_unit: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlendMode {
    Normal,
    Additive,
    Multiplicative,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawableSnapshot {
    pub source_index: usize,
    pub render_order: i32,
    pub visible: bool,
    pub texture_index: usize,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub double_sided: bool,
    pub inverted_mask: bool,
    pub multiply_color: [f32; 4],
    pub screen_color: [f32; 4],
    pub masks: Vec<usize>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSnapshot {
    pub canvas: CanvasInfo,
    pub drawables: Vec<DrawableSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureAsset {
    pub index: usize,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Live2dErrorCode {
    CoreVersionMismatch,
    EmptyMoc,
    InvalidCoreArray,
    InvalidCoreValue,
    MocConsistencyFailed,
    MocReviveFailed,
    ModelInitializeFailed,
    ModelMemoryInvalid,
    PlatformUnsupported,
    ResourceIo,
    TextureIndexInvalid,
    UnsupportedBlendMode,
}

#[derive(Debug)]
pub struct Live2dError {
    pub code: Live2dErrorCode,
    pub detail: String,
}

impl Live2dError {
    pub(crate) fn new(code: Live2dErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Live2dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Live2dError {}

pub struct Live2dModel {
    textures: Vec<TextureAsset>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    core: core::CoreModel,
}

impl Live2dModel {
    pub fn load(model: &PreparedModel) -> Result<Self, Live2dError> {
        let textures = model
            .index()
            .textures
            .iter()
            .enumerate()
            .map(|(index, texture)| TextureAsset {
                index,
                path: model.root().join(&texture.file),
                width: texture.width,
                height: texture.height,
            })
            .collect::<Vec<_>>();

        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let moc_path = model.root().join(&model.index().moc);
            let core = core::CoreModel::load(&moc_path)?;
            core.validate_texture_indices(textures.len())?;
            Ok(Self { textures, core })
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = textures;
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    pub fn texture_assets(&self) -> &[TextureAsset] {
        &self.textures
    }

    pub fn update_and_snapshot(&mut self) -> Result<RenderSnapshot, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.update_and_snapshot()
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_layout_is_tightly_packed_for_gpu_upload() {
        assert_eq!(size_of::<Vertex>(), 16);
        assert_eq!(align_of::<Vertex>(), 4);
    }

    #[test]
    fn supported_blend_modes_are_explicit() {
        assert_ne!(BlendMode::Normal, BlendMode::Additive);
        assert_ne!(BlendMode::Additive, BlendMode::Multiplicative);
    }
}
