use std::{
  io::{self, BufRead, Write},
  sync::{mpsc::{self, Receiver}, Arc},
  time::Instant,
};

use serde::{Deserialize, Serialize};
use wgpu::{Backends, CurrentSurfaceTexture};
use winit::{
  application::ApplicationHandler,
  dpi::PhysicalSize,
  event::WindowEvent,
  event_loop::{ActiveEventLoop, EventLoop},
  window::{Window, WindowAttributes, WindowId},
};

const DEFAULT_SHADER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
  var pos = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  let xy = pos[vertex_index];
  return vec4<f32>(xy, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = frag_coord.xy / vec2<f32>(1280.0, 720.0);
  return vec4<f32>(uv.x, uv.y, 0.5 + 0.5 * sin(frag_coord.x * 0.02), 1.0);
}
"#;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum BackendChoice {
  Auto,
  Dx12,
  Vulkan,
  Metal,
  Opengl,
}

impl BackendChoice {
  fn parse(value: Option<&str>) -> Self {
    match value.map(|entry| entry.to_ascii_lowercase()) {
      Some(value) if value == "dx12" => Self::Dx12,
      Some(value) if value == "vulkan" => Self::Vulkan,
      Some(value) if value == "metal" => Self::Metal,
      Some(value) if value == "opengl" || value == "gl" => Self::Opengl,
      _ => Self::Auto,
    }
  }

  fn to_wgpu(self) -> Backends {
    match self {
      Self::Auto => Backends::all(),
      Self::Dx12 => Backends::DX12,
      Self::Vulkan => Backends::VULKAN,
      Self::Metal => Backends::METAL,
      Self::Opengl => Backends::GL,
    }
  }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarCommand {
  Ping,
  Shutdown,
  SetShader { source: String },
  SetResolution { width: u32, height: u32 },
  SetBackend { backend: String },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarEvent {
  Started {
    requested_backend: BackendChoice,
    resolved_backend: String,
    adapter: String,
    device: String,
  },
  Stats {
    fps: f64,
    frame_time_ms: f64,
    frame: u64,
  },
  Diagnostic {
    level: &'static str,
    message: String,
  },
  Pong,
  ShaderUpdated {
    success: bool,
  },
  BackendChangeRequired {
    message: String,
  },
}

struct RendererState {
  _instance: wgpu::Instance,
  surface: wgpu::Surface<'static>,
  _adapter: wgpu::Adapter,
  device: wgpu::Device,
  queue: wgpu::Queue,
  config: wgpu::SurfaceConfiguration,
  pipeline: wgpu::RenderPipeline,
}

struct PreviewApp {
  requested_backend: BackendChoice,
  shader_source: String,
  commands: Receiver<SidecarCommand>,
  renderer: Option<RendererState>,
  window: Option<Arc<Window>>,
  should_exit: bool,
  last_stats_at: Instant,
  frame_counter: u64,
}

impl PreviewApp {
  fn new(requested_backend: BackendChoice, commands: Receiver<SidecarCommand>) -> Self {
    Self {
      requested_backend,
      shader_source: DEFAULT_SHADER.to_string(),
      commands,
      renderer: None,
      window: None,
      should_exit: false,
      last_stats_at: Instant::now(),
      frame_counter: 0,
    }
  }

  fn init_renderer(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
    if self.renderer.is_some() {
      return Ok(());
    }

    let window = Arc::new(
      event_loop
        .create_window(
          WindowAttributes::default()
            .with_title("PersonalShaderToy Native Preview")
            .with_inner_size(PhysicalSize::new(1280, 720)),
        )
        .map_err(|error| error.to_string())?,
    );

    let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_desc.backends = self.requested_backend.to_wgpu();
    let instance = wgpu::Instance::new(instance_desc);

    let surface = instance.create_surface(window.clone()).map_err(|error| error.to_string())?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::HighPerformance,
      compatible_surface: Some(&surface),
      force_fallback_adapter: false,
    }))
    .map_err(|error| error.to_string())?;

    let adapter_info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
      label: Some("preview-sidecar-device"),
      required_features: wgpu::Features::empty(),
      required_limits: wgpu::Limits::default(),
      experimental_features: Default::default(),
      memory_hints: wgpu::MemoryHints::Performance,
      trace: wgpu::Trace::Off,
    }))
    .map_err(|error| error.to_string())?;

    let size = window.inner_size();
    let capabilities = surface.get_capabilities(&adapter);
    let surface_format = capabilities
      .formats
      .iter()
      .copied()
      .find(|format| format.is_srgb())
      .unwrap_or(capabilities.formats[0]);

    let config = wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format: surface_format,
      width: size.width.max(1),
      height: size.height.max(1),
      present_mode: wgpu::PresentMode::Fifo,
      desired_maximum_frame_latency: 2,
      alpha_mode: capabilities.alpha_modes[0],
      view_formats: vec![],
    };

    surface.configure(&device, &config);
    let pipeline = create_pipeline(&device, config.format, &self.shader_source)?;

    emit(&SidecarEvent::Started {
      requested_backend: self.requested_backend,
      resolved_backend: format!("{:?}", adapter_info.backend).to_ascii_lowercase(),
      adapter: adapter_info.name.clone(),
      device: adapter_info.name,
    });

    self.window = Some(window);
    self.renderer = Some(RendererState {
      _instance: instance,
      surface,
      _adapter: adapter,
      device,
      queue,
      config,
      pipeline,
    });

    Ok(())
  }

  fn process_commands(&mut self) {
    while let Ok(command) = self.commands.try_recv() {
      match command {
        SidecarCommand::Ping => emit(&SidecarEvent::Pong),
        SidecarCommand::Shutdown => self.should_exit = true,
        SidecarCommand::SetShader { source } => {
          self.shader_source = source;
          match self.rebuild_pipeline() {
            Ok(()) => emit(&SidecarEvent::ShaderUpdated { success: true }),
            Err(error) => {
              emit(&SidecarEvent::Diagnostic {
                level: "error",
                message: error,
              });
              emit(&SidecarEvent::ShaderUpdated { success: false });
            }
          }
        }
        SidecarCommand::SetResolution { width, height } => {
          if let Some(window) = &self.window {
            let _ = window.request_inner_size(PhysicalSize::new(width.max(1), height.max(1)));
          }
        }
        SidecarCommand::SetBackend { backend } => {
          emit(&SidecarEvent::BackendChangeRequired {
            message: format!(
              "Backend change to '{backend}' requires restarting the preview-sidecar in this spike."
            ),
          });
        }
      }
    }
  }

  fn rebuild_pipeline(&mut self) -> Result<(), String> {
    let Some(renderer) = self.renderer.as_mut() else {
      return Ok(());
    };

    renderer.pipeline = create_pipeline(&renderer.device, renderer.config.format, &self.shader_source)?;
    Ok(())
  }

  fn resize(&mut self, size: PhysicalSize<u32>) {
    let Some(renderer) = self.renderer.as_mut() else {
      return;
    };

    if size.width == 0 || size.height == 0 {
      return;
    }

    renderer.config.width = size.width;
    renderer.config.height = size.height;
    renderer.surface.configure(&renderer.device, &renderer.config);
  }

  fn render(&mut self) {
    let Some(renderer) = self.renderer.as_mut() else {
      return;
    };

    let frame = match renderer.surface.get_current_texture() {
      CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => frame,
      CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
        renderer.surface.configure(&renderer.device, &renderer.config);
        return;
      }
      CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
        emit(&SidecarEvent::Diagnostic {
          level: "warning",
          message: "Surface is temporarily unavailable.".to_string(),
        });
        return;
      }
      CurrentSurfaceTexture::Validation => {
        emit(&SidecarEvent::Diagnostic {
          level: "error",
          message: "Surface validation failed while acquiring the next frame.".to_string(),
        });
        return;
      }
    };

    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = renderer
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("preview-sidecar-encoder"),
      });

    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("preview-sidecar-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
              r: 0.02,
              g: 0.02,
              b: 0.03,
              a: 1.0,
            }),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });

      pass.set_pipeline(&renderer.pipeline);
      pass.draw(0..3, 0..1);
    }

    renderer.queue.submit(Some(encoder.finish()));
    frame.present();

    self.frame_counter += 1;
    let elapsed = self.last_stats_at.elapsed();
    if elapsed.as_secs_f64() >= 1.0 {
      let fps = self.frame_counter as f64 / elapsed.as_secs_f64();
      emit(&SidecarEvent::Stats {
        fps,
        frame_time_ms: if fps > 0.0 { 1000.0 / fps } else { 0.0 },
        frame: self.frame_counter,
      });
      self.frame_counter = 0;
      self.last_stats_at = Instant::now();
    }
  }
}

impl ApplicationHandler for PreviewApp {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    if let Err(error) = self.init_renderer(event_loop) {
      emit(&SidecarEvent::Diagnostic {
        level: "error",
        message: error,
      });
      event_loop.exit();
    }
  }

  fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
    match event {
      WindowEvent::CloseRequested => event_loop.exit(),
      WindowEvent::RedrawRequested => self.render(),
      WindowEvent::Resized(size) => self.resize(size),
      _ => {}
    }
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    self.process_commands();
    if self.should_exit {
      event_loop.exit();
      return;
    }

    if let Some(window) = &self.window {
      window.request_redraw();
    }
  }
}

fn create_pipeline(
  device: &wgpu::Device,
  format: wgpu::TextureFormat,
  source: &str,
) -> Result<wgpu::RenderPipeline, String> {
  let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
  let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    label: Some("preview-sidecar-shader"),
    source: wgpu::ShaderSource::Wgsl(source.into()),
  });

  let info = pollster::block_on(shader.get_compilation_info());
  let errors = info
    .messages
    .iter()
    .filter(|message| matches!(message.message_type, wgpu::CompilationMessageType::Error))
    .map(|message| {
      if let Some(location) = &message.location {
        format!("{}:{} {}", location.line_number, location.line_position, message.message)
      } else {
        message.message.clone()
      }
    })
    .collect::<Vec<_>>();

  if !errors.is_empty() {
    return Err(errors.join("\n"));
  }

  let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    label: Some("preview-sidecar-pipeline"),
    layout: None,
    vertex: wgpu::VertexState {
      module: &shader,
      entry_point: Some("vs_main"),
      buffers: &[],
      compilation_options: Default::default(),
    },
    primitive: wgpu::PrimitiveState::default(),
    depth_stencil: None,
    multisample: wgpu::MultisampleState::default(),
    fragment: Some(wgpu::FragmentState {
      module: &shader,
      entry_point: Some("fs_main"),
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

fn spawn_stdin_thread() -> Receiver<SidecarCommand> {
  let (tx, rx) = mpsc::channel();

  std::thread::spawn(move || {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
      let Ok(line) = line else {
        break;
      };

      let trimmed = line.trim();
      if trimmed.is_empty() {
        continue;
      }

      match serde_json::from_str::<SidecarCommand>(trimmed) {
        Ok(command) => {
          if tx.send(command).is_err() {
            break;
          }
        }
        Err(error) => emit(&SidecarEvent::Diagnostic {
          level: "error",
          message: format!("Failed to parse command: {error}"),
        }),
      }
    }
  });

  rx
}

fn emit(event: &SidecarEvent) {
  if let Ok(text) = serde_json::to_string(event) {
    println!("{text}");
    let _ = io::stdout().flush();
  }
}

fn main() {
  let requested_backend = BackendChoice::parse(std::env::args().skip(1).next().as_deref());
  let commands = spawn_stdin_thread();
  let event_loop = EventLoop::new().expect("failed to create event loop");
  let mut app = PreviewApp::new(requested_backend, commands);
  event_loop.run_app(&mut app).expect("failed to run preview-sidecar");
}
