//! Browser renderer: draws a molscene scene with the shared wgpu core
//! (`molscene-render`) onto a `<canvas>` (live, orbitable) and exports PNG bytes
//! (offscreen, supersampled). This is the browser counterpart to the native
//! `molscene_render::render_png`; both consume the same `GeometrySpec` and share
//! the impostor/mesh pipelines, so the canvas view and a downloaded PNG match.
//!
//! The renderer consumes a serialized `GeometrySpec` (the one wire format) — not
//! a `Scene` — so the same path serves the JS demo (which builds a `Scene` then
//! `toGeometryJson()`) and the Python notebook (which already has the compiled
//! spec). Load a spec once with `loadSpecJson`, then `draw` per camera change.

use std::cell::RefCell;
use std::rc::Rc;

use molscene_core::geometry::GeometrySpec;
use molscene_render::gpu::{self, Frame, Orbit, Pipelines};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// Offscreen color format for PNG export (the canvas uses its own surface
/// format, which may be BGRA).
const PNG_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct Inner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: RefCell<wgpu::SurfaceConfiguration>,
    /// Pipelines for the canvas surface format.
    pipelines: Pipelines,
    /// Pipelines for the offscreen PNG format (Rgba8UnormSrgb).
    png_pipelines: Pipelines,
    /// The geometry currently loaded for display (the only wire format).
    spec: RefCell<Option<GeometrySpec>>,
}

/// A WebGPU renderer bound to a canvas. Create it once with
/// `await Renderer.create(canvas)`, `loadSpecJson(spec)` whenever the geometry
/// changes, then `draw(yaw, pitch, zoom)` on pointer drag / wheel, and
/// `toPng(...)` for a downloadable image.
#[wasm_bindgen]
pub struct Renderer {
    inner: Rc<Inner>,
}

#[wasm_bindgen]
impl Renderer {
    /// Async constructor: request a WebGPU adapter/device for `canvas` and
    /// configure its surface. Rejects if the browser has no WebGPU support.
    pub async fn create(canvas: HtmlCanvasElement) -> Result<Renderer, JsValue> {
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsValue::from_str(&format!("create_surface failed: {e}")))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .ok_or_else(|| {
                JsValue::from_str("no WebGPU adapter (does this browser support WebGPU?)")
            })?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("molscene-wasm"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|e| JsValue::from_str(&format!("request_device failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let pipelines = Pipelines::new(&device, format);
        let png_pipelines = Pipelines::new(&device, PNG_FORMAT);

        Ok(Renderer {
            inner: Rc::new(Inner {
                device,
                queue,
                surface,
                config: RefCell::new(config),
                pipelines,
                png_pipelines,
                spec: RefCell::new(None),
            }),
        })
    }

    /// Load a serialized `GeometrySpec` (JSON) for display. Call whenever the
    /// scene changes; `draw` then renders it at any camera without re-parsing.
    #[wasm_bindgen(js_name = loadSpecJson)]
    pub fn load_spec_json(&self, json: &str) -> Result<(), JsValue> {
        let spec: GeometrySpec = serde_json::from_str(json)
            .map_err(|e| JsValue::from_str(&format!("invalid GeometrySpec JSON: {e}")))?;
        *self.inner.spec.borrow_mut() = Some(spec);
        Ok(())
    }

    /// Resize the canvas surface (call when the element's pixel size changes).
    pub fn resize(&self, width: u32, height: u32) {
        let inner = &self.inner;
        let mut config = inner.config.borrow_mut();
        config.width = width.max(1);
        config.height = height.max(1);
        inner.surface.configure(&inner.device, &config);
    }

    /// Draw the loaded geometry to the canvas with an interactive camera:
    /// `yaw`/`pitch` (radians) orbit the framed center, `zoom > 1` moves closer.
    /// A no-op until `loadSpecJson` has been called.
    pub fn draw(&self, yaw: f32, pitch: f32, zoom: f32) -> Result<(), JsValue> {
        let inner = &self.inner;
        let spec = inner.spec.borrow();
        let Some(spec) = spec.as_ref() else {
            return Ok(());
        };
        let (width, height) = {
            let c = inner.config.borrow();
            (c.width, c.height)
        };
        let frame = Frame::build(
            &inner.device,
            &inner.pipelines,
            spec,
            width,
            height,
            Orbit { yaw, pitch, zoom },
        );

        let surface_tex = inner
            .surface
            .get_current_texture()
            .map_err(|e| JsValue::from_str(&format!("acquire frame failed: {e}")))?;
        let view = surface_tex
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth = gpu::depth_view(&inner.device, width, height);

        let mut encoder = inner
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surface-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(frame.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            gpu::record(&mut pass, &inner.pipelines, &frame);
        }
        inner.queue.submit(Some(encoder.finish()));
        surface_tex.present();
        Ok(())
    }

    /// Render the loaded geometry to a PNG offscreen and resolve to its bytes (a
    /// `Uint8Array`), e.g. for a download link. `ssaa` is the antialiasing
    /// supersample factor. Returns a Promise.
    #[wasm_bindgen(js_name = toPng)]
    pub fn to_png(&self, width: u32, height: u32, ssaa: u32) -> js_sys::Promise {
        let inner = self.inner.clone();
        // Clone the spec out before the async boundary (no RefCell borrow across await).
        let spec = inner.spec.borrow().clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let Some(spec) = spec else {
                return Err(JsValue::from_str(
                    "no geometry loaded (call loadSpecJson first)",
                ));
            };
            let bytes = inner.render_png(&spec, width, height, ssaa).await?;
            Ok(js_sys::Uint8Array::from(&bytes[..]).into())
        })
    }
}

impl Inner {
    /// Offscreen render + async readback → PNG bytes.
    async fn render_png(
        &self,
        spec: &GeometrySpec,
        width: u32,
        height: u32,
        ssaa: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let ssaa = ssaa.max(1);
        let (w, h) = gpu::render_size(width, height, ssaa).map_err(|e| JsValue::from_str(&e))?;

        let frame = Frame::build(
            &self.device,
            &self.png_pipelines,
            spec,
            width,
            height,
            Orbit::default(),
        );

        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let color_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("png-color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PNG_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = gpu::depth_view(&self.device, w, h);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("png") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("png-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(frame.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            gpu::record(&mut pass, &self.png_pipelines, &frame);
        }

        let unpadded = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("png-readback"),
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
        self.queue.submit(Some(encoder.finish()));

        // Async map: the browser drives the GPU and fires the callback once we
        // yield at `.await` (no blocking poll on the web).
        let slice = out_buf.slice(..);
        let (tx, rx) = futures_channel::oneshot::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::Maintain::Poll);
        rx.await
            .map_err(|_| JsValue::from_str("readback channel dropped"))?
            .map_err(|e| JsValue::from_str(&format!("buffer map failed: {e}")))?;
        let data = slice.get_mapped_range();

        let mut pixels = vec![0u8; (unpadded * h) as usize];
        for row in 0..h as usize {
            let src = row * padded as usize;
            let dst = row * unpadded as usize;
            pixels[dst..dst + unpadded as usize]
                .copy_from_slice(&data[src..src + unpadded as usize]);
        }
        drop(data);
        out_buf.unmap();

        let final_pixels = gpu::downsample(&pixels, w, h, ssaa);
        gpu::encode_png(&final_pixels, width.max(1), height.max(1))
            .map_err(|e| JsValue::from_str(&e))
    }
}
