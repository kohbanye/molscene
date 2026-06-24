//! GPU rasterizer: render a molscene [`GeometrySpec`] to a PNG image, headlessly.
//!
//! This is a second renderer for the same renderer-neutral contract the Three.js
//! viewer consumes — it knows nothing about molecules, only the `GeometrySpec`
//! draw list (instanced spheres + cylinders, triangle meshes, an oriented-box
//! camera, a background color). Spheres and cylinders are drawn as GPU
//! *impostors* (the fragment shader ray-traces the exact primitive and writes
//! per-pixel depth), so they stay perfectly smooth at any zoom; cartoon/surface
//! meshes are drawn directly. The frame is rendered headlessly with `wgpu`
//! (no window/surface), supersampled for antialiasing, and encoded to PNG.
//!
//! The camera framing and lighting mirror the Three.js viewer so the two
//! renderers produce visually consistent images.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use molscene_core::geometry::GeometrySpec;

mod math;
use math::{cross, length as vlen, scale, sub};

/// Options for a render.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Output image width in pixels.
    pub width: u32,
    /// Output image height in pixels.
    pub height: u32,
    /// Supersampling factor for antialiasing: the scene is rendered at
    /// `width * ssaa × height * ssaa` and box-downsampled to the output size.
    /// `1` disables it; `2` (the default) is a good quality/speed tradeoff.
    pub ssaa: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            ssaa: 2,
        }
    }
}

/// Why a render could not be produced.
#[derive(Debug)]
pub enum RenderError {
    /// No GPU adapter was available (no Vulkan/Metal/DX12/GL device, and no
    /// software fallback like SwiftShader/llvmpipe). Headless GPU rendering
    /// needs a working graphics driver in the environment.
    NoAdapter,
    /// The adapter could not produce a device.
    NoDevice(String),
    /// Reading the rendered pixels back from the GPU failed.
    Readback(String),
    /// PNG encoding failed.
    Encode(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoAdapter => write!(
                f,
                "no GPU adapter available for headless rendering (need a Vulkan/Metal/DX12/GL \
                 driver, or a software fallback such as SwiftShader/llvmpipe)"
            ),
            RenderError::NoDevice(e) => write!(f, "could not create GPU device: {e}"),
            RenderError::Readback(e) => write!(f, "GPU readback failed: {e}"),
            RenderError::Encode(e) => write!(f, "PNG encoding failed: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
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

/// Distance from the box center at which the oriented box just fills the
/// frustum, aspect-aware. Mirrors `fitDistance` in the Three.js viewer.
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

/// Build the camera uniform from the spec's oriented-box framing, matching the
/// Three.js viewer (45° vertical FOV, fit per axis, eye along `right × up`).
fn build_camera(spec: &GeometrySpec, width: u32, height: u32) -> CameraUniform {
    let cam = &spec.camera;
    let aspect = width as f32 / height.max(1) as f32;
    let fov_deg = 45.0;
    let distance = fit_distance(cam.extent, aspect, fov_deg);
    let forward = cross(cam.right, cam.up); // box → camera
    let eye = math::add(cam.center, scale(forward, distance));
    let diag = vlen(cam.extent);
    let near = (distance - diag).max(0.05);
    let far = distance + diag + 1.0;

    let view = math::look_at_rh(eye, cam.center, cam.up);
    let proj = math::perspective_rh_zo(fov_deg * std::f32::consts::PI / 180.0, aspect, near, far);
    let view_proj = math::mul(&proj, &view);

    CameraUniform {
        view_proj,
        cam_pos: [eye[0], eye[1], eye[2], 0.0],
        cam_right: [cam.right[0], cam.right[1], cam.right[2], 0.0],
        cam_up: [cam.up[0], cam.up[1], cam.up[2], 0.0],
    }
}

/// Convert an authored sRGB component (0..1) into the linear value to clear an
/// sRGB attachment with (clear values are written without sRGB encoding).
fn srgb_to_linear(c: f32) -> f64 {
    (c.max(0.0).powf(2.2)) as f64
}

/// Render `spec` to PNG bytes. Blocks on the GPU; safe to call off the main
/// thread.
pub fn render_png(spec: &GeometrySpec, opts: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    pollster::block_on(render_png_async(spec, opts))
}

async fn render_png_async(
    spec: &GeometrySpec,
    opts: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    let ssaa = opts.ssaa.max(1);
    let w = opts.width.max(1) * ssaa;
    let h = opts.height.max(1) * ssaa;

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        })
        .await
        .ok_or(RenderError::NoAdapter)?;

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("molscene-render"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        )
        .await
        .map_err(|e| RenderError::NoDevice(e.to_string()))?;

    // -- camera uniform + bind group (group 0) ------------------------------
    let camera = build_camera(spec, opts.width, opts.height);
    let camera_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("camera"),
        contents: bytemuck::bytes_of(&camera),
        usage: wgpu::BufferUsages::UNIFORM,
    });
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
    let cam_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("camera-bind"),
        layout: &cam_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera_buf.as_entire_binding(),
        }],
    });

    // Material layout (group 1) for mesh opacity.
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

    // -- shader modules -----------------------------------------------------
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

    // -- pipelines ----------------------------------------------------------
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
        format: COLOR_FORMAT,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    };
    let blend_target = wgpu::ColorTargetState {
        format: COLOR_FORMAT,
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
            buffers: &[sphere_vb],
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
            buffers: &[cyl_vb],
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
    let mesh_transparent_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mesh-transparent-pipe"),
        layout: Some(&mesh_pl_layout),
        vertex: wgpu::VertexState {
            module: &mesh_mod,
            entry_point: Some("vs_main"),
            buffers: &[mesh_vb],
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

    // -- instance / mesh buffers --------------------------------------------
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
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("spheres"),
            contents: bytemuck::cast_slice(&spheres),
            usage: wgpu::BufferUsages::VERTEX,
        })
    });
    let cylinder_buf = (!cylinders.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cylinders"),
            contents: bytemuck::cast_slice(&cylinders),
            usage: wgpu::BufferUsages::VERTEX,
        })
    });

    // Per-mesh-group GPU buffers + opacity bind group. Split into opaque and
    // transparent; transparent groups are sorted back-to-front by their
    // centroid distance to the eye.
    struct MeshDraw {
        vbuf: wgpu::Buffer,
        ibuf: wgpu::Buffer,
        nindices: u32,
        bind: wgpu::BindGroup,
        depth_key: f32,
    }
    let eye = [camera.cam_pos[0], camera.cam_pos[1], camera.cam_pos[2]];
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
            layout: &mat_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: mbuf.as_entire_binding(),
            }],
        });
        // Centroid for back-to-front sorting of transparent groups.
        let mut c = [0.0f32; 3];
        for p in &mesh.positions {
            c = math::add(c, *p);
        }
        let centroid = scale(c, 1.0 / mesh.positions.len() as f32);
        let depth_key = vlen(sub(centroid, eye));
        let draw = MeshDraw {
            vbuf,
            ibuf,
            nindices: mesh.indices.len() as u32,
            bind,
            depth_key,
        };
        if mesh.opacity < 1.0 {
            transparent_meshes.push(draw);
        } else {
            opaque_meshes.push(draw);
        }
    }
    // Farthest first (painter's order).
    transparent_meshes.sort_by(|a, b| b.depth_key.total_cmp(&a.depth_key));

    // -- render targets -----------------------------------------------------
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let color_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("color"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let bg = spec.background;
    let clear = wgpu::Color {
        r: srgb_to_linear(bg[0]),
        g: srgb_to_linear(bg[1]),
        b: srgb_to_linear(bg[2]),
        a: 1.0,
    };

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("main-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &cam_bind, &[]);

        // Opaque first (order among them is depth-resolved).
        if let Some(buf) = &sphere_buf {
            pass.set_pipeline(&sphere_pipe);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..6, 0..spheres.len() as u32);
        }
        if let Some(buf) = &cylinder_buf {
            pass.set_pipeline(&cylinder_pipe);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..6, 0..cylinders.len() as u32);
        }
        if !opaque_meshes.is_empty() {
            pass.set_pipeline(&mesh_opaque_pipe);
            for m in &opaque_meshes {
                pass.set_bind_group(1, &m.bind, &[]);
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.nindices, 0, 0..1);
            }
        }
        // Transparent meshes, back to front, no depth write.
        if !transparent_meshes.is_empty() {
            pass.set_pipeline(&mesh_transparent_pipe);
            for m in &transparent_meshes {
                pass.set_bind_group(1, &m.bind, &[]);
                pass.set_vertex_buffer(0, m.vbuf.slice(..));
                pass.set_index_buffer(m.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..m.nindices, 0, 0..1);
            }
        }
    }

    // -- copy color texture to a readable buffer ----------------------------
    let bytes_per_pixel = 4u32;
    let unpadded = w * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &color_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &out_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        size,
    );
    queue.submit(Some(encoder.finish()));

    // -- map + read ---------------------------------------------------------
    let slice = out_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|e| RenderError::Readback(e.to_string()))?
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    let data = slice.get_mapped_range();

    // Drop the padding to a tight RGBA buffer at the supersampled size.
    let mut pixels = vec![0u8; (unpadded * h) as usize];
    for row in 0..h as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        pixels[dst..dst + unpadded as usize].copy_from_slice(&data[src..src + unpadded as usize]);
    }
    drop(data);
    out_buf.unmap();

    // Box-downsample the supersampled image to the output resolution.
    let final_pixels = if ssaa > 1 {
        downsample(&pixels, w, h, ssaa)
    } else {
        pixels
    };

    encode_png(&final_pixels, opts.width.max(1), opts.height.max(1))
}

/// Box-downsample an RGBA8 image by an integer factor.
fn downsample(src: &[u8], w: u32, h: u32, factor: u32) -> Vec<u8> {
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

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder
            .write_header()
            .map_err(|e| RenderError::Encode(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| RenderError::Encode(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use molscene_core::scene::Scene;
    use molscene_core::selection::Expr;
    use molscene_core::spec::{Source, Style};

    fn ethylene_scene() -> Scene {
        let sdf = "ethylene\n  molscene\n\n  6  5  0  0  0  0  0  0  0  0999 V2000\n\
            0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n\
            1.3300    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n\
           -0.5000    0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
           -0.5000   -0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
            1.8300    0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
            1.8300   -0.9300    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n\
            1  2  2  0  0  0  0\n  1  3  1  0  0  0  0\n  1  4  1  0  0  0  0\n\
            2  5  1  0  0  0  0\n  2  6  1  0  0  0  0\nM  END\n";
        let mut scene = Scene::from_sdf(sdf, Source::InlineSdf { data: sdf.into() }).unwrap();
        scene
            .spheres(Expr::All, Style::default())
            .sticks(Expr::All, Style::default());
        scene
    }

    #[test]
    fn renders_a_valid_png_or_skips_without_gpu() {
        let scene = ethylene_scene();
        let spec = scene.to_geometry();
        let opts = RenderOptions {
            width: 200,
            height: 150,
            ssaa: 2,
        };
        match render_png(&spec, &opts) {
            Ok(png_bytes) => {
                // PNG magic + a decodable header at the requested size.
                assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");
                let decoder = png::Decoder::new(std::io::Cursor::new(&png_bytes));
                let reader = decoder.read_info().unwrap();
                let info = reader.info();
                assert_eq!((info.width, info.height), (200, 150));
            }
            Err(RenderError::NoAdapter) => {
                eprintln!("skipping: no GPU adapter in this environment");
            }
            Err(e) => panic!("unexpected render error: {e}"),
        }
    }
}
