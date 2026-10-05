struct Params { transform: vec4f, multiply: vec4f, screen: vec4f, mask: vec4f, presentation: vec4f };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var mask_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
struct Out { @builtin(position) position: vec4f, @location(0) uv: vec2f };
@vertex fn vertex(@location(0) pos: vec2f, @location(1) uv: vec2f) -> Out {
    var out: Out;
    out.position = vec4f(pos * p.transform.xy + p.transform.zw, 0.0, 1.0);
    out.uv = vec2f(uv.x, 1.0 - uv.y);
    return out;
}
@fragment fn fragment(v: Out) -> @location(0) vec4f {
    let t = textureSample(tex, samp, v.uv);
    let multiplied = t.rgb * p.multiply.rgb;
    let rgb = multiplied + p.screen.rgb - multiplied * p.screen.rgb;
    var mask = 1.0;
    if p.mask.z > 0.5 {
        mask = textureSample(mask_tex, samp, v.position.xy / p.mask.xy).a;
        if p.mask.w > 0.5 { mask = 1.0 - mask; }
    }
    let a = t.a * p.presentation.x * mask;
    return vec4f(rgb * a, a);
}
@fragment fn mask_fragment(v: Out) -> @location(0) vec4f {
    return vec4f(0.0, 0.0, 0.0, textureSample(tex, samp, v.uv).a);
}
@fragment fn present_fragment(v: Out) -> @location(0) vec4f {
    var color = textureSample(tex, samp, v.uv) * p.presentation.x;
    let radius = p.presentation.y;
    if radius > 0.0 {
        let d = abs(v.uv * 2.0 - 1.0) - vec2f(1.0 - 2.0 * radius);
        let distance = length(max(d, vec2f(0.0))) + min(max(d.x, d.y), 0.0) - 2.0 * radius;
        color *= clamp(0.5 - distance * 0.5 * min(p.mask.x, p.mask.y), 0.0, 1.0);
    }
    return color;
}
