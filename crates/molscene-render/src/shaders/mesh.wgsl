// Triangle meshes (cartoon ribbons, molecular surface): drawn straight from the
// per-vertex positions / normals / colors molscene tessellates in Rust. Group
// opacity comes from a per-draw uniform; meshes are double-sided so a thin
// ribbon lit from either face stays shaded.

struct Material {
    opacity: vec4<f32>,
};
@group(1) @binding(0) var<uniform> mat: Material;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = cam.view_proj * vec4<f32>(position, 1.0);
    out.normal = normal;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    return vec4<f32>(shade(n, in.color), mat.opacity.x);
}
