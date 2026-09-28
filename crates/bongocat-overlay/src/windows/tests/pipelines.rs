//! The encoded-space colour contract and Cubism's winding order.

use super::*;

#[test]
fn cull_states_use_cubism_counter_clockwise_front_faces() {
    for cull_mode in [D3D11_CULL_NONE, D3D11_CULL_BACK, D3D11_CULL_FRONT] {
        let descriptor = rasterizer_descriptor(cull_mode);
        assert_eq!(descriptor.CullMode, cull_mode);
        assert!(descriptor.FrontCounterClockwise.as_bool());
    }
}

#[test]
fn color_formats_match_the_encoded_space_contract() {
    assert_eq!(MODEL_TEXTURE_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM);
    assert_eq!(COMPOSITION_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert_eq!(COMPOSITION_RENDER_TARGET_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert_eq!(MASK_TEXTURE_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM);
}
