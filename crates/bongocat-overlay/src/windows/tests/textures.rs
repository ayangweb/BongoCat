//! The GPU structs matching the layout D3D11 expects.

use super::*;

#[test]
fn gpu_structs_match_d3d11_layout() {
    assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
    assert_eq!(size_of::<Uniforms>(), 96);
    assert_eq!(size_of::<Uniforms>() % 16, 0);
}
