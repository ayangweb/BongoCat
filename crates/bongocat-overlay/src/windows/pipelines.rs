//! The shader and the fixed-function state the frame is drawn with.
//!
//! Culling is reversed because Cubism models are authored counter-clockwise
//! front-facing, and the colour formats follow the encoded-space contract in
//! ADR-0063: an sRGB texture view must not decode the source and an sRGB render
//! target must not re-encode the composited frame, or the overlay's colours
//! would differ from the macOS backend's for the same model.

use super::*;

// The renderer follows the encoded-space compatibility contract in ADR-0063.
// Keep the same encoded RGB values on both native backends: do not let an
// sRGB texture view decode the source or an sRGB render target re-encode the
// composited frame. Alpha is still premultiplied for the compositor, and masks
// carry coverage only.
pub(crate) const COMPOSITION_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;

pub(crate) const COMPOSITION_RENDER_TARGET_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;

pub(crate) const MODEL_TEXTURE_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;

pub(crate) const MASK_TEXTURE_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;

pub(crate) const SHADER_SOURCE: &str = r#"
    cbuffer UniformBuffer : register(b0) {
        float4 scale_offset;
        float4 multiply_color;
        float4 screen_color;
        float4 mask_settings;
        float4 corner_radius;
        // Model/drawable opacity only. Window presentation opacity is applied
        // once to the completed DirectComposition surface.
        float opacity;
        float3 padding;
    };

    struct VertexInput {
        float2 position : POSITION;
        float2 uv : TEXCOORD;
    };

    struct RasterVertex {
        float4 position : SV_POSITION;
        float2 uv : TEXCOORD;
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

    RasterVertex cubism_vertex(VertexInput input) {
        RasterVertex output;
        float2 clip = input.position * scale_offset.xy + scale_offset.zw;
        output.position = float4(clip, 0.0, 1.0);
        output.uv = float2(input.uv.x, 1.0 - input.uv.y);
        return output;
    }

    Texture2D<float4> model_texture : register(t0);
    Texture2D<float4> mask_texture : register(t1);
    SamplerState texture_sampler : register(s0);

    // The source texture is an ordinary UNORM view on purpose. This is the
    // encoded-space compatibility blend: do not insert a linear/sRGB conversion
    // here without changing both backends and the product contract.
    float4 cubism_fragment(RasterVertex input) : SV_TARGET {
        float4 texture_color = model_texture.Sample(texture_sampler, input.uv);
        float3 color = texture_color.rgb * multiply_color.rgb;
        color = color + screen_color.rgb - color * screen_color.rgb;
        float mask = 1.0;
        if (mask_settings.z > 0.5) {
            float2 mask_uv = input.position.xy / mask_settings.xy;
            mask = mask_texture.Sample(texture_sampler, mask_uv).a;
            if (mask_settings.w > 0.5) {
                mask = 1.0 - mask;
            }
        }
        float alpha = texture_color.a * opacity * mask
                    * corner_coverage(input.position.xy, corner_radius);
        return float4(color * alpha, alpha);
    }

    float4 cubism_mask_fragment(RasterVertex input) : SV_TARGET {
        float alpha = model_texture.Sample(texture_sampler, input.uv).a;
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

pub(crate) struct Pipelines {
    pub(crate) vertex_shader: ID3D11VertexShader,
    pub(crate) fragment_shader: ID3D11PixelShader,
    pub(crate) mask_shader: ID3D11PixelShader,
    pub(crate) input_layout: ID3D11InputLayout,
    pub(crate) constant_buffer: ID3D11Buffer,
    pub(crate) sampler: ID3D11SamplerState,
    pub(crate) rasterizer: ID3D11RasterizerState,
    pub(crate) cull_back_rasterizer: ID3D11RasterizerState,
    pub(crate) cull_front_rasterizer: ID3D11RasterizerState,
    pub(crate) normal_blend: ID3D11BlendState,
    pub(crate) additive_blend: ID3D11BlendState,
    pub(crate) multiplicative_blend: ID3D11BlendState,
    pub(crate) mask_blend: ID3D11BlendState,
}

impl Pipelines {
    pub(crate) fn blend(&self, mode: BlendMode) -> &ID3D11BlendState {
        match mode {
            BlendMode::Normal => &self.normal_blend,
            BlendMode::Additive => &self.additive_blend,
            BlendMode::Multiplicative => &self.multiplicative_blend,
        }
    }
}

pub(crate) unsafe fn create_pipelines(device: &ID3D11Device) -> WindowsResult<Pipelines> {
    let vertex_blob = unsafe { compile_shader(s!("cubism_vertex"), s!("vs_5_0"))? };
    let fragment_blob = unsafe { compile_shader(s!("cubism_fragment"), s!("ps_5_0"))? };
    let mask_blob = unsafe { compile_shader(s!("cubism_mask_fragment"), s!("ps_5_0"))? };
    let vertex_bytes = unsafe { blob_bytes(&vertex_blob) };
    let fragment_bytes = unsafe { blob_bytes(&fragment_blob) };
    let mask_bytes = unsafe { blob_bytes(&mask_blob) };
    let mut vertex_shader = None;
    let mut fragment_shader = None;
    let mut mask_shader = None;
    unsafe {
        device.CreateVertexShader(
            vertex_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut vertex_shader),
        )?;
        device.CreatePixelShader(
            fragment_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut fragment_shader),
        )?;
        device.CreatePixelShader(
            mask_bytes,
            None::<&ID3D11ClassLinkage>,
            Some(&mut mask_shader),
        )?;
    }
    let elements = [
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
        D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXCOORD"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 8,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        },
    ];
    let mut input_layout = None;
    unsafe { device.CreateInputLayout(&elements, vertex_bytes, Some(&mut input_layout))? };
    let constant_desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<Uniforms>() as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        ..Default::default()
    };
    let mut constant_buffer = None;
    unsafe { device.CreateBuffer(&constant_desc, None, Some(&mut constant_buffer))? };
    let sampler_desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MaxLOD: f32::MAX,
        ..Default::default()
    };
    let mut sampler = None;
    unsafe { device.CreateSamplerState(&sampler_desc, Some(&mut sampler))? };
    let rasterizer = unsafe { create_rasterizer_state(device, D3D11_CULL_NONE)? };
    let cull_back_rasterizer = unsafe { create_rasterizer_state(device, D3D11_CULL_BACK)? };
    let cull_front_rasterizer = unsafe { create_rasterizer_state(device, D3D11_CULL_FRONT)? };
    Ok(Pipelines {
        vertex_shader: required(vertex_shader, "vertex shader")?,
        fragment_shader: required(fragment_shader, "fragment shader")?,
        mask_shader: required(mask_shader, "mask shader")?,
        input_layout: required(input_layout, "input layout")?,
        constant_buffer: required(constant_buffer, "constant buffer")?,
        sampler: required(sampler, "sampler")?,
        rasterizer,
        cull_back_rasterizer,
        cull_front_rasterizer,
        normal_blend: unsafe { create_blend_state(device, blend_factors(BlendMode::Normal))? },
        additive_blend: unsafe { create_blend_state(device, blend_factors(BlendMode::Additive))? },
        multiplicative_blend: unsafe {
            create_blend_state(device, blend_factors(BlendMode::Multiplicative))?
        },
        mask_blend: unsafe { create_blend_state(device, blend_factors(BlendMode::Normal))? },
    })
}

pub(crate) fn rasterizer_descriptor(cull_mode: D3D11_CULL_MODE) -> D3D11_RASTERIZER_DESC {
    D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: cull_mode,
        // Cubism's D3D11 renderer treats counter-clockwise triangles as the
        // front face. Keep that explicit now that single-sided drawables are
        // actually culled instead of being masked by CULL_NONE.
        FrontCounterClockwise: true.into(),
        ..Default::default()
    }
}

pub(crate) unsafe fn create_rasterizer_state(
    device: &ID3D11Device,
    cull_mode: D3D11_CULL_MODE,
) -> WindowsResult<ID3D11RasterizerState> {
    let descriptor = rasterizer_descriptor(cull_mode);
    let mut state = None;
    unsafe { device.CreateRasterizerState(&descriptor, Some(&mut state))? };
    required(state, "rasterizer state")
}

pub(crate) unsafe fn create_blend_state(
    device: &ID3D11Device,
    factors: crate::BlendFactors,
) -> WindowsResult<ID3D11BlendState> {
    let target = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: d3d_blend_factor(factors.source_rgb),
        DestBlend: d3d_blend_factor(factors.destination_rgb),
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: d3d_blend_factor(factors.source_alpha),
        DestBlendAlpha: d3d_blend_factor(factors.destination_alpha),
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let mut descriptor = D3D11_BLEND_DESC::default();
    descriptor.RenderTarget[0] = target;
    let mut state = None;
    unsafe { device.CreateBlendState(&descriptor, Some(&mut state))? };
    required(state, "blend state")
}

pub(crate) const fn d3d_blend_factor(
    factor: BlendFactor,
) -> windows::Win32::Graphics::Direct3D11::D3D11_BLEND {
    match factor {
        BlendFactor::Zero => D3D11_BLEND_ZERO,
        BlendFactor::One => D3D11_BLEND_ONE,
        BlendFactor::OneMinusSourceAlpha => D3D11_BLEND_INV_SRC_ALPHA,
        BlendFactor::DestinationColor => D3D11_BLEND_DEST_COLOR,
    }
}

pub(crate) unsafe fn compile_shader(entry: PCSTR, target: PCSTR) -> WindowsResult<ID3DBlob> {
    let mut code = None;
    let mut diagnostics = None;
    let result = unsafe {
        D3DCompile(
            SHADER_SOURCE.as_ptr().cast(),
            SHADER_SOURCE.len(),
            s!("BongoCatProductOverlay.hlsl"),
            None,
            None::<&ID3DInclude>,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut diagnostics),
        )
    };
    if let Err(error) = result {
        let message = diagnostics
            .as_ref()
            .map(|blob| unsafe { blob_message(blob) })
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| error.message());
        return Err(Error::new(error.code(), message));
    }
    required(code, "shader bytecode")
}

pub(crate) unsafe fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    let pointer = unsafe { blob.GetBufferPointer() }.cast::<u8>();
    let length = unsafe { blob.GetBufferSize() };
    // SAFETY: ID3DBlob owns a contiguous allocation for the returned length and
    // the slice is borrowed no longer than the live blob reference.
    unsafe { std::slice::from_raw_parts(pointer, length) }
}

pub(crate) unsafe fn blob_message(blob: &ID3DBlob) -> String {
    unsafe { String::from_utf8_lossy(blob_bytes(blob)) }
        .trim_end_matches(['\0', '\r', '\n'])
        .to_owned()
}
