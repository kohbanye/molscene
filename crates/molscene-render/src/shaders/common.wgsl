// Shared camera uniform + shading, prepended to each pipeline's WGSL at build
// time (WGSL has no `#include`). The shading rig: a hemisphere ambient (sky
// white / ground 0x444444, intensity 0.6) plus a key (1,1,1)·0.8 and fill
// (-1,0.5,-0.5)·0.3 directional pair, in world space.

struct Camera {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
};
@group(0) @binding(0) var<uniform> cam: Camera;

// Authored RGB (CPK / named colors) are display-sRGB; convert to linear for
// lighting. The render target is sRGB, so the GPU re-encodes on store.
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn shade(n_in: vec3<f32>, albedo_srgb: vec3<f32>) -> vec3<f32> {
    let n = normalize(n_in);
    let albedo = to_linear(albedo_srgb);
    let key_dir = normalize(vec3<f32>(1.0, 1.0, 1.0));
    let fill_dir = normalize(vec3<f32>(-1.0, 0.5, -0.5));
    let ground = to_linear(vec3<f32>(0.2667, 0.2667, 0.2667));
    let hemi = mix(ground, vec3<f32>(1.0, 1.0, 1.0), 0.5 * n.y + 0.5) * 0.6;
    let key = max(dot(n, key_dir), 0.0) * 0.8;
    let fill = max(dot(n, fill_dir), 0.0) * 0.3;
    return albedo * (hemi + vec3<f32>(key + fill, key + fill, key + fill));
}
