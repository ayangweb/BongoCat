//! The topmost layer pass, for D3D11.
//!
//! The same shape as the Metal one: one textured quad per layer, drawn after
//! everything else, positioned in normalized device coordinates so a panel is
//! interface chrome that does not mirror with the model.
//!
//! The difference between the backends is in the upload. Metal replaces a texture
//! region in place; D3D11 has no such operation on a texture this code would want
//! to own, so a layer whose pixels changed is given a **new** texture and the old
//! one is released. That is a coarser answer than Metal's — one allocation per
//! redraw rather than one write — and it is the right trade here: a panel's raster
//! changes at most a couple of times a second, a panel is at most a few hundred
//! kilobytes, and reusing the proven `create_texture_resource` keeps this file free
//! of the D3D11 creation code that would have to be got right twice.

use super::*;
use crate::layers::LayerTextures;
use bongocat_render::PlacedOverlayLayer;

/// One layer's texture and vertex buffer.
pub(crate) struct LayerTexture {
    resource: TextureResource,
    vertex_buffer: ID3D11Buffer,
}

/// Every layer's texture, keyed by layer id.
pub(crate) struct LayerResources {
    textures: LayerTextures<LayerTexture>,
    /// Where the current layer set sits, kept beside the textures so the draw pass
    /// and a pointer's hit test read one computation rather than two.
    placed: Vec<PlacedOverlayLayer>,
}

impl LayerResources {
    pub(crate) fn new() -> Self {
        Self {
            textures: LayerTextures::new(),
            placed: Vec::new(),
        }
    }

    /// Place an incoming layer set and upload whatever changed.
    ///
    /// Called once per tick, before the draw, because creating a texture needs
    /// `&mut self` and the draw pass borrows immutably.
    pub(crate) fn set_layers(
        &mut self,
        device: &ID3D11Device,
        layers: &[bongocat_render::OverlayLayer],
        published: &crate::layers::PlacedLayers,
    ) -> WindowsResult<()> {
        let placed = crate::layers::place(layers);
        self.prepare(device, &placed)?;
        // Published after the textures are ready, so a press that lands between the
        // two is answered against a placement the next draw will show.
        published.publish(placed.clone());
        self.placed = placed;
        Ok(())
    }

    /// The layers placed for the drawable currently on the swap chain.
    pub(crate) fn placed(&self) -> &[PlacedOverlayLayer] {
        &self.placed
    }

    fn prepare(
        &mut self,
        device: &ID3D11Device,
        placed: &[PlacedOverlayLayer],
    ) -> WindowsResult<()> {
        for layer in placed {
            if self.textures.is_current(&layer.layer) {
                continue;
            }
            // Released before the replacement is created: a texture cannot be
            // resized, and holding both at once is a texture's worth of memory for
            // no frame.
            self.textures.remove(layer.layer.id);
            // SAFETY: the descriptor and the initial data describe the same byte
            // length, and both COM objects are returned by value so they are
            // released with their Rust owners.
            unsafe {
                let resource = create_texture_resource(
                    device,
                    layer.layer.raster.width,
                    layer.layer.raster.height,
                    MODEL_TEXTURE_FORMAT,
                    layer.layer.raster.pixels.as_ptr(),
                    layer.layer.raster.width.saturating_mul(4),
                )?;
                let vertex_buffer = create_vertex_buffer(device, layer)?;
                self.textures.insert(
                    layer.layer.id,
                    LayerTexture {
                        resource,
                        vertex_buffer,
                    },
                    &layer.layer,
                );
            }
        }
        self.textures.retain(placed);
        Ok(())
    }

    /// The texture and vertex buffer for a layer.
    pub(crate) fn get(&self, id: u64) -> Option<(&TextureResource, &ID3D11Buffer)> {
        self.textures
            .get(id)
            .map(|entry| (&entry.resource, &entry.vertex_buffer))
    }
}

/// Upload one layer's four corners, already in normalized device coordinates.
///
/// `IMMEDIABLE` rather than `DYNAMIC` because the bytes are written once, at
/// creation, and never again: a layer's pixels changing means a new texture.
unsafe fn create_vertex_buffer(
    device: &ID3D11Device,
    placed: &PlacedOverlayLayer,
) -> WindowsResult<ID3D11Buffer> {
    let vertices = placed.rect.vertices();
    let byte_width = u32::try_from(size_of_val(&vertices))
        .map_err(|_| invariant_error("layer vertex buffer is too large"))?;
    let descriptor = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: vertices.as_ptr().cast(),
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&descriptor, Some(&data), Some(&mut buffer))? };
    required(buffer, "layer vertex buffer")
}

/// Draw every placed layer, topmost, above the model and the key overlays.
///
/// The order and the reason are the same as the key overlays' one layer down: a
/// panel a user installed has to be visible, and a panel nobody can see is a panel
/// nobody can use.
pub(crate) fn draw_layers(
    context: &ID3D11DeviceContext,
    pipelines: &Pipelines,
    index_buffer: &ID3D11Buffer,
    empty_mask: &TextureResource,
    resources: &LayerResources,
    placed: &[PlacedOverlayLayer],
) -> WindowsResult<()> {
    let stride = size_of::<bongocat_render::Vertex>() as u32;
    for layer in placed {
        let Some((resource, vertex_buffer)) = resources.get(layer.layer.id) else {
            continue;
        };
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
        // SAFETY: every interface belongs to this renderer and its current thread,
        // and the buffers and views outlive the immediate context.
        unsafe {
            context.RSSetState(&pipelines.rasterizer);
            context.OMSetBlendState(&pipelines.normal_blend, None, u32::MAX);
            context.UpdateSubresource(
                &pipelines.constant_buffer,
                0,
                None,
                std::ptr::from_ref(&uniforms).cast(),
                0,
                0,
            );
            let bound_vertex_buffer = Some(vertex_buffer.clone());
            let offset = 0_u32;
            context.IASetVertexBuffers(
                0,
                1,
                Some(&raw const bound_vertex_buffer),
                Some(&raw const stride),
                Some(&raw const offset),
            );
            context.IASetIndexBuffer(index_buffer, DXGI_FORMAT_R16_UINT, 0);
            context.PSSetShaderResources(
                0,
                Some(&[
                    Some(resource.shader_resource.clone()),
                    Some(empty_mask.shader_resource.clone()),
                ]),
            );
            context.DrawIndexed(6, 0, 0);
        }
    }
    Ok(())
}
