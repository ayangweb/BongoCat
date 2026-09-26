//! The blend contract the renderer is held to.
//!
//! Cubism names its blend modes; the contract here says what each one means in
//! terms of source and destination factors, and which drawables have their
//! culling reversed because the model is mirrored. A mode the table does not
//! cover is refused rather than defaulted, because a wrong blend factor produces
//! a picture that looks almost right.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlendFactor {
    Zero,
    One,
    OneMinusSourceAlpha,
    DestinationColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlendFactors {
    pub source_rgb: BlendFactor,
    pub destination_rgb: BlendFactor,
    pub source_alpha: BlendFactor,
    pub destination_alpha: BlendFactor,
}

/// The platform-independent culling decision for one model drawable.
///
/// Core's `double_sided` flag disables culling. A horizontal mirror negates the
/// model X scale and therefore reverses triangle winding, so the mirrored
/// backends must cull the opposite face to keep the original front surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DrawableCullMode {
    None,
    Front,
    Back,
}

pub(crate) const fn drawable_cull_mode(
    double_sided: bool,
    mirror_horizontal: bool,
) -> DrawableCullMode {
    match (double_sided, mirror_horizontal) {
        (true, _) => DrawableCullMode::None,
        (false, false) => DrawableCullMode::Back,
        (false, true) => DrawableCullMode::Front,
    }
}

pub(crate) const fn blend_factors(mode: BlendMode) -> BlendFactors {
    match mode {
        BlendMode::Normal => BlendFactors {
            source_rgb: BlendFactor::One,
            destination_rgb: BlendFactor::OneMinusSourceAlpha,
            source_alpha: BlendFactor::One,
            destination_alpha: BlendFactor::OneMinusSourceAlpha,
        },
        BlendMode::Additive => BlendFactors {
            source_rgb: BlendFactor::One,
            destination_rgb: BlendFactor::One,
            source_alpha: BlendFactor::Zero,
            destination_alpha: BlendFactor::One,
        },
        BlendMode::Multiplicative => BlendFactors {
            source_rgb: BlendFactor::DestinationColor,
            destination_rgb: BlendFactor::OneMinusSourceAlpha,
            source_alpha: BlendFactor::Zero,
            destination_alpha: BlendFactor::One,
        },
    }
}
