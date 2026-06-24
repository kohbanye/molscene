//! Minimal column-major 4×4 matrix math for building the camera transform.
//! Hand-rolled (no `glam`) to keep the dependency surface small; only the few
//! operations the renderer needs are implemented.

pub type Vec3 = [f32; 3];

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}
pub fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l > 1e-9 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// Column-major 4×4 matrix (element `[col * 4 + row]`), matching WGSL's
/// `mat4x4<f32>` memory layout so it can be uploaded as a uniform directly.
pub type Mat4 = [f32; 16];

/// `a * b` (column-major).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = s;
        }
    }
    out
}

/// Right-handed perspective with a `[0, 1]` depth range (the WebGPU/wgpu
/// convention — near maps to 0, far to 1), looking down −z in view space.
pub fn perspective_rh_zo(fovy_rad: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy_rad * 0.5).tan();
    let nf = 1.0 / (near - far);
    let mut m = [0.0f32; 16];
    m[0] = f / aspect; // col 0, row 0
    m[5] = f; // col 1, row 1
    m[10] = far * nf; // col 2, row 2
    m[11] = -1.0; // col 2, row 3
    m[14] = far * near * nf; // col 3, row 2
    m
}

/// Right-handed look-at view matrix.
pub fn look_at_rh(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
    let zaxis = normalize(sub(eye, center)); // camera looks down −z
    let xaxis = normalize(cross(up, zaxis));
    let yaxis = cross(zaxis, xaxis);
    [
        xaxis[0],
        yaxis[0],
        zaxis[0],
        0.0,
        xaxis[1],
        yaxis[1],
        zaxis[1],
        0.0,
        xaxis[2],
        yaxis[2],
        zaxis[2],
        0.0,
        -dot(xaxis, eye),
        -dot(yaxis, eye),
        -dot(zaxis, eye),
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_identity() {
        let id: Mat4 = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let m = perspective_rh_zo(1.0, 1.5, 0.1, 100.0);
        assert_eq!(mul(&id, &m), m);
    }

    #[test]
    fn perspective_maps_near_and_far_to_webgpu_depth_range() {
        // The depth contract the impostor shaders rely on: a right-handed view
        // (looking down −z) maps near → 0 and far → 1 after the perspective
        // divide. A sign/layout regression in `perspective_rh_zo` breaks depth.
        let (near, far) = (0.1f32, 100.0f32);
        let p = perspective_rh_zo(1.0, 1.5, near, far);
        let project_z = |z: f32| {
            let clip_z = p[10] * z + p[14]; // col2.row2 * z + col3.row2
            let clip_w = p[11] * z; // col2.row3 * z  (w = −z)
            clip_z / clip_w
        };
        assert!(project_z(-near).abs() < 1e-5, "near should map to 0");
        assert!((project_z(-far) - 1.0).abs() < 1e-5, "far should map to 1");
    }

    #[test]
    fn look_at_puts_eye_at_origin_of_view() {
        // The eye maps to the view-space origin.
        let v = look_at_rh([0.0, 0.0, 5.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        // Transform eye (column-major mat * vec).
        let e = [0.0, 0.0, 5.0, 1.0];
        let mut out = [0.0f32; 4];
        for row in 0..4 {
            for k in 0..4 {
                out[row] += v[k * 4 + row] * e[k];
            }
        }
        assert!(out[0].abs() < 1e-5 && out[1].abs() < 1e-5 && out[2].abs() < 1e-5);
    }
}
