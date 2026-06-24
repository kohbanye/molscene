// Sphere impostor: each instance draws a single camera-facing quad; the fragment
// shader ray-traces the exact sphere, writing per-pixel depth so impostors
// interpenetrate correctly with meshes and each other. Perfectly smooth at any
// zoom — no tessellation.

// Enlarge the billboard past the geometric radius so the perspective silhouette
// is never clipped at the quad edge.
const K: f32 = 1.4;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) center: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) radius: f32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) center: vec3<f32>,
    @location(1) radius: f32,
    @location(2) color: vec3<f32>,
) -> VsOut {
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let q = quad[vi];
    let world = center
        + cam.cam_right.xyz * (radius * K * q.x)
        + cam.cam_up.xyz * (radius * K * q.y);
    var out: VsOut;
    out.clip = cam.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.center = center;
    out.color = color;
    out.radius = radius;
    return out;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_main(in: VsOut) -> FsOut {
    let o = cam.cam_pos.xyz;
    let d = normalize(in.world - o);
    let oc = o - in.center;
    let b = dot(oc, d);
    let c = dot(oc, oc) - in.radius * in.radius;
    let disc = b * b - c;
    if (disc < 0.0) {
        discard;
    }
    let t = -b - sqrt(disc);
    if (t < 0.0) {
        discard;
    }
    let hit = o + d * t;
    let n = normalize(hit - in.center);
    let clip = cam.view_proj * vec4<f32>(hit, 1.0);
    var out: FsOut;
    out.depth = clip.z / clip.w;
    out.color = vec4<f32>(shade(n, in.color), 1.0);
    return out;
}
