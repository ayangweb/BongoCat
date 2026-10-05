struct Uniforms {
    scale_offset: vec4<f32>,
    multiply_color: vec4<f32>,
    screen_color: vec4<f32>,
    mask_settings: vec4<f32>,
    corner_radius: vec4<f32>,
    opacity_padding: vec4<f32>,
};

@group(0) @binding(0) var model_texture: texture_2d<f32>;
@group(0) @binding(1) var mask_texture: texture_2d<f32>;
@group(0) @binding(2) var texture_sampler: sampler;
@group(1) @binding(0) var<uniform> uniforms: Uniforms;

struct ModelVertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct RasterVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn corner_coverage(position: vec2<f32>, settings: vec4<f32>) -> f32 {
    let radius = min(settings.x, 0.5);
    if radius <= 0.0 {
        return 1.0;
    }
    let uv = position / settings.yz;
    let centered = abs(uv * 2.0 - 1.0);
    let extent = 2.0 * radius;
    let delta = centered - vec2<f32>(1.0) + vec2<f32>(extent);
    let distance = length(max(delta, vec2<f32>(0.0)))
        + min(max(delta.x, delta.y), 0.0) - extent;
    let scale = 0.5 * min(settings.y, settings.z);
    return clamp(0.5 - distance * scale, 0.0, 1.0);
}

@vertex
fn model_vertex(input: ModelVertexInput) -> RasterVertex {
    var output: RasterVertex;
    let clip = input.position * uniforms.scale_offset.xy + uniforms.scale_offset.zw;
    output.position = vec4<f32>(clip, 0.0, 1.0);
    output.uv = vec2<f32>(input.uv.x, 1.0 - input.uv.y);
    return output;
}

@fragment
fn model_fragment(input: RasterVertex) -> @location(0) vec4<f32> {
    let texture_color = textureSample(model_texture, texture_sampler, input.uv);
    var color = texture_color.rgb * uniforms.multiply_color.rgb;
    color = color + uniforms.screen_color.rgb - color * uniforms.screen_color.rgb;
    var mask = 1.0;
    if uniforms.mask_settings.z > 0.5 {
        let mask_uv = input.position.xy / uniforms.mask_settings.xy;
        mask = textureSample(mask_texture, texture_sampler, mask_uv).a;
        if uniforms.mask_settings.w > 0.5 {
            mask = 1.0 - mask;
        }
    }
    let alpha = texture_color.a * uniforms.opacity_padding.x * mask
        * corner_coverage(input.position.xy, uniforms.corner_radius);
    return vec4<f32>(color * alpha, alpha);
}

@fragment
fn mask_fragment(input: RasterVertex) -> @location(0) vec4<f32> {
    let alpha = textureSample(model_texture, texture_sampler, input.uv).a;
    return vec4<f32>(0.0, 0.0, 0.0, alpha);
}

struct FullscreenVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn fullscreen_vertex(@builtin(vertex_index) vertex_index: u32) -> FullscreenVertex {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: FullscreenVertex;
    output.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    output.uv = positions[vertex_index] * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return output;
}

@fragment
fn present_fragment(input: FullscreenVertex) -> @location(0) vec4<f32> {
    let color = textureSample(model_texture, texture_sampler, input.uv);
    return color * uniforms.opacity_padding.x;
}
