//! The topmost layer pass, for Metal.
//!
//! A layer is one textured quad drawn after everything else, positioned in
//! normalized device coordinates so a panel is interface chrome that does not
//! mirror with the model and does not scale with the model's units.
//!
//! The only reason this file exists separately from the model's own drawing is
//! that a layer's texture changes on a different schedule from a model's: a panel
//! is re-rasterized whenever a value it binds to changes, and its texture is
//! re-uploaded only when the resulting pixels differ. The layer cache below is
//! what keeps the difference cheap.

use super::*;
use crate::layers::LayerTextures;
use bongocat_render::PlacedOverlayLayer;

/// One layer's GPU texture, vertex buffer and the six shared indices.
pub(crate) struct LayerTexture {
    texture: metal::Texture,
    vertex_buffer: metal::Buffer,
}

/// Every layer's texture, keyed by layer id.
pub(crate) struct LayerResources {
    textures: LayerTextures<LayerTexture>,
    /// Where the current layer set sits, kept beside the textures so the draw pass
    /// and a pointer's hit test read one computation rather than two.
    placed: Vec<PlacedOverlayLayer>,
    /// The indices every layer quad shares.
    ///
    /// One buffer rather than one per layer: the six values are the same for every
    /// quad, and a per-layer index buffer would be a second allocation per panel to
    /// hold constants. Metal has no device-free buffer allocation, so this is
    /// created from the renderer's own device and belongs to it — which also means
    /// it is released with the renderer rather than living for the process.
    index_buffer: metal::Buffer,
}

impl LayerResources {
    pub(crate) fn new(device: &Device) -> Result<Self, OverlayError> {
        let indices = [0_u16, 1, 2, 0, 2, 3];
        let index_buffer = device.new_buffer_with_data(
            indices.as_ptr().cast(),
            size_of_val(&indices) as u64,
            MTLResourceOptions::StorageModeShared,
        );
        Ok(Self {
            textures: LayerTextures::new(),
            index_buffer,
            placed: Vec::new(),
        })
    }

    /// The textures a placed layer set needs, creating or refreshing what changed.
    ///
    /// Returns the same list when nothing changed, so the caller can draw without
    /// knowing whether an upload happened.
    pub(crate) fn prepare(&mut self, device: &Device, placed: &[PlacedOverlayLayer]) {
        for layer in placed {
            if !self.textures.is_current(&layer.layer) {
                // Dropped before the replacement is created: a size change means
                // the old texture cannot be reused, and holding both at once is a
                // texture's worth of memory for no frame.
                self.textures.remove(layer.layer.id);
                let texture = create_layer_texture(device, &layer.layer.raster);
                let vertices = layer.rect.vertices();
                let vertex_buffer = device.new_buffer_with_data(
                    vertices.as_ptr().cast(),
                    size_of_val(&vertices) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                self.textures.insert(
                    layer.layer.id,
                    LayerTexture {
                        texture,
                        vertex_buffer,
                    },
                    &layer.layer,
                );
            }
        }
        self.textures.retain(placed);
    }

    /// Place an incoming layer set and upload whatever changed.
    ///
    /// Called once per tick, before the draw, because preparing a texture needs
    /// `&mut self` and the draw pass borrows immutably. The placed list is kept
    /// here so the draw pass and a pointer's hit test agree on where every layer
    /// is, from one computation rather than two.
    pub(crate) fn set_layers(
        &mut self,
        device: &Device,
        layers: &[bongocat_render::OverlayLayer],
        published: &crate::layers::PlacedLayers,
    ) {
        let placed = crate::layers::place(layers);
        self.prepare(device, &placed);
        // Published after the textures are ready, so a click that lands between the
        // two is answered against a placement the next draw will show.
        published.publish(placed.clone());
        self.placed = placed;
    }

    /// The layers placed for the drawable currently on the layer.
    pub(crate) fn placed(&self) -> &[PlacedOverlayLayer] {
        &self.placed
    }

    /// The texture and vertex buffer for a layer.
    pub(crate) fn get(&self, id: u64) -> Option<(&metal::Texture, &metal::Buffer)> {
        self.textures
            .get(id)
            .map(|entry| (&entry.texture, &entry.vertex_buffer))
    }
}

/// Create a texture for one raster and fill it from the raster's pixels.
///
/// Straight RGBA8 to match `MODEL_TEXTURE_FORMAT`: the shader premultiplies once,
/// and a premultiplied source would darken every antialiased edge twice.
fn create_layer_texture(
    device: &Device,
    raster: &bongocat_render::OverlayLayerRaster,
) -> metal::Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MODEL_TEXTURE_FORMAT);
    descriptor.set_width(u64::from(raster.width));
    descriptor.set_height(u64::from(raster.height));
    // Shared storage: a layer is uploaded from memory a few times a second and read
    // by the GPU, which is the case shared storage is for. A private buffer would
    // need a staging copy per upload for no benefit at this size.
    descriptor.set_storage_mode(MTLStorageMode::Shared);
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    let texture = device.new_texture(&descriptor);
    texture.replace_region(
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(raster.width),
                height: u64::from(raster.height),
                depth: 1,
            },
        },
        0,
        raster.pixels.as_ptr().cast(),
        u64::from(raster.width) * 4,
    );
    texture
}

/// Draw every prepared layer, topmost, above the model and the key overlays.
///
/// The order and the reason are the same as the key overlays' one layer down: a
/// panel a user installed has to be visible, and a panel that is not visible is a
/// panel nobody can use.
pub(crate) fn draw_layers(
    encoder: &metal::RenderCommandEncoderRef,
    pipelines: &Pipelines,
    sampler: &metal::SamplerState,
    empty_mask: &metal::Texture,
    resources: &LayerResources,
    placed: &[PlacedOverlayLayer],
) {
    for layer in placed {
        let Some((texture, vertex_buffer)) = resources.get(layer.layer.id) else {
            continue;
        };
        encoder.set_cull_mode(MTLCullMode::None);
        // The layer quad is authored directly in NDC, so the transform is the
        // identity: multiplying it into the vertex positions would be a second
        // place the placement arithmetic lived.
        //
        // The window's corner radius is deliberately not applied. It is a property
        // of the model window's own box, and a panel is not the window — a panel
        // that inherited the window's rounding would show a second rounded edge
        // inside the first, which is the one visual artefact a host-rendered panel
        // must not have.
        let uniforms = Uniforms {
            scale_offset: [1.0, 1.0, 0.0, 0.0],
            multiply_color: [1.0, 1.0, 1.0, 1.0],
            screen_color: [0.0, 0.0, 0.0, 0.0],
            mask_settings: [0.0, 0.0, 0.0, 0.0],
            corner_radius: [0.0; 4],
            opacity: layer.layer.placement.opacity,
            padding: [0.0, 0.0, 0.0],
        };
        encoder.set_render_pipeline_state(&pipelines.normal);
        encoder.set_vertex_buffer(0, Some(vertex_buffer), 0);
        encoder.set_vertex_bytes(
            1,
            size_of::<Uniforms>() as u64,
            std::ptr::from_ref(&uniforms).cast(),
        );
        encoder.set_fragment_bytes(
            1,
            size_of::<Uniforms>() as u64,
            std::ptr::from_ref(&uniforms).cast(),
        );
        encoder.set_fragment_texture(0, Some(texture));
        encoder.set_fragment_texture(1, Some(empty_mask));
        encoder.set_fragment_sampler_state(0, Some(sampler));
        encoder.draw_indexed_primitives(
            MTLPrimitiveType::Triangle,
            6,
            MTLIndexType::UInt16,
            &resources.index_buffer,
            0,
        );
    }
}
