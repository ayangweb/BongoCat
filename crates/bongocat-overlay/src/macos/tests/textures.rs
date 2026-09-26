//! The GPU structs, the colour contract, and an empty drawable.

use super::*;

#[test]
fn gpu_structs_match_metal_layout() {
    assert_eq!(size_of::<bongocat_render::Vertex>(), 16);
    assert_eq!(size_of::<Uniforms>(), 96);
}

#[test]
fn color_formats_match_the_encoded_space_contract() {
    assert_eq!(MODEL_TEXTURE_FORMAT, MTLPixelFormat::RGBA8Unorm);
    assert_eq!(COLOR_ATTACHMENT_FORMAT, MTLPixelFormat::BGRA8Unorm);
    assert_eq!(MASK_TEXTURE_FORMAT, MTLPixelFormat::BGRA8Unorm);
}

/// A drawable the Core reports with vertices but no triangles must not stop
/// the GPU model from being prepared.
///
/// Cubism allows such a drawable — an authoring tool that deletes every
/// triangle of a part without deleting the part leaves one behind — and a
/// real community model ships one, so treating it as a fatal error made an
/// otherwise drawable model fail both its cover capture and its activation.
///
/// The mesh is still built, because another drawable may name it as a mask
/// source and the mask buffer it fills has to keep the value it was cleared
/// to; it simply draws zero triangles. Metal rejects a zero-length buffer,
/// so both arrays are carried by a single placeholder element while the mesh
/// keeps the snapshot's own vertex count for the cross-frame check.
#[test]
fn a_drawable_without_triangles_still_prepares_the_gpu_model() {
    use bongocat_render::{DrawableDynamicFlags, DrawableSnapshot};

    let Some(device) = Device::system_default() else {
        // No Metal device on this machine; the contract under test is a GPU
        // one, and the platform-neutral half is covered in `bongocat-render`.
        return;
    };
    let directory =
        std::env::temp_dir().join(format!("bongocat-empty-drawable-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temporary directory");
    let texture_path = directory.join("texture.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]))
        .save(&texture_path)
        .expect("write texture");

    let canvas = CanvasInfo {
        width: 2.0,
        height: 2.0,
        origin_x: 1.0,
        origin_y: 1.0,
        pixels_per_unit: 1.0,
    };
    let resources = RenderResources {
        textures: vec![TextureAsset {
            id: TextureId::new(0),
            path: texture_path.clone(),
            width: 2,
            height: 2,
        }],
        key_assets: Vec::new(),
        background: None,
    };
    let drawable = |id: usize, indices: Vec<u16>| DrawableSnapshot {
        id: DrawableId::new(id),
        dynamic_flags: DrawableDynamicFlags::default(),
        render_order: id as i32,
        visible: true,
        texture_id: TextureId::new(0),
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        double_sided: false,
        inverted_mask: false,
        multiply_color: [1.0; 4],
        screen_color: [0.0; 4],
        masks: Vec::new(),
        vertices: vec![
            bongocat_render::Vertex {
                position: [-0.5, -0.5],
                uv: [0.0, 0.0],
            },
            bongocat_render::Vertex {
                position: [0.5, -0.5],
                uv: [1.0, 0.0],
            },
            bongocat_render::Vertex {
                position: [0.0, 0.5],
                uv: [0.5, 1.0],
            },
        ],
        indices,
    };
    let snapshot = RenderSnapshot {
        canvas,
        bounds: ModelBounds::from_canvas(canvas),
        active_keys: Vec::new(),
        model_opacity: 1.0,
        mirror_horizontal: false,
        drawables: vec![drawable(0, vec![0, 1, 2]), drawable(1, Vec::new())],
    };

    let model = GpuModel::prepare(&device, &resources, &snapshot, 2, 2)
        .expect("a model with an empty drawable must still prepare");
    let empty = model
        .meshes
        .iter()
        .find(|mesh| mesh.id == DrawableId::new(1))
        .expect("the empty drawable keeps its mesh so a mask can still name it");
    assert_eq!(
        empty.index_count, 0,
        "an empty drawable must draw no triangles, which is what leaves the mask buffer it \
         fills at its cleared value"
    );
    assert_eq!(
        empty.vertex_bytes,
        std::mem::size_of::<bongocat_render::Vertex>() * 3,
        "the mesh must keep the snapshot's own vertex count for the cross-frame check"
    );
    assert!(
        empty.vertex_buffer.length() > 0 && empty.index_buffer.length() > 0,
        "Metal cannot allocate a zero-length buffer, so both arrays need a placeholder"
    );

    let _ = std::fs::remove_file(&texture_path);
}
