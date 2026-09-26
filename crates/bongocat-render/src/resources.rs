//! What a frame draws from.
//!
//! The resources are named by path and validated once, at load. A snapshot that
//! names a texture the resources do not have is refused at the validation step
//! rather than at the draw call, where the failure would be a read past the end of
//! a texture array on a device that happened to have spare memory.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureAsset {
    pub id: TextureId,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyAsset {
    pub id: KeyAssetId,
    pub side: KeySide,
    pub name: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundAsset {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderResources {
    pub textures: Vec<TextureAsset>,
    pub key_assets: Vec<KeyAsset>,
    pub background: Option<BackgroundAsset>,
}
