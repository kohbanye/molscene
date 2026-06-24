//! GPU rasterizer: render a molscene [`GeometrySpec`] to a PNG image, headlessly.
//!
//! This is a second renderer for the same renderer-neutral contract the browser
//! bindings consume — it knows nothing about molecules, only the `GeometrySpec`
//! draw list (instanced spheres + cylinders, triangle meshes, an oriented-box
//! camera, a background color). Spheres and cylinders are drawn as GPU
//! *impostors* (the fragment shader ray-traces the exact primitive and writes
//! per-pixel depth), so they stay perfectly smooth at any zoom; cartoon/surface
//! meshes are drawn directly.
//!
//! The shared rendering core lives in [`gpu`] and compiles for both native and
//! `wasm32` (the browser bindings in `molscene-wasm` build on it for canvas
//! display + PNG download). This crate's own [`render_png`] is the native,
//! headless path: it sets up a GPU device with no surface, renders to an
//! offscreen texture (supersampled for antialiasing), reads the pixels back, and
//! encodes a PNG.

pub mod gpu;
mod math;

#[cfg(not(target_arch = "wasm32"))]
use molscene_core::geometry::GeometrySpec;

#[cfg(not(target_arch = "wasm32"))]
use gpu::{Frame, Orbit, Pipelines};

/// The offscreen color format for the native PNG path (the browser path uses
/// its surface's preferred format instead).
#[cfg(not(target_arch = "wasm32"))]
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

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
    /// The requested output size (× supersampling) overflows or exceeds the
    /// maximum renderable dimension.
    InvalidSize(String),
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
            RenderError::InvalidSize(e) => write!(f, "invalid render size: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render `spec` to PNG bytes. Blocks on the GPU; safe to call off the main
/// thread. Native only — the browser uses the async path in `molscene-wasm`.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_png(spec: &GeometrySpec, opts: &RenderOptions) -> Result<Vec<u8>, RenderError> {
    pollster::block_on(render_png_async(spec, opts))
}

#[cfg(not(target_arch = "wasm32"))]
async fn render_png_async(
    spec: &GeometrySpec,
    opts: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    let ssaa = opts.ssaa.max(1);
    let (w, h) =
        gpu::render_size(opts.width, opts.height, ssaa).map_err(RenderError::InvalidSize)?;

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

    let pipelines = Pipelines::new(&device, COLOR_FORMAT);
    let frame = Frame::build(
        &device,
        &pipelines,
        spec,
        opts.width,
        opts.height,
        Orbit::default(),
    );

    // Offscreen render targets (supersampled).
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
    let depth_view = gpu::depth_view(&device, w, h);

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
                    load: wgpu::LoadOp::Clear(frame.clear),
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
        gpu::record(&mut pass, &pipelines, &frame);
    }

    // Copy the color texture into a mappable buffer (rows padded to the copy
    // alignment), then read it back.
    let unpadded = w * 4;
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

    // Drop the row padding to a tight RGBA buffer at the supersampled size.
    let mut pixels = vec![0u8; (unpadded * h) as usize];
    for row in 0..h as usize {
        let src = row * padded as usize;
        let dst = row * unpadded as usize;
        pixels[dst..dst + unpadded as usize].copy_from_slice(&data[src..src + unpadded as usize]);
    }
    drop(data);
    out_buf.unmap();

    let final_pixels = gpu::downsample(&pixels, w, h, ssaa);
    gpu::encode_png(&final_pixels, opts.width.max(1), opts.height.max(1))
        .map_err(RenderError::Encode)
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
