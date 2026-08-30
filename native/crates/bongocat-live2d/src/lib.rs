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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(usize)]
pub enum ProductParameter {
    AngleX,
    AngleY,
    AngleZ,
    EyeBallX,
    EyeBallY,
    LeftHandDown,
    RightHandDown,
    MouseX,
    MouseY,
    MouseLeftDown,
    MouseRightDown,
    StickLeftDown,
    StickRightDown,
    StickShowLeftHand,
    StickShowRightHand,
    StickLeftX,
    StickLeftY,
    StickRightX,
    StickRightY,
}

impl ProductParameter {
    pub const ALL: [Self; Self::COUNT] = [
        Self::AngleX,
        Self::AngleY,
        Self::AngleZ,
        Self::EyeBallX,
        Self::EyeBallY,
        Self::LeftHandDown,
        Self::RightHandDown,
        Self::MouseX,
        Self::MouseY,
        Self::MouseLeftDown,
        Self::MouseRightDown,
        Self::StickLeftDown,
        Self::StickRightDown,
        Self::StickShowLeftHand,
        Self::StickShowRightHand,
        Self::StickLeftX,
        Self::StickLeftY,
        Self::StickRightX,
        Self::StickRightY,
    ];
    pub(crate) const COUNT: usize = 19;

    pub const fn id(self) -> &'static str {
        match self {
            Self::AngleX => "ParamAngleX",
            Self::AngleY => "ParamAngleY",
            Self::AngleZ => "ParamAngleZ",
            Self::EyeBallX => "ParamEyeBallX",
            Self::EyeBallY => "ParamEyeBallY",
            Self::LeftHandDown => "CatParamLeftHandDown",
            Self::RightHandDown => "CatParamRightHandDown",
            Self::MouseX => "ParamMouseX",
            Self::MouseY => "ParamMouseY",
            Self::MouseLeftDown => "ParamMouseLeftDown",
            Self::MouseRightDown => "ParamMouseRightDown",
            Self::StickLeftDown => "CatParamStickLeftDown",
            Self::StickRightDown => "CatParamStickRightDown",
            Self::StickShowLeftHand => "CatParamStickShowLeftHand",
            Self::StickShowRightHand => "CatParamStickShowRightHand",
            Self::StickLeftX => "CatParamStickLX",
            Self::StickLeftY => "CatParamStickLY",
            Self::StickRightX => "CatParamStickRX",
            Self::StickRightY => "CatParamStickRY",
        }
    }

    pub(crate) const fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterRange {
    pub minimum: f32,
    pub maximum: f32,
    pub default: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParameterUpdate {
    Unsupported,
    Applied { value: f32, clamped: bool },
}

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
    ParameterValueInvalid,
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

    pub fn parameter_range(&self, parameter: ProductParameter) -> Option<ParameterRange> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.parameter_range(parameter)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = parameter;
            None
        }
    }

    pub fn parameter_value(&self, parameter: ProductParameter) -> Result<Option<f32>, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.parameter_value(parameter)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = parameter;
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    pub fn set_parameter(
        &mut self,
        parameter: ProductParameter,
        value: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.core.set_parameter(parameter, value)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (parameter, value);
            Err(Live2dError::new(
                Live2dErrorCode::PlatformUnsupported,
                "Cubism Core is available only on the Windows and macOS product targets",
            ))
        }
    }

    pub fn set_normalized_parameter(
        &mut self,
        parameter: ProductParameter,
        value: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{} received a non-finite normalized value", parameter.id()),
            ));
        }
        let Some(range) = self.parameter_range(parameter) else {
            return Ok(ParameterUpdate::Unsupported);
        };
        let normalized = value.clamp(-1.0, 1.0);
        let mapped = if normalized >= 0.0 {
            range.default + (range.maximum - range.default) * normalized
        } else {
            range.default + (range.default - range.minimum) * normalized
        };
        self.set_parameter(parameter, mapped)
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

    #[test]
    fn product_parameter_ids_are_stable_and_unique() {
        let mut ids = ProductParameter::ALL
            .iter()
            .map(|parameter| parameter.id())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ProductParameter::ALL.len());
        assert_eq!(ProductParameter::LeftHandDown.id(), "CatParamLeftHandDown");
        assert_eq!(ProductParameter::StickRightY.id(), "CatParamStickRY");
    }
}
