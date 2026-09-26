//! The model's GPU-resident resources.
//!
//! Textures, masks and the buffer a cover capture reads back from are created,
//! filled and released in one place, so the order Metal requires — upload, draw,
//! then wait for the command buffer before reading — stays visible in one file
//! rather than spread through the draw path.

use super::*;

pub(crate) const MODEL_TEXTURE_FORMAT: MTLPixelFormat = MTLPixelFormat::RGBA8Unorm;

pub(crate) const MASK_TEXTURE_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;

pub(crate) struct GpuModel {
    pub(crate) textures: BTreeMap<TextureId, Texture>,
    pub(crate) key_textures: BTreeMap<KeyAssetId, Texture>,
    pub(crate) background: Option<Texture>,
    pub(crate) background_vertex_buffer: Buffer,
    pub(crate) background_index_buffer: Buffer,
    pub(crate) meshes: Vec<Mesh>,
    pub(crate) empty_mask: Texture,
    pub(crate) bounds: ModelBounds,
    pub(crate) model_opacity: f32,
    pub(crate) mirror_horizontal: bool,
    pub(crate) active_keys: Vec<KeyOverlay>,
    pub(crate) masked_drawable_count: usize,
}

impl GpuModel {
    pub(crate) fn prepare(
        device: &Device,
        resources: &RenderResources,
        snapshot: &RenderSnapshot,
        drawable_width: u64,
        drawable_height: u64,
    ) -> Result<Self, OverlayError> {
        validate_render_snapshot(resources, snapshot)
            .map_err(|error| OverlayError::new(error.to_string()))?;
        let textures = resources
            .textures
            .iter()
            .map(|asset| load_texture(device, asset).map(|texture| (asset.id, texture)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let key_textures = resources
            .key_assets
            .iter()
            .map(|asset| {
                load_texture(
                    device,
                    &TextureAsset {
                        id: TextureId::new(asset.id.index()),
                        path: asset.path.clone(),
                        width: asset.width,
                        height: asset.height,
                    },
                )
                .map(|texture| (asset.id, texture))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let background = resources
            .background
            .as_ref()
            .map(|asset| {
                load_texture(
                    device,
                    &TextureAsset {
                        id: TextureId::new(usize::MAX),
                        path: asset.path.clone(),
                        width: asset.width,
                        height: asset.height,
                    },
                )
            })
            .transpose()?;
        let canvas_bounds = ModelBounds::from_canvas(snapshot.canvas);
        let background_vertices = [
            bongocat_render::Vertex {
                position: [canvas_bounds.min_x, canvas_bounds.min_y],
                uv: [0.0, 0.0],
            },
            bongocat_render::Vertex {
                position: [canvas_bounds.max_x, canvas_bounds.min_y],
                uv: [1.0, 0.0],
            },
            bongocat_render::Vertex {
                position: [canvas_bounds.max_x, canvas_bounds.max_y],
                uv: [1.0, 1.0],
            },
            bongocat_render::Vertex {
                position: [canvas_bounds.min_x, canvas_bounds.max_y],
                uv: [0.0, 1.0],
            },
        ];
        let background_indices = [0_u16, 1, 2, 0, 2, 3];
        let mut meshes = snapshot
            .drawables
            .iter()
            .map(|drawable| {
                // Metal rejects a zero-length buffer, and a drawable the Core
                // reports without triangles has nothing to upload. Both arrays
                // therefore fall back to a single placeholder element, while
                // `vertex_bytes` keeps the length of the snapshot's own array so
                // `sync_snapshot` still notices a changed vertex count. Drawing
                // zero triangles is then a no-op, which is what leaves the mask
                // buffer such a mesh fills at the value it was cleared to.
                let vertex_bytes = std::mem::size_of_val(drawable.vertices.as_slice());
                let placeholder_vertex = [bongocat_render::Vertex {
                    position: [0.0, 0.0],
                    uv: [0.0, 0.0],
                }];
                let placeholder_index = [0_u16];
                let vertices: &[bongocat_render::Vertex] = if drawable.vertices.is_empty() {
                    &placeholder_vertex
                } else {
                    &drawable.vertices
                };
                let indices: &[u16] = if drawable.indices.is_empty() {
                    &placeholder_index
                } else {
                    &drawable.indices
                };
                let vertex_buffer = device.new_buffer_with_data(
                    vertices.as_ptr().cast(),
                    std::mem::size_of_val(vertices) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                let index_buffer = device.new_buffer_with_data(
                    indices.as_ptr().cast(),
                    std::mem::size_of_val(indices) as u64,
                    MTLResourceOptions::StorageModeShared,
                );
                Mesh {
                    id: drawable.id,
                    render_order: drawable.render_order,
                    vertex_buffer,
                    vertex_bytes,
                    index_buffer,
                    indices: drawable.indices.clone(),
                    index_count: drawable.indices.len() as u64,
                    texture_id: drawable.texture_id,
                    opacity: drawable.opacity,
                    blend_mode: drawable.blend_mode,
                    multiply_color: drawable.multiply_color,
                    screen_color: drawable.screen_color,
                    masks: drawable.masks.clone(),
                    visible: drawable.visible,
                    double_sided: drawable.double_sided,
                    inverted_mask: drawable.inverted_mask,
                    mask_texture: (!drawable.masks.is_empty())
                        .then(|| create_mask_texture(device, drawable_width, drawable_height)),
                }
            })
            .collect::<Vec<_>>();
        meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        Ok(Self {
            textures,
            key_textures,
            background,
            background_vertex_buffer: device.new_buffer_with_data(
                background_vertices.as_ptr().cast(),
                std::mem::size_of_val(&background_vertices) as u64,
                MTLResourceOptions::StorageModeShared,
            ),
            background_index_buffer: device.new_buffer_with_data(
                background_indices.as_ptr().cast(),
                std::mem::size_of_val(&background_indices) as u64,
                MTLResourceOptions::StorageModeShared,
            ),
            meshes,
            empty_mask: create_solid_mask_texture(device),
            bounds: snapshot.bounds,
            model_opacity: snapshot.model_opacity,
            mirror_horizontal: snapshot.mirror_horizontal,
            active_keys: snapshot.active_keys.clone(),
            masked_drawable_count: snapshot
                .drawables
                .iter()
                .filter(|drawable| !drawable.masks.is_empty())
                .count(),
        })
    }

    pub(crate) fn sync_snapshot(&mut self, snapshot: &RenderSnapshot) -> Result<(), OverlayError> {
        if !snapshot.model_opacity.is_finite() || !(0.0..=1.0).contains(&snapshot.model_opacity) {
            return Err(OverlayError::new("model opacity is outside [0, 1]"));
        }
        if snapshot.drawables.len() != self.meshes.len() {
            return Err(OverlayError::new(format!(
                "drawable count changed from {} to {}",
                self.meshes.len(),
                snapshot.drawables.len()
            )));
        }
        for drawable in &snapshot.drawables {
            let mesh = self
                .meshes
                .iter_mut()
                .find(|mesh| mesh.id == drawable.id)
                .ok_or_else(|| {
                    OverlayError::new(format!("drawable source {} is unavailable", drawable.id))
                })?;
            if mesh.mask_texture.is_some() != !drawable.masks.is_empty() {
                return Err(OverlayError::new(format!(
                    "drawable {} changed clipping topology",
                    drawable.id
                )));
            }
            if mesh.indices != drawable.indices {
                return Err(OverlayError::new(format!(
                    "drawable {} changed immutable triangle indices",
                    drawable.id
                )));
            }
            if mesh.vertex_bytes != std::mem::size_of_val(drawable.vertices.as_slice()) {
                return Err(OverlayError::new(format!(
                    "drawable {} changed vertex buffer size",
                    drawable.id
                )));
            }
            if drawable.dynamic_flags.vertex_positions_changed && !drawable.vertices.is_empty() {
                upload_slice(&mesh.vertex_buffer, &drawable.vertices, "vertices")?;
            }
            mesh.render_order = drawable.render_order;
            mesh.texture_id = drawable.texture_id;
            mesh.opacity = drawable.opacity;
            mesh.blend_mode = drawable.blend_mode;
            mesh.multiply_color = drawable.multiply_color;
            mesh.screen_color = drawable.screen_color;
            mesh.masks.clone_from(&drawable.masks);
            mesh.visible = drawable.visible;
            mesh.double_sided = drawable.double_sided;
            mesh.inverted_mask = drawable.inverted_mask;
        }
        self.meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        self.bounds = snapshot.bounds;
        self.active_keys.clone_from(&snapshot.active_keys);
        self.model_opacity = snapshot.model_opacity;
        self.mirror_horizontal = snapshot.mirror_horizontal;
        Ok(())
    }

    /// Re-create every mask target at a new drawable size.
    ///
    /// A mask pass renders at the drawable size, so a window resize invalidates
    /// those textures even though the meshes they clip do not change. Textures,
    /// vertex and index buffers, bounds and the solid mask are all
    /// size-independent and stay as they are, which is what keeps a resize from
    /// reloading every model asset.
    pub(crate) fn resize_masks(&mut self, device: &Device, width: u64, height: u64) {
        for mesh in &mut self.meshes {
            if mesh.mask_texture.is_some() {
                mesh.mask_texture = Some(create_mask_texture(device, width, height));
            }
        }
    }
}

pub(crate) fn upload_slice<T>(
    buffer: &Buffer,
    values: &[T],
    name: &str,
) -> Result<(), OverlayError> {
    let bytes = std::mem::size_of_val(values) as u64;
    if buffer.length() != bytes {
        return Err(OverlayError::new(format!(
            "{name} buffer size changed from {} to {bytes}",
            buffer.length()
        )));
    }
    if bytes == 0 {
        return Ok(());
    }
    // SAFETY: StorageModeShared exposes a writable CPU mapping for the full
    // fixed-size buffer, and source and destination cannot overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(
            values.as_ptr().cast::<u8>(),
            buffer.contents().cast::<u8>(),
            bytes as usize,
        )
    };
    Ok(())
}

pub(crate) fn create_mask_texture(device: &Device, width: u64, height: u64) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MASK_TEXTURE_FORMAT);
    descriptor.set_width(width);
    descriptor.set_height(height);
    descriptor.set_storage_mode(MTLStorageMode::Private);
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    device.new_texture(&descriptor)
}

pub(crate) fn create_solid_mask_texture(device: &Device) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MASK_TEXTURE_FORMAT);
    descriptor.set_width(1);
    descriptor.set_height(1);
    descriptor.set_storage_mode(MTLStorageMode::Shared);
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    let texture = device.new_texture(&descriptor);
    let pixel = [0_u8; 4];
    texture.replace_region(
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: 1,
                height: 1,
                depth: 1,
            },
        },
        0,
        pixel.as_ptr().cast(),
        4,
    );
    texture
}

pub(crate) fn load_texture(device: &Device, asset: &TextureAsset) -> Result<Texture, OverlayError> {
    let image = ImageReader::open(&asset.path)
        .map_err(|error| OverlayError::new(format!("open {}: {error}", asset.path.display())))?
        .decode()
        .map_err(|error| OverlayError::new(format!("decode {}: {error}", asset.path.display())))?
        .into_rgba8();
    if image.width() != asset.width || image.height() != asset.height {
        return Err(OverlayError::new(format!(
            "texture dimensions changed for {}",
            asset.path.display()
        )));
    }
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MODEL_TEXTURE_FORMAT);
    descriptor.set_width(u64::from(asset.width));
    descriptor.set_height(u64::from(asset.height));
    descriptor.set_storage_mode(MTLStorageMode::Shared);
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    let texture = device.new_texture(&descriptor);
    texture.replace_region(
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: u64::from(asset.width),
                height: u64::from(asset.height),
                depth: 1,
            },
        },
        0,
        image.as_ptr().cast(),
        u64::from(asset.width) * 4,
    );
    Ok(texture)
}

/// Read one frame's pixels out of a drawable texture.
///
/// This is the full-frame form of [`verify_frame_smoke`]: same readback, every
/// pixel of it. The drawable is BGRA and uses the same encoded values as the
/// renderer's compatibility contract, so the bytes are what the compositor
/// receives rather than a hidden linear-space copy and the cover matches what
/// the overlay shows.
pub(crate) fn read_drawable_frame(
    texture: &metal::TextureRef,
) -> Result<CapturedFrame, OverlayError> {
    let width = u32::try_from(texture.width())
        .map_err(|_| OverlayError::new("Metal drawable width is out of range"))?;
    let height = u32::try_from(texture.height())
        .map_err(|_| OverlayError::new("Metal drawable height is out of range"))?;
    let row_bytes = width as usize * 4;
    let mut bytes = vec![0_u8; row_bytes * height as usize];
    texture.get_bytes(
        bytes.as_mut_ptr().cast(),
        row_bytes as u64,
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: texture.width(),
                height: texture.height(),
                depth: 1,
            },
        },
        0,
    );
    CapturedFrame::from_premultiplied_bgra(&bytes, row_bytes, width, height)
}

pub(crate) fn verify_frame_smoke(texture: &metal::TextureRef) -> Result<(), OverlayError> {
    let width = texture.width();
    let height = texture.height();
    let mut pixels =
        Vec::with_capacity((FRAME_SMOKE_GRID_DIMENSION * FRAME_SMOKE_GRID_DIMENSION) as usize);
    for y in 0..FRAME_SMOKE_GRID_DIMENSION {
        for x in 0..FRAME_SMOKE_GRID_DIMENSION {
            let mut pixel = [0_u8; 4];
            texture.get_bytes(
                pixel.as_mut_ptr().cast(),
                4,
                MTLRegion {
                    origin: MTLOrigin {
                        x: width.saturating_sub(1) * x / (FRAME_SMOKE_GRID_DIMENSION - 1),
                        y: height.saturating_sub(1) * y / (FRAME_SMOKE_GRID_DIMENSION - 1),
                        z: 0,
                    },
                    size: MTLSize {
                        width: 1,
                        height: 1,
                        depth: 1,
                    },
                },
                0,
            );
            pixels.push(pixel);
        }
    }
    validate_frame_smoke(pixels).map_err(|error| OverlayError::new(format!("Metal {error}")))?;
    Ok(())
}
