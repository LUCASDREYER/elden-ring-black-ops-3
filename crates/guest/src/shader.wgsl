// One textured, lit mesh in Elden Ring world space.

struct Globals {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = g.view_proj * g.model * vec4(v.pos, 1.0);
    o.normal = (g.model * vec4(v.normal, 0.0)).xyz;
    o.uv = v.uv;
    return o;
}

@fragment
fn fs(o: VOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, o.uv).rgb;
    let l = normalize(vec3(0.4, 1.0, -0.3));
    let k = 0.55 + 0.45 * max(dot(normalize(o.normal), l), 0.0);
    // Opaque, so premultiplied alpha is the colour itself.
    return vec4(c * k, 1.0);
}
