//! The model's GPU-resident resources.
//!
//! Textures, masks and the staging surface a cover capture reads back from are
//! created, filled and released in one place, so the order the D3D11 context
//! requires — upload, copy, then release the mapped staging surface — is
//! visible in one file rather than spread through the draw path.

use super::*;

pub(crate) struct MaskTarget {
    pub(crate) _texture: ID3D11Texture2D,
    pub(crate) render_target: ID3D11RenderTargetView,
    pub(crate) shader_resource: ID3D11ShaderResourceView,
}

pub(crate) struct TextureResource {
    pub(crate) _texture: ID3D11Texture2D,
    pub(crate) shader_resource: ID3D11ShaderResourceView,
}

pub(crate) struct GpuModel {
    pub(crate) textures: BTreeMap<TextureId, TextureResource>,
    pub(crate) key_textures: BTreeMap<KeyAssetId, TextureResource>,
    pub(crate) background: Option<TextureResource>,
    pub(crate) background_vertex_buffer: ID3D11Buffer,
    pub(crate) background_index_buffer: ID3D11Buffer,
    pub(crate) meshes: Vec<Mesh>,
    pub(crate) empty_mask: TextureResource,
    pub(crate) bounds: ModelBounds,
    pub(crate) model_opacity: f32,
    pub(crate) mirror_horizontal: bool,
    pub(crate) active_keys: Vec<KeyOverlay>,
    pub(crate) masked_drawable_count: usize,
}

impl GpuModel {
    pub(crate) unsafe fn prepare(
        device: &ID3D11Device,
        resources: &RenderResources,
        snapshot: &RenderSnapshot,
        width: u32,
        height: u32,
    ) -> WindowsResult<Self> {
        validate_render_snapshot(resources, snapshot)
            .map_err(|error| invariant_error(error.message()))?;
        let textures = resources
            .textures
            .iter()
            .map(|asset| unsafe { load_texture(device, asset) }.map(|texture| (asset.id, texture)))
            .collect::<WindowsResult<BTreeMap<_, _>>>()?;
        let key_textures = resources
            .key_assets
            .iter()
            .map(|asset| {
                let texture = TextureAsset {
                    id: TextureId::new(asset.id.index()),
                    path: asset.path.clone(),
                    width: asset.width,
                    height: asset.height,
                };
                unsafe { load_texture(device, &texture) }.map(|texture| (asset.id, texture))
            })
            .collect::<WindowsResult<BTreeMap<_, _>>>()?;
        let background = resources
            .background
            .as_ref()
            .map(|asset| unsafe {
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
        let mut meshes = Vec::with_capacity(snapshot.drawables.len());
        for drawable in &snapshot.drawables {
            // D3D11 rejects a zero-byte buffer, and a drawable the Core reports
            // without triangles has nothing to upload. Both arrays therefore
            // fall back to a single placeholder element, while `vertex_bytes`
            // and `index_bytes` keep the lengths of the snapshot's own arrays so
            // the update path still notices a changed vertex count. Drawing zero
            // indices is a no-op, which is what leaves the mask target such a
            // mesh fills at the value it was cleared to.
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
            let vertex_buffer =
                unsafe { create_buffer(device, vertices, D3D11_BIND_VERTEX_BUFFER.0 as u32)? };
            let index_buffer =
                unsafe { create_buffer(device, indices, D3D11_BIND_INDEX_BUFFER.0 as u32)? };
            meshes.push(Mesh {
                id: drawable.id,
                render_order: drawable.render_order,
                vertex_buffer,
                vertex_bytes: size_of_val(drawable.vertices.as_slice()),
                index_buffer,
                index_bytes: size_of_val(drawable.indices.as_slice()),
                indices: drawable.indices.clone(),
                index_count: drawable.indices.len() as u32,
                texture_id: drawable.texture_id,
                opacity: drawable.opacity,
                blend_mode: drawable.blend_mode,
                multiply_color: drawable.multiply_color,
                screen_color: drawable.screen_color,
                masks: drawable.masks.clone(),
                visible: drawable.visible,
                double_sided: drawable.double_sided,
                inverted_mask: drawable.inverted_mask,
                mask_target: if drawable.masks.is_empty() {
                    None
                } else {
                    Some(unsafe { create_mask_target(device, width, height)? })
                },
            });
        }
        meshes.sort_by_key(|mesh| (mesh.render_order, mesh.id));
        Ok(Self {
            textures,
            key_textures,
            background,
            background_vertex_buffer: unsafe {
                create_buffer(
                    device,
                    &background_vertices,
                    D3D11_BIND_VERTEX_BUFFER.0 as u32,
                )?
            },
            background_index_buffer: unsafe {
                create_buffer(
                    device,
                    &background_indices,
                    D3D11_BIND_INDEX_BUFFER.0 as u32,
                )?
            },
            meshes,
            empty_mask: unsafe { create_empty_mask(device)? },
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

    pub(crate) unsafe fn sync_snapshot(
        &mut self,
        context: &ID3D11DeviceContext,
        snapshot: &RenderSnapshot,
    ) -> WindowsResult<()> {
        if !snapshot.model_opacity.is_finite() || !(0.0..=1.0).contains(&snapshot.model_opacity) {
            return Err(invariant_error("model opacity is outside [0, 1]"));
        }
        if snapshot.drawables.len() != self.meshes.len() {
            return Err(invariant_error(
                "drawable count changed within a generation",
            ));
        }
        for drawable in &snapshot.drawables {
            let mesh = self
                .meshes
                .iter_mut()
                .find(|mesh| mesh.id == drawable.id)
                .ok_or_else(|| invariant_error("drawable source is unavailable"))?;
            if mesh.vertex_bytes != size_of_val(drawable.vertices.as_slice()) {
                return Err(invariant_error("drawable vertex buffer size changed"));
            }
            if mesh.index_bytes != size_of_val(drawable.indices.as_slice())
                || mesh.indices != drawable.indices
            {
                return Err(invariant_error("drawable triangle indices changed"));
            }
            if mesh.mask_target.is_some() != !drawable.masks.is_empty() {
                return Err(invariant_error("drawable clipping topology changed"));
            }
            if drawable.dynamic_flags.vertex_positions_changed && !drawable.vertices.is_empty() {
                // SAFETY: the vertex buffer is validated to match the immutable
                // snapshot geometry, and Core marked its positions as changed.
                unsafe {
                    context.UpdateSubresource(
                        &mesh.vertex_buffer,
                        0,
                        None,
                        drawable.vertices.as_ptr().cast(),
                        0,
                        0,
                    );
                }
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
}

pub(crate) unsafe fn create_buffer<T>(
    device: &ID3D11Device,
    values: &[T],
    bind_flags: u32,
) -> WindowsResult<ID3D11Buffer> {
    let byte_width = u32::try_from(size_of_val(values))
        .map_err(|_| invariant_error("GPU buffer exceeds D3D11 size limits"))?;
    if byte_width == 0 {
        return Err(invariant_error("GPU buffer cannot be empty"));
    }
    let descriptor = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind_flags,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: values.as_ptr().cast(),
        ..Default::default()
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&descriptor, Some(&data), Some(&mut buffer))? };
    required(buffer, "GPU buffer")
}

pub(crate) unsafe fn load_texture(
    device: &ID3D11Device,
    asset: &TextureAsset,
) -> WindowsResult<TextureResource> {
    let image = ImageReader::open(&asset.path)
        .map_err(|error| Error::new(HRESULT(0x80004005_u32 as i32), error.to_string()))?
        .decode()
        .map_err(|error| Error::new(HRESULT(0x80004005_u32 as i32), error.to_string()))?
        .into_rgba8();
    if image.width() != asset.width || image.height() != asset.height {
        return Err(invariant_error(
            "texture dimensions changed after validation",
        ));
    }
    unsafe {
        create_texture_resource(
            device,
            asset.width,
            asset.height,
            MODEL_TEXTURE_FORMAT,
            image.as_ptr(),
            asset.width.saturating_mul(4),
        )
    }
}

pub(crate) unsafe fn create_empty_mask(device: &ID3D11Device) -> WindowsResult<TextureResource> {
    let pixel = [0_u8; 4];
    unsafe { create_texture_resource(device, 1, 1, MASK_TEXTURE_FORMAT, pixel.as_ptr(), 4) }
}

pub(crate) unsafe fn create_texture_resource(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    bytes: *const u8,
    row_pitch: u32,
) -> WindowsResult<TextureResource> {
    let descriptor = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        ..Default::default()
    };
    let data = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.cast(),
        SysMemPitch: row_pitch,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&descriptor, Some(&data), Some(&mut texture))? };
    let texture = required(texture, "texture")?;
    let mut shader_resource = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource))? };
    Ok(TextureResource {
        _texture: texture,
        shader_resource: required(shader_resource, "texture shader resource")?,
    })
}

pub(crate) unsafe fn create_mask_target(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> WindowsResult<MaskTarget> {
    let descriptor = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: MASK_TEXTURE_FORMAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut texture))? };
    let texture = required(texture, "mask texture")?;
    let render_target = unsafe { create_render_target(device, &texture, MASK_TEXTURE_FORMAT)? };
    let mut shader_resource = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource))? };
    Ok(MaskTarget {
        _texture: texture,
        render_target,
        shader_resource: required(shader_resource, "mask shader resource")?,
    })
}

/// Create a render target view, possibly with a format the texture itself does
/// not carry.
///
/// A view may name any format in the same family as the resource. The explicit
/// descriptor keeps the composition and mask paths pinned to their respective
/// UNORM formats instead of inheriting an unrelated view format by accident.
/// `..Default::default()` zeroes the descriptor union, selecting its
/// `Texture2D { MipSlice: 0 }` member.
pub(crate) unsafe fn create_render_target(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> WindowsResult<ID3D11RenderTargetView> {
    let descriptor = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let mut target = None;
    unsafe { device.CreateRenderTargetView(texture, Some(&descriptor), Some(&mut target))? };
    required(target, "render target")
}

pub(crate) unsafe fn create_staging_texture(
    device: &ID3D11Device,
    source: &ID3D11Texture2D,
) -> WindowsResult<ID3D11Texture2D> {
    let mut descriptor = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut descriptor) };
    descriptor.Usage = D3D11_USAGE_STAGING;
    descriptor.BindFlags = 0;
    descriptor.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    descriptor.MiscFlags = 0;
    let mut staging = None;
    unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut staging))? };
    required(staging, "staging texture")
}

/// Read one frame's pixels out of a staging texture.
///
/// This is the full-frame form of [`verify_frame_smoke`]: same mapping, every row
/// of it. The staging texture keeps the back buffer's BGRA layout and its own row
/// pitch, so each row is copied into a tightly packed buffer that
/// [`CapturedFrame::from_premultiplied_bgra`] can take. The back buffer is
/// UNORM, so the bytes are the same encoded values the overlay presents rather
/// than a hidden linear-space copy, and the cover is encoded from exactly what
/// the overlay shows.
pub(crate) unsafe fn read_staging_frame(
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> WindowsResult<CapturedFrame> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
    let result = if mapped.pData.is_null() || mapped.RowPitch < width.saturating_mul(4) {
        Err(invariant_error("D3D11 readback mapping is invalid"))
    } else {
        let row_bytes = width as usize * 4;
        let mut bytes = vec![0_u8; row_bytes * height as usize];
        for (row, destination) in bytes.chunks_exact_mut(row_bytes).enumerate() {
            // SAFETY: the mapping was checked to hold a four-byte BGRA pixel for
            // every column, so this row starts inside it and one row fits.
            let source = unsafe {
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize)
            };
            unsafe { std::ptr::copy_nonoverlapping(source, destination.as_mut_ptr(), row_bytes) };
        }
        CapturedFrame::from_premultiplied_bgra(&bytes, row_bytes, width, height)
            .map_err(|error| invariant_error(&error.to_string()))
    };
    unsafe { context.Unmap(texture, 0) };
    result
}

pub(crate) unsafe fn verify_frame_smoke(
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> WindowsResult<()> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
    let result = if mapped.pData.is_null() || mapped.RowPitch < width.saturating_mul(4) {
        Err(invariant_error("D3D11 readback mapping is invalid"))
    } else {
        let mut pixels =
            Vec::with_capacity((FRAME_SMOKE_GRID_DIMENSION * FRAME_SMOKE_GRID_DIMENSION) as usize);
        for y in 0..FRAME_SMOKE_GRID_DIMENSION as usize {
            for x in 0..FRAME_SMOKE_GRID_DIMENSION as usize {
                let offset = (height.saturating_sub(1) as usize * y
                    / (FRAME_SMOKE_GRID_DIMENSION as usize - 1))
                    * mapped.RowPitch as usize
                    + (width.saturating_sub(1) as usize * x
                        / (FRAME_SMOKE_GRID_DIMENSION as usize - 1))
                        * 4;
                // SAFETY: grid coordinates are inside width/height and RowPitch
                // was checked to contain every four-byte BGRA pixel in a row.
                let pointer = unsafe { mapped.pData.cast::<u8>().add(offset) };
                let pixel =
                    unsafe { [*pointer, *pointer.add(1), *pointer.add(2), *pointer.add(3)] };
                pixels.push(pixel);
            }
        }
        validate_frame_smoke(pixels).map_err(invariant_error)
    };
    unsafe { context.Unmap(texture, 0) };
    result.map(|_| ())
}
