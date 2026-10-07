// The first-person weapon and arms: GPU skinning in eye space, drawn into the hand layer.

struct HandGlobals {
    // eye space (inches) -> clip, for this frame's hand projection
    view_proj: mat4x4<f32>,
    // eye space -> Elden Ring camera space, for lighting
    to_view: mat4x4<f32>,
};

struct MaterialParams {
    tint: vec4<f32>,
    // x: 1 = alpha-tested decal
    flags: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: HandGlobals;
@group(1) @binding(0) var<storage, read> bones: array<mat4x4<f32>>;
@group(2) @binding(0) var<uniform> mat: MaterialParams;
@group(2) @binding(1) var tex: texture_2d<f32>;
@group(2) @binding(2) var samp: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    let skin = bones[v.joints.x] * v.weights.x + bones[v.joints.y] * v.weights.y
             + bones[v.joints.z] * v.weights.z + bones[v.joints.w] * v.weights.w;
    var o: VOut;
    o.clip = g.view_proj * (skin * vec4(v.pos, 1.0));
    o.normal = (g.to_view * (skin * vec4(v.normal, 0.0))).xyz;
    o.uv = v.uv;
    return o;
}

@fragment
fn fs(o: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, o.uv);
    if (mat.flags.x > 0.5 && t.a < 0.5) {
        discard;
    }
    let c = t.rgb * mat.tint.rgb;
    // A key light from above and slightly in front, plus fill. Elden Ring relights the
    // hand layer with its own local light on top of this.
    var n = normalize(o.normal);
    let key = max(dot(n, normalize(vec3(-0.3, 0.8, -0.5))), 0.0);
    let back = max(dot(n, normalize(vec3(0.5, 0.2, 0.8))), 0.0);
    let k = 0.42 + 0.55 * key + 0.15 * back;
    // Opaque, so premultiplied alpha is the colour itself.
    return vec4(c * k, 1.0);
}
