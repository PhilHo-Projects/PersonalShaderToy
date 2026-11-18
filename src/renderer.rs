use crate::{capture, gpu_timer::GpuTimer, render_settings::RenderSettings};
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use shader_core::Prepared;
use std::sync::Arc;
use winit::window::Window;
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
struct PingPongTarget {
    views: [wgpu::TextureView; 2],
    read_index: usize,
}

impl PingPongTarget {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) -> Self {
        let make_tex = || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ping-pong-target"),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OFFSCREEN_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let t0 = make_tex();
        let t1 = make_tex();
        let v0 = t0.create_view(&Default::default());
        let v1 = t1.create_view(&Default::default());

        // Explicitly clear both textures to black on creation. Without this,
        // a freshly-allocated wgpu texture has driver-dependent undefined
        // content — zeros on some drivers, STALE GPU MEMORY on others. A
        // multipass shader that does TAA self-feedback (Buffer A reading its
        // own previous frame via iChannel3) will blend incoming frames with
        // that garbage, producing a persistent "ghost" of whatever was in
        // GPU memory before — commonly the previous window contents, which
        // looks like a duplicate of the black hole after a resize.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ping-pong-clear"),
        });
        for view in [&v0, &v1] {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ping-pong-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        queue.submit(std::iter::once(encoder.finish()));

        Self {
            views: [v0, v1],
            read_index: 0,
        }
    }

    pub fn read_view(&self) -> &wgpu::TextureView {
        &self.views[self.read_index]
    }

    pub fn write_view(&self) -> &wgpu::TextureView {
        &self.views[1 - self.read_index]
    }

    pub fn swap(&mut self) {
        self.read_index = 1 - self.read_index;
    }
}

struct Pass {
    name: String,
    channels: [Option<String>; 4],
    uniform: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    target: Option<PingPongTarget>,
    legacy: bool,
}
pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub available_adapters: Vec<String>,
    pub present_modes: Vec<wgpu::PresentMode>,
    pub info: wgpu::AdapterInfo,
    pub egui_ctx: egui::Context,
    pub egui_state: EguiWinitState,
    egui_renderer: EguiRenderer,
    multi_bgl: wgpu::BindGroupLayout,
    multi_pl: wgpu::PipelineLayout,
    linear_sampler: wgpu::Sampler,
    dummy_view: wgpu::TextureView,
    keyboard_texture: wgpu::Texture,
    keyboard_view: wgpu::TextureView,
    image: wgpu::Texture,
    image_view: wgpu::TextureView,
    pub image_id: egui::TextureId,
    pub size: [u32; 2],
    passes: Vec<Pass>,
    pub timer: Option<GpuTimer>,
}
impl Renderer {
    pub fn new(window: Arc<Window>, settings: &RenderSettings) -> Result<Self, String> {
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        let requested_backends = settings.backend.to_wgpu();
        instance_desc.backends = requested_backends;
        instance_desc.backend_options.dx12.shader_compiler = settings.dx12_compiler.to_wgpu();
        let instance = wgpu::Instance::new(instance_desc);

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let mut adapters = pollster::block_on(instance.enumerate_adapters(requested_backends));
        let mut available_adapters: Vec<String> = adapters
            .iter()
            .filter(|adapter| adapter.is_surface_supported(&surface))
            .map(|adapter| adapter.get_info().name)
            .collect();
        available_adapters.dedup();

        // Explicit adapter request by name takes priority over backend order.
        let named_index = settings.adapter_name.as_ref().and_then(|wanted| {
            adapters.iter().position(|adapter| {
                adapter.is_surface_supported(&surface)
                    && adapter.get_info().name.split("/PCIe").next() == wanted.split("/PCIe").next()
            })
        });
        if settings.adapter_name.is_some() && named_index.is_none() {
            return Err(format!("Adapter unavailable: {:?}", settings.adapter_name));
        }
        let preferred_order = settings.backend.preferred_backend_order();
        let preferred_index = named_index.or_else(|| {
            preferred_order.iter().find_map(|preferred_backend| {
                adapters.iter().position(|adapter| {
                    adapter.is_surface_supported(&surface)
                        && adapter.get_info().backend == *preferred_backend
                })
            })
        });
        let adapter = if let Some(index) = preferred_index {
            adapters.swap_remove(index)
        } else {
            adapters
                .into_iter()
                .find(|adapter| adapter.is_surface_supported(&surface))
                .ok_or_else(|| {
                    "No compatible GPU adapter found for the preview surface.".to_string()
                })?
        };

        let adapter_info = adapter.get_info();
        // GPU pass timing needs timestamp queries; request them only when the
        // adapter offers them (GL typically does not).
        let mut required_features = wgpu::Features::empty();
        if adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("personal-shadertoy-device"),
            required_features,
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| e.to_string())?;

        let size = window.inner_size();
        let capabilities = surface.get_capabilities(&adapter);
        let surface_format = capabilities
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
            .unwrap_or(capabilities.alpha_modes[0]);

        let present_modes = capabilities.present_modes.clone();
        // COPY_SRC enables screenshots, but not every surface supports it
        // (GL swapchains typically don't) — configure() panics on unsupported
        // usages, so only request what the surface offers.
        let mut surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if capabilities.usages.contains(wgpu::TextureUsages::COPY_SRC) {
            surface_usage |= wgpu::TextureUsages::COPY_SRC;
        }
        let config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: settings.resolve_present_mode(&capabilities.present_modes),
            desired_maximum_frame_latency: settings.frame_latency,
            alpha_mode,
            view_formats: vec![],
        };

        // Surface::configure reports failures through the device error sink
        // (fatal panic by default) instead of a Result. Capture them in an
        // error scope so a backend that cannot drive this window — e.g. DXGI
        // refusing a swapchain on a GL-tainted HWND — comes back as Err and
        // the rebuild revert path can recover instead of aborting the app.
        let configure_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &config);
        if let Some(error) = pollster::block_on(configure_scope.pop()) {
            return Err(format!(
                "Surface configuration failed on {:?}: {error}",
                adapter_info.backend
            ));
        }

        let multi_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("multi-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_binding_entry(1),
                tex_binding_entry(2),
                tex_binding_entry(3),
                tex_binding_entry(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let multi_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("multi-pl"),
            bind_group_layouts: &[Some(&multi_bgl)],
            immediate_size: 0,
        });

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        // 1x1 black dummy texture
        let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dummy-tex"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &dummy_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0u8, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = dummy_texture.create_view(&Default::default());

        // 256x3 keyboard texture
        let keyboard_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("keyboard-tex"),
            size: wgpu::Extent3d {
                width: 256,
                height: 3,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let keyboard_view = keyboard_texture.create_view(&Default::default());

        let egui_ctx = egui::Context::default();
        let egui_state = EguiWinitState::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = EguiRenderer::new(
            &device,
            surface_format,
            RendererOptions {
                depth_stencil_format: None,
                ..Default::default()
            },
        );

        let size = [1, 1];
        let (image, image_view) = Self::image(&device, size);
        let mut egui_renderer = egui_renderer;
        let image_id =
            egui_renderer.register_native_texture(&device, &image_view, wgpu::FilterMode::Linear);
        let timer = None;
        Ok(Self {
            surface,
            device,
            queue,
            config,
            available_adapters,
            present_modes,
            info: adapter_info,
            egui_ctx,
            egui_state,
            egui_renderer,
            multi_bgl,
            multi_pl,
            linear_sampler,
            dummy_view,
            keyboard_texture,
            keyboard_view,
            image,
            image_view,
            image_id,
            size,
            passes: Vec::new(),
            timer,
        })
    }
    fn image(device: &wgpu::Device, [w, h]: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shader image"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        (tex, view)
    }
    pub fn resize(&mut self, size: [u32; 2]) {
        if size == self.size {
            return;
        }
        self.size = size;
        (self.image, self.image_view) = Self::image(&self.device, size);
        self.egui_renderer.update_egui_texture_from_wgpu_texture(
            &self.device,
            &self.image_view,
            wgpu::FilterMode::Linear,
            self.image_id,
        );
        self.reset_feedback();
    }
    pub fn has_shader(&self) -> bool {
        !self.passes.is_empty()
    }
    pub fn reset_feedback(&mut self) {
        for p in &mut self.passes {
            if p.name != "Image" {
                p.target = Some(PingPongTarget::new(
                    &self.device,
                    &self.queue,
                    self.size[0],
                    self.size[1],
                ));
            }
        }
        self.timer = GpuTimer::new(&self.device, &self.queue, self.passes.len());
    }
    pub fn install(&mut self, prepared: &[Prepared]) -> Result<(), String> {
        let mut next = Vec::with_capacity(prepared.len());
        for p in prepared {
            let is_image = p.name == "Image";
            let pipeline = create_pipeline(
                &self.device,
                if is_image {
                    IMAGE_FORMAT
                } else {
                    OFFSCREEN_FORMAT
                },
                &self.multi_pl,
                &p.wgsl,
                &p.entry,
            )
            .map_err(|e| format!("{}: {e}", p.name))?;
            let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&p.name),
                size: 144,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: false,
            });
            let target = (!is_image).then(|| {
                PingPongTarget::new(&self.device, &self.queue, self.size[0], self.size[1])
            });
            next.push(Pass {
                name: p.name.clone(),
                channels: p.channels.clone(),
                legacy: p.legacy,
                uniform,
                pipeline,
                target,
            });
        }
        self.passes = next;
        self.timer = GpuTimer::new(&self.device, &self.queue, self.passes.len());
        Ok(())
    }

    pub fn draw(
        &mut self,
        time: f32,
        delta: f32,
        frame: i32,
        mouse: [f32; 4],
        date: [f32; 4],
        keys: &[u8],
    ) -> Vec<f64> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        let gpu = self
            .timer
            .as_mut()
            .map(GpuTimer::begin_frame)
            .unwrap_or_default();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.keyboard_texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: wgpu::TextureAspect::All,
            },
            keys,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 256,
                height: 3,
                depth_or_array_layers: 1,
            },
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let [w, h] = self.size.map(|n| n as f32);
        for i in 0..self.passes.len() {
            let p = &self.passes[i];
            let mut data = [0f32; 36];
            data[0] = time;
            data[1] = delta;
            data[if p.legacy { 1 } else { 2 }] = f32::from_bits(frame as u32);
            data[4..8].copy_from_slice(&[w, h, 1., 0.]);
            data[8..12].copy_from_slice(&mouse);
            data[12..16].copy_from_slice(&date);
            let views: [&wgpu::TextureView; 4] = std::array::from_fn(|n| {
                let (view, dims) = match p.channels[n].as_deref() {
                    Some("keyboard") => (&self.keyboard_view, [256., 3., 1., 0.]),
                    Some(ch) => self
                        .passes
                        .iter()
                        .find(|p| p.name == ch)
                        .and_then(|v| v.target.as_ref())
                        .map_or((&self.dummy_view, [1., 1., 1., 0.]), |t| {
                            (t.read_view(), [w, h, 1., 0.])
                        }),
                    None => (&self.dummy_view, [1., 1., 1., 0.]),
                };
                data[16 + 4 * n..20 + 4 * n].copy_from_slice(&dims);
                view
            });
            let k = if p.legacy { 16 } else { 32 };
            data[k..k + 4].copy_from_slice(&[0., 0., w, h]);
            self.queue
                .write_buffer(&p.uniform, 0, bytemuck::cast_slice(&data));
            let entries = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: p.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(views[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ];
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&p.name),
                layout: &self.multi_bgl,
                entries: &entries,
            });
            let view = p
                .target
                .as_ref()
                .map_or(&self.image_view, |t| t.write_view());
            {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(&p.name),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    timestamp_writes: self.timer.as_mut().and_then(|t| t.pass_timestamp_writes()),
                    ..Default::default()
                });
                rp.set_pipeline(&p.pipeline);
                rp.set_bind_group(0, &bg, &[]);
                rp.draw(0..3, 0..1);
            }
            if let Some(t) = self.passes[i].target.as_mut() {
                t.swap();
            }
        }
        let slot = self.timer.as_mut().and_then(|t| t.resolve(&mut encoder));
        self.queue.submit([encoder.finish()]);
        if let (Some(slot), Some(t)) = (slot, &mut self.timer) {
            t.after_submit(slot);
        }
        gpu
    }
    pub fn timed_passes(&self) -> Vec<(String, f64)> {
        self.timer
            .as_ref()
            .and_then(|t| t.latest())
            .map(|t| {
                self.passes
                    .iter()
                    .zip(&t.pass_ms)
                    .map(|(p, ms)| (p.name.clone(), *ms))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn screenshot(&self) -> Result<std::path::PathBuf, String> {
        let mut enc = self.device.create_command_encoder(&Default::default());
        let read = capture::queue_screenshot_copy(
            &self.device,
            &mut enc,
            &self.image,
            self.size[0],
            self.size[1],
            IMAGE_FORMAT,
        )?;
        self.queue.submit([enc.finish()]);
        capture::finish_screenshot_readback(&self.device, read)
    }
    pub fn paint(&mut self, window: &Window, out: egui::FullOutput) {
        self.egui_state
            .handle_platform_output(window, out.platform_output);
        let screen = ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: out.pixels_per_point,
        };
        let shapes = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
        for (id, d) in &out.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, d);
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        let extra = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut enc,
            &shapes,
            &screen,
        );
        {
            let rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.egui_renderer
                .render(&mut rp.forget_lifetime(), &shapes, &screen);
        }
        self.queue.submit(extra.into_iter().chain([enc.finish()]));
        for id in out.textures_delta.free {
            self.egui_renderer.free_texture(&id);
        }
        frame.present();
    }
}
fn create_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    pipeline_layout: &wgpu::PipelineLayout,
    source: &str,
    entry: &str,
) -> Result<wgpu::RenderPipeline, String> {
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("preview-shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let info = pollster::block_on(shader.get_compilation_info());
    let errors: Vec<String> = info
        .messages
        .iter()
        .filter(|m| matches!(m.message_type, wgpu::CompilationMessageType::Error))
        .map(|m| {
            if let Some(loc) = &m.location {
                format!("{}:{} {}", loc.line_number, loc.line_position, m.message)
            } else {
                m.message.clone()
            }
        })
        .collect();

    if !errors.is_empty() {
        let _ = pollster::block_on(error_scope.pop());
        return Err(errors.join("\n"));
    }

    let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fullscreen triangle"),
        source: wgpu::ShaderSource::Wgsl(shader_core::VERTEX_WGSL.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("preview-pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &vertex,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    });

    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(error.to_string());
    }

    Ok(pipeline)
}
fn tex_binding_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
