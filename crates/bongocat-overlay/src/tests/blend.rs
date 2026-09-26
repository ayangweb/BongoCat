//! Every Cubism blend mode is covered, and a mirrored drawable culls the other way.

use super::*;

#[test]
fn premultiplied_blend_contract_covers_all_cubism_modes() {
    assert_eq!(
        blend_factors(BlendMode::Normal),
        BlendFactors {
            source_rgb: BlendFactor::One,
            destination_rgb: BlendFactor::OneMinusSourceAlpha,
            source_alpha: BlendFactor::One,
            destination_alpha: BlendFactor::OneMinusSourceAlpha,
        }
    );
    assert_eq!(
        blend_factors(BlendMode::Additive),
        BlendFactors {
            source_rgb: BlendFactor::One,
            destination_rgb: BlendFactor::One,
            source_alpha: BlendFactor::Zero,
            destination_alpha: BlendFactor::One,
        }
    );
    assert_eq!(
        blend_factors(BlendMode::Multiplicative),
        BlendFactors {
            source_rgb: BlendFactor::DestinationColor,
            destination_rgb: BlendFactor::OneMinusSourceAlpha,
            source_alpha: BlendFactor::Zero,
            destination_alpha: BlendFactor::One,
        }
    );
}

#[test]
fn mirrored_single_sided_drawables_reverse_culling() {
    assert_eq!(drawable_cull_mode(false, false), DrawableCullMode::Back);
    assert_eq!(drawable_cull_mode(false, true), DrawableCullMode::Front);
    assert_eq!(drawable_cull_mode(true, false), DrawableCullMode::None);
    assert_eq!(drawable_cull_mode(true, true), DrawableCullMode::None);
}
