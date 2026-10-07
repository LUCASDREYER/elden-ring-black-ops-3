// Muzzle flash: a camera-facing star at the muzzle, drawn over the weapon in the hand layer.
// Procedural, no texture: a hot core, a soft glow and a few spikes.

struct Flash {
    // hand projection (camera space -> clip)
    proj: mat4x4<f32>,
    // xyz: muzzle in camera space (metres), w: half size
    center: vec4<f32>,
    // x: intensity 0..1, y: rotation (radians)
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> f: Flash;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    let c = corners[i];
    let p = f.center.xyz + vec3(c * f.center.w, 0.0);
    var o: VOut;
    o.clip = f.proj * vec4(p, 1.0);
    o.uv = c;
    return o;
}

@fragment
fn fs(o: VOut) -> @location(0) vec4<f32> {
    let r = length(o.uv);
    if (r >= 1.0) {
        discard;
    }
    let a = atan2(o.uv.y, o.uv.x) + f.params.y;
    let core = pow(max(1.0 - r * 2.2, 0.0), 1.5);
    let glow = pow(1.0 - r, 3.0) * 0.6;
    let spikes = pow(abs(cos(a * 2.5)), 24.0) * pow(1.0 - r, 1.5) * 0.9;
    let k = clamp((core + glow + spikes) * f.params.x, 0.0, 1.0);
    let col = mix(vec3(1.0, 0.55, 0.18), vec3(1.0, 0.95, 0.8), core);
    // Premultiplied: bright where it's opaque, fading to nothing at the edge.
    return vec4(col * k, k * 0.85);
}
