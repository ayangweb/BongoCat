//! Cubism's flags and blend modes decode to what they mean.

use super::*;

#[test]
fn dynamic_flags_preserve_each_cubism_change_bit() {
    assert_eq!(
        decode_dynamic_flags(
            sys::csmVisibilityDidChange as u8
                | sys::csmOpacityDidChange as u8
                | sys::csmDrawOrderDidChange as u8
                | sys::csmRenderOrderDidChange as u8
                | sys::csmVertexPositionsDidChange as u8
                | sys::csmBlendColorDidChange as u8,
        ),
        DrawableDynamicFlags {
            visibility_changed: true,
            opacity_changed: true,
            draw_order_changed: true,
            render_order_changed: true,
            vertex_positions_changed: true,
            blend_color_changed: true,
        }
    );
}

#[test]
fn drawable_blend_flags_accept_only_documented_core_modes() {
    assert_eq!(decode_blend_mode(0).expect("normal"), BlendMode::Normal);
    assert_eq!(decode_blend_mode(1).expect("additive"), BlendMode::Additive);
    assert_eq!(
        decode_blend_mode(2).expect("multiplicative"),
        BlendMode::Multiplicative
    );
    assert_eq!(
        decode_blend_mode(3)
            .expect_err("combined flags are unsupported")
            .code,
        Live2dErrorCode::UnsupportedBlendMode
    );
    assert_eq!(
        decode_blend_mode(0x100)
            .expect_err("unknown high bits are unsupported")
            .code,
        Live2dErrorCode::UnsupportedBlendMode
    );
}
