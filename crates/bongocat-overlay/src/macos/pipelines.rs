//! The shader and the pipeline state the frame is drawn with.
//!
//! The colour formats follow the encoded-space contract in ADR-0063: an sRGB
//! texture view must not decode the source and an sRGB drawable must not
//! re-encode the composited frame, or the overlay's colours would differ from
//! the Windows backend's for the same model.

use super::*;

// The renderer follows the encoded-space compatibility contract in ADR-0063.
// Keep the same encoded RGB values on both native backends: do not let an
// sRGB texture view decode the source or an sRGB drawable re-encode the
// composited frame. Alpha is still premultiplied for the window compositor,
// and masks carry coverage only.
pub(crate) const COLOR_ATTACHMENT_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;

pub(crate) const SHADER_SOURCE: &str = r#"
    #include <metal_stdlib>
    using namespace metal;

    struct Vertex {
        float2 position;
        float2 uv;
    };

    struct Uniforms {
        float4 scale_offset;
        float4 multiply_color;
        float4 screen_color;
        float4 mask_settings;
        float4 corner_radius;
        // Model/drawable opacity only. Window presentation opacity is applied
        // once to the completed panel surface.
        float opacity;
        float3 padding;
    };

    struct RasterVertex {
        float4 position [[position]];
        float2 uv;
    };

    // Legacy window rounding. `corner_radius.x` is the configured radius as a
    // fraction of the window box, and `corner_radius.yz` are the drawable
    // dimensions. The corner arcs stay elliptical on a non-square window,
    // exactly like a CSS percentage `border-radius`.
    float corner_coverage(float2 position, float4 corner_radius) {
        float radius = min(corner_radius.x, 0.5);
        if (radius <= 0.0) {
            return 1.0;
        }
        float2 uv = position / corner_radius.yz;
        float2 centered = abs(uv * 2.0 - 1.0);
        float extent = 2.0 * radius;
        float2 delta = centered - 1.0 + extent;
        float distance = length(max(delta, 0.0)) + min(max(delta.x, delta.y), 0.0) - extent;
        // Convert the signed distance to device pixels using the smaller
        // drawable dimension, so the antialiased band never narrows below one
        // pixel on the longer axis.
        float scale = 0.5 * min(corner_radius.y, corner_radius.z);
        return saturate(0.5 - distance * scale);
    }

    vertex RasterVertex cubism_vertex(
        const device Vertex* vertices [[buffer(0)]],
        constant Uniforms& uniforms [[buffer(1)]],
        uint vertex_id [[vertex_id]]
    ) {
        RasterVertex output;
        float2 clip = vertices[vertex_id].position * uniforms.scale_offset.xy
                    + uniforms.scale_offset.zw;
        output.position = float4(clip, 0.0, 1.0);
        output.uv = vertices[vertex_id].uv;
        output.uv.y = 1.0 - output.uv.y;
        return output;
    }

    // The source texture is an ordinary UNORM view on purpose. This is the
    // encoded-space compatibility blend: do not insert a linear/sRGB conversion
    // here without changing both backends and the product contract.
    fragment float4 cubism_fragment(
        RasterVertex input [[stage_in]],
        texture2d<float> model_texture [[texture(0)]],
        texture2d<float> mask_texture [[texture(1)]],
        sampler texture_sampler [[sampler(0)]],
        constant Uniforms& uniforms [[buffer(1)]]
    ) {
        float4 texture_color = model_texture.sample(texture_sampler, input.uv);
        float3 color = texture_color.rgb * uniforms.multiply_color.rgb;
        color = color + uniforms.screen_color.rgb - color * uniforms.screen_color.rgb;
        float mask = 1.0;
        if (uniforms.mask_settings.z > 0.5) {
            float2 mask_uv = input.position.xy / uniforms.mask_settings.xy;
            mask = mask_texture.sample(texture_sampler, mask_uv).a;
            if (uniforms.mask_settings.w > 0.5) {
                mask = 1.0 - mask;
            }
        }
        float alpha = texture_color.a * uniforms.opacity * mask
                    * corner_coverage(input.position.xy, uniforms.corner_radius);
        return float4(color * alpha, alpha);
    }

    fragment float4 cubism_mask_fragment(
        RasterVertex input [[stage_in]],
        texture2d<float> model_texture [[texture(0)]],
        sampler texture_sampler [[sampler(0)]]
    ) {
        float alpha = model_texture.sample(texture_sampler, input.uv).a;
        return float4(0.0, 0.0, 0.0, alpha);
    }
"#;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Uniforms {
    pub(crate) scale_offset: [f32; 4],
    pub(crate) multiply_color: [f32; 4],
    pub(crate) screen_color: [f32; 4],
    pub(crate) mask_settings: [f32; 4],
    pub(crate) corner_radius: [f32; 4],
    pub(crate) opacity: f32,
    pub(crate) padding: [f32; 3],
}

pub(crate) struct Mesh {
    pub(crate) id: DrawableId,
    pub(crate) render_order: i32,
    pub(crate) vertex_buffer: Buffer,
    /// Byte length of the snapshot's own vertex array. It is what the buffer
    /// holds unless the drawable carries no vertices at all, in which case the
    /// buffer is a placeholder and this stays zero.
    pub(crate) vertex_bytes: usize,
    pub(crate) index_buffer: Buffer,
    pub(crate) indices: Vec<u16>,
    pub(crate) index_count: u64,
    pub(crate) texture_id: TextureId,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: BlendMode,
    pub(crate) multiply_color: [f32; 4],
    pub(crate) screen_color: [f32; 4],
    pub(crate) masks: Vec<DrawableId>,
    pub(crate) visible: bool,
    pub(crate) double_sided: bool,
    pub(crate) inverted_mask: bool,
    pub(crate) mask_texture: Option<Texture>,
}

pub(crate) struct Pipelines {
    pub(crate) normal: RenderPipelineState,
    pub(crate) additive: RenderPipelineState,
    pub(crate) multiplicative: RenderPipelineState,
    pub(crate) mask: RenderPipelineState,
}

impl Pipelines {
    pub(crate) fn for_mode(&self, mode: BlendMode) -> &RenderPipelineState {
        match mode {
            BlendMode::Normal => &self.normal,
            BlendMode::Additive => &self.additive,
            BlendMode::Multiplicative => &self.multiplicative,
        }
    }
}

pub(crate) fn create_pipelines(device: &Device) -> Result<Pipelines, OverlayError> {
    let library = device
        .new_library_with_source(SHADER_SOURCE, &CompileOptions::new())
        .map_err(|error| OverlayError::new(format!("compile Metal shaders: {error}")))?;
    let vertex = library
        .get_function("cubism_vertex", None)
        .map_err(|error| OverlayError::new(format!("load vertex shader: {error}")))?;
    let fragment = library
        .get_function("cubism_fragment", None)
        .map_err(|error| OverlayError::new(format!("load fragment shader: {error}")))?;
    let mask_fragment = library
        .get_function("cubism_mask_fragment", None)
        .map_err(|error| OverlayError::new(format!("load mask fragment shader: {error}")))?;
    Ok(Pipelines {
        normal: create_pipeline(device, &vertex, &fragment, BlendMode::Normal)?,
        additive: create_pipeline(device, &vertex, &fragment, BlendMode::Additive)?,
        multiplicative: create_pipeline(device, &vertex, &fragment, BlendMode::Multiplicative)?,
        mask: create_mask_pipeline(device, &vertex, &mask_fragment)?,
    })
}

pub(crate) fn create_pipeline(
    device: &Device,
    vertex: &metal::FunctionRef,
    fragment: &metal::FunctionRef,
    mode: BlendMode,
) -> Result<RenderPipelineState, OverlayError> {
    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(vertex));
    descriptor.set_fragment_function(Some(fragment));
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| OverlayError::new("Metal pipeline color attachment is unavailable"))?;
    attachment.set_pixel_format(COLOR_ATTACHMENT_FORMAT);
    attachment.set_blending_enabled(true);
    let factors = blend_factors(mode);
    attachment.set_source_rgb_blend_factor(metal_blend_factor(factors.source_rgb));
    attachment.set_destination_rgb_blend_factor(metal_blend_factor(factors.destination_rgb));
    attachment.set_source_alpha_blend_factor(metal_blend_factor(factors.source_alpha));
    attachment.set_destination_alpha_blend_factor(metal_blend_factor(factors.destination_alpha));
    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| OverlayError::new(format!("create Metal pipeline: {error}")))
}

pub(crate) fn create_mask_pipeline(
    device: &Device,
    vertex: &metal::FunctionRef,
    fragment: &metal::FunctionRef,
) -> Result<RenderPipelineState, OverlayError> {
    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(vertex));
    descriptor.set_fragment_function(Some(fragment));
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| OverlayError::new("Metal mask pipeline attachment is unavailable"))?;
    attachment.set_pixel_format(MASK_TEXTURE_FORMAT);
    attachment.set_blending_enabled(true);
    let factors = blend_factors(BlendMode::Normal);
    attachment.set_source_rgb_blend_factor(metal_blend_factor(factors.source_rgb));
    attachment.set_destination_rgb_blend_factor(metal_blend_factor(factors.destination_rgb));
    attachment.set_source_alpha_blend_factor(metal_blend_factor(factors.source_alpha));
    attachment.set_destination_alpha_blend_factor(metal_blend_factor(factors.destination_alpha));
    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| OverlayError::new(format!("create Metal mask pipeline: {error}")))
}

pub(crate) const fn metal_blend_factor(factor: BlendFactor) -> MTLBlendFactor {
    match factor {
        BlendFactor::Zero => MTLBlendFactor::Zero,
        BlendFactor::One => MTLBlendFactor::One,
        BlendFactor::OneMinusSourceAlpha => MTLBlendFactor::OneMinusSourceAlpha,
        BlendFactor::DestinationColor => MTLBlendFactor::DestinationColor,
    }
}

pub(crate) fn metal_cull_mode(double_sided: bool, mirror_horizontal: bool) -> MTLCullMode {
    match drawable_cull_mode(double_sided, mirror_horizontal) {
        DrawableCullMode::None => MTLCullMode::None,
        DrawableCullMode::Front => MTLCullMode::Front,
        DrawableCullMode::Back => MTLCullMode::Back,
    }
}
