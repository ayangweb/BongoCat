//! The render vocabulary's tests, split by the module they cover.

use super::*;

fn frame(number: u64) -> RenderFrame {
    RenderFrame {
        transport_sequence: number,
        model_generation: 3,
        frame_number: number,
        model_commit: None,
        resources: Arc::new(RenderResources {
            textures: vec![],
            key_assets: vec![],
            background: None,
        }),
        snapshot: Arc::new(RenderSnapshot {
            canvas: CanvasInfo {
                width: 1.0,
                height: 1.0,
                origin_x: 0.0,
                origin_y: 0.0,
                pixels_per_unit: 1.0,
            },
            bounds: ModelBounds {
                min_x: -0.5,
                max_x: 0.5,
                min_y: -0.5,
                max_y: 0.5,
            },
            active_keys: Vec::new(),
            model_opacity: 1.0,
            mirror_horizontal: false,
            drawables: vec![],
        }),
    }
}

fn validated_resources() -> RenderResources {
    RenderResources {
        textures: vec![TextureAsset {
            id: TextureId::new(0),
            path: PathBuf::from("texture.png"),
            width: 1,
            height: 1,
        }],
        key_assets: Vec::new(),
        background: None,
    }
}

fn validated_snapshot() -> RenderSnapshot {
    RenderSnapshot {
        canvas: CanvasInfo {
            width: 1.0,
            height: 1.0,
            origin_x: 0.0,
            origin_y: 0.0,
            pixels_per_unit: 1.0,
        },
        bounds: ModelBounds {
            min_x: -0.5,
            max_x: 0.5,
            min_y: -0.5,
            max_y: 0.5,
        },
        active_keys: Vec::new(),
        model_opacity: 1.0,
        mirror_horizontal: false,
        drawables: vec![DrawableSnapshot {
            id: DrawableId::new(0),
            dynamic_flags: DrawableDynamicFlags::default(),
            render_order: 0,
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
                Vertex {
                    position: [-0.5, -0.5],
                    uv: [0.0, 0.0],
                },
                Vertex {
                    position: [0.5, -0.5],
                    uv: [1.0, 0.0],
                },
                Vertex {
                    position: [0.0, 0.5],
                    uv: [0.5, 1.0],
                },
            ],
            indices: vec![0, 1, 2],
        }],
    }
}

mod channel;
mod commit;
mod geometry;
mod identity;
mod key;
mod validate;
