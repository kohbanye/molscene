//! Platform-agnostic GPU rendering core, shared by the native PNG path
//! (`crate::render_png`) and the browser bindings (`molscene-wasm`: canvas
//! display + PNG download). It builds the impostor/mesh pipelines for a given
//! color-target format, uploads a [`GeometrySpec`] into GPU buffers, and records
//! the draw calls into a render pass. It does **no** device/surface setup, no
//! readback, and no blocking — each binding owns those (they differ between
//! native and the browser).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use molscene_core::geometry::GeometrySpec;

use crate::math::{self, cross, length as vlen, scale, sub, Vec3};

/// Depth attachment format used by every pipeline here.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Interactive camera adjustment applied on top of the spec's framing: orbit
/// (`yaw`/`pitch`, radians) around the framed center and a `zoom` multiplier
/// (`> 1` moves the camera closer). The identity (`Default`) reproduces the
/// spec's framing exactly, so the native still image and an un-dragged browser
/// view match.
#[derive(Debug, Clone, Copy)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            zoom: 1.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct CameraUniform {
    view_proj: [f32; 16],
    cam_pos: [f32; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SphereInstance {
    center: [f32; 3],
    radius: f32,
    color: [f32; 3],
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CylinderInstance {
    // Field offsets must match the packed offsets `vertex_attr_array` computes
    // for the cylinder pipeline (start@0, radius@12, end@16, color@28): no
    // padding between `end` and `color`, or the color reads the wrong bytes.
    start: [f32; 3],
    radius: f32,
    end: [f32; 3],
    color: [f32; 3],
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    opacity: [f32; 4],
}

/// Rotate `v` around the unit axis `k` by `angle` radians (Rodrigues).
fn rotate(v: Vec3, k: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    let kv = cross(k, v);
    let kd = math::dot(k, v) * (1.0 - c);
    [
        v[0] * c + kv[0] * s + k[0] * kd,
        v[1] * c + kv[1] * s + k[1] * kd,
        v[2] * c + kv[2] * s + k[2] * kd,
    ]
}

/// Distance from the box center at which the oriented box just fills the
/// frustum, aspect-aware (the per-axis oriented-box fit every frontend shares).
fn fit_distance(extent: [f32; 3], aspect: f32, fov_deg: f32) -> f32 {
    // A NaN aspect/FOV makes every comparison false, so `valid` is false and we
    // fall through to the safe fallback instead of producing an Infinity/NaN
    // distance.
    let valid = aspect > 0.0 && fov_deg > 0.0 && fov_deg < 180.0;
    if !valid {
        return extent[2] + extent[0].max(extent[1]).max(1.0);
    }
    let tan_v = (fov_deg * std::f32::consts::PI / 360.0).tan();
    let tan_h = tan_v * aspect;
    let dist_v = extent[1] / tan_v + extent[2];
    let dist_h = extent[0] / tan_h + extent[2];
    dist_v.max(dist_h)
}

/// Build the camera uniform from the spec's oriented-box framing plus an
/// interactive `orbit` (45° vertical FOV, fit per axis, eye along `right × up`).
fn build_camera(
    spec: &GeometrySpec,
    width: u32,
    height: u32,
    orbit: Orbit,
) -> (CameraUniform, Vec3) {
    let cam = &spec.camera;
    let aspect = width as f32 / height.max(1) as f32;
    let fov_deg = 45.0;
    let distance = fit_distance(cam.extent, aspect, fov_deg) / orbit.zoom.max(0.01);

    // Orbit the screen basis around the framed center: yaw about up, then pitch
    // about the (yawed) right. Both are rotations, so the basis stays orthonormal.
    let mut right = cam.right;
    let mut up = cam.up;
    let mut forward = cross(right, up); // box → camera
    right = rotate(right, up, orbit.yaw);
    forward = rotate(forward, up, orbit.yaw);
    up = rotate(up, right, orbit.pitch);
    forward = rotate(forward, right, orbit.pitch);

    let eye = math::add(cam.center, scale(forward, distance));
    let diag = vlen(cam.extent);
    let near = (distance - diag).max(0.05);
    let far = distance + diag + 1.0;

    let view = math::look_at_rh(eye, cam.center, up);
    let proj = math::perspective_rh_zo(fov_deg * std::f32::consts::PI / 180.0, aspect, near, far);
    let view_proj = math::mul(&proj, &view);

    (
        CameraUniform {
            view_proj,
            cam_pos: [eye[0], eye[1], eye[2], 0.0],
            cam_right: [right[0], right[1], right[2], 0.0],
            cam_up: [up[0], up[1], up[2], 0.0],
        },
        eye,
    )
}

/// Convert an authored sRGB component (0..1) into the linear value to clear an
/// sRGB attachment with (clear values are written without sRGB encoding).
fn srgb_to_linear(c: f32) -> f64 {
    (c.max(0.0).powf(2.2)) as f64
}

/// The render pipelines + bind-group layouts for one color-target format. Build
/// once (per device + surface format) and reuse across frames.
pub struct Pipelines {
    cam_layout: wgpu::BindGroupLayout,
    mat_layout: wgpu::BindGroupLayout,
    sphere_pipe: wgpu::RenderPipeline,
    cylinder_pipe: wgpu::RenderPipeline,
    mesh_opaque_pipe: wgpu::RenderPipeline,
    mesh_transparent_pipe: wgpu::RenderPipeline,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let cam_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let mat_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let common = include_str!("shaders/common.wgsl");
        let make_module = |name: &str, body: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(format!("{common}\n{body}").into()),
            })
        };
        let sphere_mod = make_module("sphere", include_str!("shaders/sphere.wgsl"));
        let cylinder_mod = make_module("cylinder", include_str!("shaders/cylinder.wgsl"));
        let mesh_mod = make_module("mesh", include_str!("shaders/mesh.wgsl"));

        let impostor_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("impostor-pl"),
            bind_group_layouts: &[&cam_layout],
            push_constant_ranges: &[],
        });
        let mesh_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh-pl"),
            bind_group_layouts: &[&cam_layout, &mat_layout],
            push_constant_ranges: &[],
        });

        let depth_state = |write: bool| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: write,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let opaque_target = wgpu::ColorTargetState {
            format: color_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        };
        let blend_target = wgpu::ColorTargetState {
            format: color_format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let sphere_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x3];
        let sphere_vb = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SphereInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &sphere_attrs,
        };
        let cyl_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x3, 3 => Float32x3];
        let cyl_vb = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CylinderInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &cyl_attrs,
        };
        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
        let mesh_vb = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attrs,
        };

        let sphere_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sphere-pipe"),
            layout: Some(&impostor_layout),
            vertex: wgpu::VertexState {
                module: &sphere_mod,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&sphere_vb),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sphere_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(opaque_target.clone())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(true)),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let cylinder_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cylinder-pipe"),
            layout: Some(&impostor_layout),
            vertex: wgpu::VertexState {
                module: &cylinder_mod,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&cyl_vb),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &cylinder_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(opaque_target.clone())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_state(true)),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let mesh_primitive = wgpu::PrimitiveState {
            cull_mode: None, // double-sided
            ..Default::default()
        };
        let mesh_opaque_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh-opaque-pipe"),
            layout: Some(&mesh_pl_layout),
            vertex: wgpu::VertexState {
                module: &mesh_mod,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&mesh_vb),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_mod,
                entry_point: Some("fs_main"),
                targets: &[Some(opaque_target.clone())],
                compilation_options: Default::default(),
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(depth_state(true)),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let mesh_transparent_pipe =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh-transparent-pipe"),
                layout: Some(&mesh_pl_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_mod,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&mesh_vb),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_mod,
                    entry_point: Some("fs_main"),
                    targets: &[Some(blend_target)],
                    compilation_options: Default::default(),
                }),
                primitive: mesh_primitive,
                depth_stencil: Some(depth_state(false)), // don't occlude behind translucent
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        Self {
            cam_layout,
            mat_layout,
            sphere_pipe,
            cylinder_pipe,
            mesh_opaque_pipe,
            mesh_transparent_pipe,
        }
    }
}

struct MeshDraw {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    nindices: u32,
    bind: wgpu::BindGroup,
    depth_key: f32,
}

/// Per-frame GPU resources built from a [`GeometrySpec`]: the camera bind group,
/// instance buffers, and per-group mesh buffers. Rebuild when the scene, size,
/// or camera changes.
pub struct Frame {
    cam_bind: wgpu::BindGroup,
    spheres: Option<(wgpu::Buffer, u32)>,
    cylinders: Option<(wgpu::Buffer, u32)>,
    opaque_meshes: Vec<MeshDraw>,
    transparent_meshes: Vec<MeshDraw>,
    /// Clear color (linearized for an sRGB target).
    pub clear: wgpu::Color,
}

impl Frame {
    pub fn build(
        device: &wgpu::Device,
        pipelines: &Pipelines,
        spec: &GeometrySpec,
        width: u32,
        height: u32,
        orbit: Orbit,
    ) -> Self {
        let (camera, eye) = build_camera(spec, width, height, orbit);
        let camera_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera"),
            contents: bytemuck::bytes_of(&camera),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let cam_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera-bind"),
            layout: &pipelines.cam_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        let spheres: Vec<SphereInstance> = spec
            .spheres
            .centers
            .iter()
            .enumerate()
            .map(|(i, &c)| SphereInstance {
                center: c,
                radius: spec.spheres.radii[i],
                color: spec.spheres.colors[i],
                _pad: 0.0,
            })
            .collect();
        let cylinders: Vec<CylinderInstance> = spec
            .cylinders
            .starts
            .iter()
            .enumerate()
            .map(|(i, &s)| CylinderInstance {
                start: s,
                radius: spec.cylinders.radii[i],
                end: spec.cylinders.ends[i],
                color: spec.cylinders.colors[i],
                _pad: 0.0,
            })
            .collect();

        let sphere_buf = (!spheres.is_empty()).then(|| {
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("spheres"),
                    contents: bytemuck::cast_slice(&spheres),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                spheres.len() as u32,
            )
        });
        let cylinder_buf = (!cylinders.is_empty()).then(|| {
            (
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cylinders"),
                    contents: bytemuck::cast_slice(&cylinders),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                cylinders.len() as u32,
            )
        });

        let mut opaque_meshes: Vec<MeshDraw> = Vec::new();
        let mut transparent_meshes: Vec<MeshDraw> = Vec::new();
        for mesh in &spec.meshes {
            if mesh.positions.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            let verts: Vec<MeshVertex> = (0..mesh.positions.len())
                .map(|i| MeshVertex {
                    position: mesh.positions[i],
                    normal: *mesh.normals.get(i).unwrap_or(&[0.0, 0.0, 1.0]),
                    color: *mesh.colors.get(i).unwrap_or(&[0.5, 0.5, 0.5]),
                })
                .collect();
            let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh-v"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh-i"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let mat = MaterialUniform {
                opacity: [mesh.opacity, 0.0, 0.0, 0.0],
            };
            let mbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh-mat"),
                contents: bytemuck::bytes_of(&mat),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh-mat-bind"),
                layout: &pipelines.mat_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: mbuf.as_entire_binding(),
                }],
            });
            let mut c = [0.0f32; 3];
            for p in &mesh.positions {
                c = math::add(c, *p);
            }
            let centroid = scale(c, 1.0 / mesh.positions.len() as f32);
            let draw = MeshDraw {
                vbuf,
                ibuf,
                nindices: mesh.indices.len() as u32,
                bind,
                depth_key: vlen(sub(centroid, eye)),
            };
            if mesh.opacity < 1.0 {
                transparent_meshes.push(draw);
            } else {
                opaque_meshes.push(draw);
            }
        }
        // Farthest first (painter's order) for the translucent groups.
        transparent_meshes.sort_by(|a, b| b.depth_key.total_cmp(&a.depth_key));

        let bg = spec.background;
        let clear = wgpu::Color {
            r: srgb_to_linear(bg[0]),
            g: srgb_to_linear(bg[1]),
            b: srgb_to_linear(bg[2]),
            a: 1.0,
        };

        Self {
            cam_bind,
            spheres: sphere_buf,
            cylinders: cylinder_buf,
            opaque_meshes,
            transparent_meshes,
            clear,
        }
    }
}

/// Record every draw for `frame` into an already-begun render pass: opaque
/// impostors and meshes first (depth-resolved), then translucent meshes
/// back-to-front. The caller owns the pass (its clear/load ops and the target),
/// so the same recording drives both an offscreen texture and a surface.
pub fn record(pass: &mut wgpu::RenderPass<'_>, pipelines: &Pipelines, frame: &Frame) {
    pass.set_bind_group(0, &frame.cam_bind, &[]);

    if let Some((buf, n)) = &frame.spheres {
        pass.set_pipeline(&pipelines.sphere_pipe);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..6, 0..*n);
    }
    if let Some((buf, n)) = &frame.cylinders {
        pass.set_pipeline(&pipelines.cylinder_pipe);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..6, 0..*n);
    }
    if !frame.opaque_meshes.is_empty() {
        pass.set_pipeline(&pipelines.mesh_opaque_pipe);
        for m in &frame.opaque_meshes {
            pass.set_bind_group(1, &m.bind, &[]);
            pass.set_vertex_buffer(0, m.vbuf.slice(..));
            pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..m.nindices, 0, 0..1);
        }
    }
    if !frame.transparent_meshes.is_empty() {
        pass.set_pipeline(&pipelines.mesh_transparent_pipe);
        for m in &frame.transparent_meshes {
            pass.set_bind_group(1, &m.bind, &[]);
            pass.set_vertex_buffer(0, m.vbuf.slice(..));
            pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..m.nindices, 0, 0..1);
        }
    }
}

/// Create the depth texture view for a render of size `width × height`.
pub fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Box-downsample an RGBA8 image by an integer factor.
pub fn downsample(src: &[u8], w: u32, h: u32, factor: u32) -> Vec<u8> {
    if factor <= 1 {
        return src.to_vec();
    }
    let ow = w / factor;
    let oh = h / factor;
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    let n = factor * factor;
    for oy in 0..oh {
        for ox in 0..ow {
            let mut acc = [0u32; 4];
            for dy in 0..factor {
                for dx in 0..factor {
                    let sx = ox * factor + dx;
                    let sy = oy * factor + dy;
                    let si = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += src[si + c] as u32;
                    }
                }
            }
            let di = ((oy * ow + ox) * 4) as usize;
            for c in 0..4 {
                out[di + c] = (acc[c] / n) as u8;
            }
        }
    }
    out
}

/// Encode tightly-packed RGBA8 pixels as PNG bytes.
pub fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}
