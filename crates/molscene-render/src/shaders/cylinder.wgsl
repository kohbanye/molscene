// Cylinder impostor: each instance draws a camera-facing quad sized to the
// bond's bounding sphere; the fragment shader ray-traces a finite cylinder
// (lateral surface + flat end caps) and writes per-pixel depth.
//
// The intersection is written fully unrolled — no loops and no dynamically
// indexed arrays — because some software Vulkan SPIR-V optimizers (notably
// SwiftShader's) crash on the loop/indexing patterns a folded version emits.

const K: f32 = 1.1;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) start: vec3<f32>,
    @location(2) end: vec3<f32>,
    @location(3) color: vec3<f32>,
    @location(4) radius: f32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) start: vec3<f32>,
    @location(1) radius: f32,
    @location(2) end: vec3<f32>,
    @location(3) color: vec3<f32>,
) -> VsOut {
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let q = quad[vi];
    let center = (start + end) * 0.5;
    let bound = (length(end - start) * 0.5 + radius) * K;
    let world = center + cam.cam_right.xyz * (bound * q.x) + cam.cam_up.xyz * (bound * q.y);
    var out: VsOut;
    out.clip = cam.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.start = start;
    out.end = end;
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
    let pa = in.start;
    let axis = in.end - in.start;
    let len = length(axis);
    // Degenerate bond (coincident endpoints): no axis direction — skip it
    // instead of dividing by zero (which would make `va` NaN).
    if (len < 1e-6) {
        discard;
    }
    let va = axis / len;
    let r = in.radius;

    var best_t = 1e30;
    var best_n = vec3<f32>(0.0, 0.0, 0.0);

    // Lateral surface: project the ray and origin offset off the axis.
    let dp = o - pa;
    let d_par = dot(d, va);
    let dp_par = dot(dp, va);
    let A = d - va * d_par;
    let B = dp - va * dp_par;
    let a2 = dot(A, A);
    let b2 = dot(A, B);
    let c2 = dot(B, B) - r * r;
    if (a2 > 1e-12) {
        let disc = b2 * b2 - a2 * c2;
        if (disc >= 0.0) {
            let sq = sqrt(disc);
            let t0 = (-b2 - sq) / a2;
            let m0 = dp_par + t0 * d_par;
            if (t0 > 0.0 && m0 >= 0.0 && m0 <= len) {
                best_t = t0;
                best_n = normalize((o + d * t0) - (pa + va * m0));
            }
            let t1 = (-b2 + sq) / a2;
            let m1 = dp_par + t1 * d_par;
            if (best_t > 1e29 && t1 > 0.0 && m1 >= 0.0 && m1 <= len) {
                best_t = t1;
                best_n = normalize((o + d * t1) - (pa + va * m1));
            }
        }
    }

    // Flat cap at the start (plane through pa, normal -va).
    let n0 = -va;
    let denom0 = dot(d, n0);
    if (abs(denom0) > 1e-6) {
        let t = dot(pa - o, n0) / denom0;
        if (t > 0.0 && t < best_t && length((o + d * t) - pa) <= r) {
            best_t = t;
            best_n = n0;
        }
    }

    // Flat cap at the end (plane through pb, normal +va).
    let pb = pa + va * len;
    let denom1 = dot(d, va);
    if (abs(denom1) > 1e-6) {
        let t = dot(pb - o, va) / denom1;
        if (t > 0.0 && t < best_t && length((o + d * t) - pb) <= r) {
            best_t = t;
            best_n = va;
        }
    }

    if (best_t > 1e29) {
        discard;
    }
    let hit = o + d * best_t;
    let clip = cam.view_proj * vec4<f32>(hit, 1.0);
    var out: FsOut;
    out.depth = clip.z / clip.w;
    out.color = vec4<f32>(shade(best_n, in.color), 1.0);
    return out;
}
