//! The GPU structs matching the layout D3D11 expects.

use super::*;

#[test]
fn gpu_structs_match_d3d11_layout() {
    assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
    assert_eq!(size_of::<Uniforms>(), 96);
    assert_eq!(size_of::<Uniforms>() % 16, 0);
}

#[test]
fn texture_upload_premultiplies_rgb_before_filtering() {
    let mut image = image::RgbaImage::from_raw(2, 1, vec![200, 100, 50, 128, 20, 40, 60, 0])
        .expect("test image dimensions");

    premultiply_alpha(&mut image);

    assert_eq!(image.as_raw(), &[100, 50, 25, 128, 0, 0, 0, 0]);
}
