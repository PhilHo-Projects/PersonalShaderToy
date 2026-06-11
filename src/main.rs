use notify::{Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, BufRead, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes, WindowId},
};

use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;

mod bench;
mod gpu_timer;
mod render_settings;

use render_settings::BackendChoice;

const RESIZE_SETTLE_DELAY: Duration = Duration::from_millis(120);
const DEFAULT_WINDOW_WIDTH: u32 = 1600;
const DEFAULT_WINDOW_HEIGHT: u32 = 900;
const MIN_WINDOW_WIDTH: u32 = 1100;
const MIN_WINDOW_HEIGHT: u32 = 680;
const DEFAULT_SHADER_LIST_RATIO: f32 = 0.20;
const DEFAULT_PREVIEW_PANEL_RATIO: f32 = 0.50;
const MIN_SHADER_LIST_WIDTH: f32 = 180.0;
const MAX_SHADER_LIST_WIDTH: f32 = 360.0;
const MIN_PREVIEW_PANEL_WIDTH: f32 = 360.0;
const MAX_PREVIEW_PANEL_WIDTH: f32 = 960.0;
const PREVIEW_STATUS_ROW_HEIGHT: f32 = 22.0;
const PREVIEW_LOG_HEIGHT: f32 = 200.0;
const PREVIEW_LAYOUT_GUTTER: f32 = 30.0;
const SCREENSHOT_DIR: &str = "target/native-captures";
/// Rolling frame-time window for live percentile stats (~10s at 60fps).
const STATS_HISTORY: usize = 600;

// ═══════════════════════════════════════════════════════════════════════════════
// WGSL templates — matching the browser WebGPURenderer for shader compatibility
// ═══════════════════════════════════════════════════════════════════════════════

const VERTEX_WGSL: &str = r#"
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
"#;

const SINGLE_HEADER: &str = r#"
struct ShaderUniforms {
  iTime: f32,
  iFrame: i32,
  _pad0: vec2<f32>,
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
  iViewportOrigin: vec2<f32>,
  iViewportSize: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: ShaderUniforms;
"#;

const SINGLE_FOOTER: &str = r#"
@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
  let adjusted_coord = ((frag_coord.xy - u.iViewportOrigin) / u.iViewportSize) * u.iResolution.xy;
  return mainImage(adjusted_coord);
}
"#;

const MULTI_HEADER: &str = r#"
struct MultiPassUniforms {
  iTime: f32,
  iTimeDelta: f32,
  iFrame: i32,
  _pad0: f32,
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
  iChannelResolution: array<vec4<f32>, 4>,
  iViewportOrigin: vec2<f32>,
  iViewportSize: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: MultiPassUniforms;
@group(0) @binding(1) var iChannel0: texture_2d<f32>;
@group(0) @binding(2) var iChannel1: texture_2d<f32>;
@group(0) @binding(3) var iChannel2: texture_2d<f32>;
@group(0) @binding(4) var iChannel3: texture_2d<f32>;
@group(0) @binding(5) var iLinearSampler: sampler;

fn stSample(tex: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
  return textureSampleLevel(tex, iLinearSampler, vec2<f32>(uv.x, 1.0 - uv.y), 0.0);
}

fn stTexelFetch(tex: texture_2d<f32>, coord: vec2<i32>) -> vec4<f32> {
  let dims = vec2<i32>(textureDimensions(tex, 0));
  let y = clamp(dims.y - 1 - coord.y, 0, dims.y - 1);
  let x = clamp(coord.x, 0, dims.x - 1);
  return textureLoad(tex, vec2<i32>(x, y), 0);
}
"#;

const MULTI_FOOTER: &str = r#"
@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
  let local_coord = ((frag_coord.xy - u.iViewportOrigin) / u.iViewportSize) * u.iResolution.xy;
  let st_frag_coord = vec2<f32>(local_coord.x, u.iResolution.y - local_coord.y);
  return mainImage(st_frag_coord);
}
"#;

const GLSL_SINGLE_HEADER: &str = r#"#version 450
layout(binding = 0, set = 0) uniform ShaderUniforms {
  float iTime;
  int iFrame;
  vec2 _pad0;
  vec4 iResolution;
  vec4 iMouse;
  vec4 iDate;
  vec2 iViewportOrigin;
  vec2 iViewportSize;
};
"#;

const GLSL_SINGLE_FOOTER: &str = r#"
layout(location = 0) out vec4 outColor;
void main() {
    vec4 color = vec4(0.0, 0.0, 0.0, 1.0);
    vec2 local_coord = ((gl_FragCoord.xy - iViewportOrigin) / iViewportSize) * iResolution.xy;
    vec2 adjusted_coord = vec2(local_coord.x, iResolution.y - local_coord.y);
    mainImage(color, adjusted_coord);
    outColor = color;
}
"#;

const GLSL_MULTI_HEADER: &str = r#"#version 450
layout(binding = 0, set = 0) uniform MultiPassUniforms {
  float iTime;
  float iTimeDelta;
  int iFrame;
  float _pad0;
  vec4 iResolution;
  vec4 iMouse;
  vec4 iDate;
  vec4 iChannelResolution[4];
  vec2 iViewportOrigin;
  vec2 iViewportSize;
};

layout(binding = 1, set = 0) uniform texture2D t_iChannel0;
layout(binding = 2, set = 0) uniform texture2D t_iChannel1;
layout(binding = 3, set = 0) uniform texture2D t_iChannel2;
layout(binding = 4, set = 0) uniform texture2D t_iChannel3;
layout(binding = 5, set = 0) uniform sampler iLinearSampler;

#define iChannel0 sampler2D(t_iChannel0, iLinearSampler)
#define iChannel1 sampler2D(t_iChannel1, iLinearSampler)
#define iChannel2 sampler2D(t_iChannel2, iLinearSampler)
#define iChannel3 sampler2D(t_iChannel3, iLinearSampler)
"#;

const GLSL_MULTI_FOOTER: &str = r#"
layout(location = 0) out vec4 outColor;
void main() {
    vec4 color = vec4(0.0, 0.0, 0.0, 1.0);
    vec2 local_coord = ((gl_FragCoord.xy - iViewportOrigin) / iViewportSize) * iResolution.xy;
    vec2 adjusted_coord = vec2(local_coord.x, iResolution.y - local_coord.y);
    mainImage(color, adjusted_coord);
    outColor = color;
}
"#;

const GLSL_COMPAT_HELPERS: &str = r#"
const float PST_FLT_MAX = 3.402823466e+38;

bool pst_isnan(float v) { return v != v; }
bvec2 pst_isnan(vec2 v) { return notEqual(v, v); }
bvec3 pst_isnan(vec3 v) { return notEqual(v, v); }
bvec4 pst_isnan(vec4 v) { return notEqual(v, v); }

bool pst_isinf(float v) { return abs(v) > PST_FLT_MAX; }
bvec2 pst_isinf(vec2 v) { return greaterThan(abs(v), vec2(PST_FLT_MAX)); }
bvec3 pst_isinf(vec3 v) { return greaterThan(abs(v), vec3(PST_FLT_MAX)); }
bvec4 pst_isinf(vec4 v) { return greaterThan(abs(v), vec4(PST_FLT_MAX)); }
#define pst_texture(tex, coord) texture(tex, vec2((coord).x, 1.0 - (coord).y))
#define pst_texelFetch(tex, coord, lod) texelFetch(tex, ivec2((coord).x, textureSize(tex, lod).y - 1 - (coord).y), lod)
"#;

const DEFAULT_SHADER: &str = r#"
struct ShaderUniforms {
  iTime: f32,
  iFrame: i32,
  _pad0: vec2<f32>,
  iResolution: vec4<f32>,
  iMouse: vec4<f32>,
  iDate: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: ShaderUniforms;

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
  let uv = frag_coord.xy / u.iResolution.xy;
  return vec4<f32>(uv.x, uv.y, 0.5 + 0.5 * sin(u.iTime + frag_coord.x * 0.02), 1.0);
}
"#;

const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

// ═══════════════════════════════════════════════════════════════════════════════
// GPU uniform buffer structs — must match WGSL layouts exactly
// ═══════════════════════════════════════════════════════════════════════════════

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SinglePassUniforms {
    i_time: f32,
    i_frame: i32,
    _pad0: [f32; 2],
    i_resolution: [f32; 4],
    i_mouse: [f32; 4],
    i_date: [f32; 4],
    i_viewport_origin: [f32; 2],
    i_viewport_size: [f32; 2],
}

fn scan_shaders() -> BTreeMap<String, Vec<PathBuf>> {
    let mut map = BTreeMap::new();
    let root = Path::new("shaders");
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let folder_name = entry.file_name().to_string_lossy().to_string();
                let mut files = Vec::new();
                if let Ok(sub_entries) = fs::read_dir(entry.path()) {
                    for sub_entry in sub_entries.flatten() {
                        let p = sub_entry.path();
                        if p.is_file() {
                            if let Some(ext) = p.extension() {
                                let ext_str = ext.to_string_lossy().to_lowercase();
                                if ext_str == "wgsl"
                                    || ext_str == "glsl"
                                    || ext_str == "frag"
                                    || ext_str == "hlsl"
                                {
                                    files.push(p);
                                }
                            }
                        }
                    }
                }
                if !files.is_empty() {
                    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
                    map.insert(folder_name, files);
                }
            }
        }
    }
    map
}

fn shader_file_kind_from_path(path: &Path) -> ShaderFileKind {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "wgsl" => ShaderFileKind::Wgsl,
        "hlsl" => ShaderFileKind::Hlsl,
        "frag" => ShaderFileKind::Frag,
        _ => ShaderFileKind::Glsl,
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct MultiPassUniforms {
    i_time: f32,
    i_time_delta: f32,
    i_frame: i32,
    _pad0: f32,
    i_resolution: [f32; 4],
    i_mouse: [f32; 4],
    i_date: [f32; 4],
    i_channel_resolution: [[f32; 4]; 4],
    i_viewport_origin: [f32; 2],
    i_viewport_size: [f32; 2],
}

// ═══════════════════════════════════════════════════════════════════════════════
// Protocol types
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
#[derive(serde::Serialize)]
#[serde(tag = "type")]
enum SidecarEvent {
    WindowReady {
        width: u32,
        height: u32,
        driver: String,
        backend_type: String,
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
    ShaderUpdated {
        success: bool,
    },
    Pong,
    BackendChangeRequired {
        message: String,
    },
    ScreenshotTaken {
        path: String,
    },
}

#[allow(dead_code, non_snake_case)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarCommand {
    Ping,
    Shutdown,
    SetShader {
        source: String,
    },
    SetResolution {
        width: u32,
        height: u32,
    },
    SetBackend {
        backend: String,
    },
    SetUniforms {
        iTime: f32,
        iTimeDelta: f32,
        iResolution: [f32; 3],
        iMouse: [f32; 4],
        iFrame: u32,
        iDate: [f32; 4],
    },
    SetKeyboard {
        keys: Vec<u8>,
    },
    TakeScreenshot,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Multi-pass parser — mirrors src/renderer/shaderParser.ts
// ═══════════════════════════════════════════════════════════════════════════════

struct ParsedPass {
    name: String,
    source: String,
    channels: [Option<String>; 4],
}

fn pass_requires_multi_pipeline(pass: &ParsedPass) -> bool {
    pass.channels.iter().any(Option::is_some)
        || [
            "iChannelResolution",
            "iChannel0",
            "iChannel1",
            "iChannel2",
            "iChannel3",
            "iTimeDelta",
        ]
        .iter()
        .any(|needle| pass.source.contains(needle))
}

fn parse_passes(source: &str) -> Vec<ParsedPass> {
    let has_markers = source.lines().any(|l| {
        let t = l.trim();
        t.starts_with("//! PASS:") || t.starts_with("//!PASS:")
    });

    if !has_markers {
        return vec![ParsedPass {
            name: "Image".into(),
            source: source.to_string(),
            channels: [None, None, None, None],
        }];
    }

    let mut passes: Vec<ParsedPass> = Vec::new();
    let mut current: Option<ParsedPass> = None;
    let mut src_lines: Vec<&str> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        if let Some(name) = trimmed
            .strip_prefix("//! PASS:")
            .or_else(|| trimmed.strip_prefix("//!PASS:"))
        {
            if let Some(mut pass) = current.take() {
                pass.source = src_lines.join("\n").trim().to_string();
                passes.push(pass);
                src_lines.clear();
            }
            current = Some(ParsedPass {
                name: name.trim().to_string(),
                source: String::new(),
                channels: [None, None, None, None],
            });
            continue;
        }

        if let Some(ref mut cur) = current {
            if let Some(rest) = trimmed.strip_prefix("//! iChannel") {
                if let Some(digit) = rest.chars().next().and_then(|c| c.to_digit(10)) {
                    if digit <= 3 {
                        if let Some(val) = rest.get(2..).map(|s| s.trim_start_matches(':').trim()) {
                            cur.channels[digit as usize] =
                                Some(if val.eq_ignore_ascii_case("self") {
                                    cur.name.clone()
                                } else {
                                    val.to_string()
                                });
                            continue;
                        }
                    }
                }
            }
        }

        src_lines.push(line);
    }

    if let Some(mut pass) = current {
        pass.source = src_lines.join("\n").trim().to_string();
        passes.push(pass);
    }

    if !passes.iter().any(|p| p.name.eq_ignore_ascii_case("image")) {
        if let Some(last) = passes.last_mut() {
            last.name = "Image".into();
        }
    }

    passes
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ping-pong offscreen render target
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
struct PingPongTarget {
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    read_index: usize,
    width: u32,
    height: u32,
}

#[allow(dead_code)]
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
            textures: [t0, t1],
            views: [v0, v1],
            read_index: 0,
            width: width.max(1),
            height: height.max(1),
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

    fn resize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> bool {
        let w = width.max(1);
        let h = height.max(1);
        if w == self.width && h == self.height {
            return false;
        }
        *self = Self::new(device, queue, w, h);
        true
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Renderer state
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
struct CompiledPass {
    name: String,
    pipeline: wgpu::RenderPipeline,
    channels: [Option<String>; 4],
    is_image: bool,
    uniform_buf: wgpu::Buffer,
}

#[allow(dead_code)]
enum ShaderMode {
    None,
    Single(wgpu::RenderPipeline, wgpu::BindGroup),
    Multi(Vec<CompiledPass>, HashMap<String, PingPongTarget>, bool),
}

#[allow(dead_code)]
struct RendererState {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    // Single-pass resources
    single_uniform_buf: wgpu::Buffer,
    single_bgl: wgpu::BindGroupLayout,
    single_pl: wgpu::PipelineLayout,

    // Multi-pass resources
    multi_uniform_buf: wgpu::Buffer,
    multi_bgl: wgpu::BindGroupLayout,
    multi_pl: wgpu::PipelineLayout,
    linear_sampler: wgpu::Sampler,
    dummy_view: wgpu::TextureView,
    _dummy_texture: wgpu::Texture,
    keyboard_texture: wgpu::Texture,
    keyboard_view: wgpu::TextureView,

    mode: ShaderMode,

    /// None when the backend lacks timestamp queries (e.g. most GL).
    gpu_timer: Option<gpu_timer::GpuTimer>,

    egui_ctx: egui::Context,
    egui_state: EguiWinitState,
    egui_renderer: EguiRenderer,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main application
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
struct PreviewApp {
    workspace_mode: WorkspaceMode,
    create_view: CreateShaderView,
    settings: render_settings::RenderSettings,
    shader_source: String,
    shader_language: ShaderInputLanguage,
    loaded_shader_name: String,
    loaded_shader_path: Option<PathBuf>,
    active_backend_name: String,
    active_adapter_name: String,
    active_driver_name: String,
    commands: Receiver<SidecarCommand>,
    renderer: Option<RendererState>,
    window: Option<Arc<Window>>,
    should_exit: bool,
    last_stats_at: Instant,
    start_time: Instant,
    frame_counter: u64,
    total_frames: u64,
    last_frame_time: f64,
    keyboard_data: Vec<u8>,
    mouse_pos: [f32; 4],
    mouse_down: bool,
    mouse_click_origin: [f32; 2],
    last_cursor_points: Option<[f32; 2]>,
    preview_focused: bool,
    screenshot_requested: bool,
    diagnostics: Vec<DiagnosticEntry>,
    show_diagnostics: bool,

    // File browser
    shader_files: BTreeMap<String, Vec<PathBuf>>,
    shader_watch_rx: Receiver<()>,
    shader_watcher: Option<RecommendedWatcher>,
    selected_folder: Option<String>,
    display_aspect: DisplayAspectPreset,
    render_scale: RenderScalePreset,
    preview_pixel_size: [u32; 2],
    preview_size_dirty: bool,
    pending_surface_size: Option<PhysicalSize<u32>>,
    last_resize_request_at: Option<Instant>,
    viewport_rect: egui::Rect,
    temporal_reset_pending: bool,
    multipass_diag_enabled: bool,
    multipass_diag_pending: bool,
    editor_document: Option<EditorDocument>,
    new_shader_form: NewShaderFormState,
    pending_editor_action: Option<PendingEditorAction>,
    compile_update_tx: Sender<CompileUpdate>,
    compile_update_rx: Receiver<CompileUpdate>,
    active_compile: Option<ActiveCompile>,
    next_compile_job_id: u64,
    /// Renderer rebuild requested by UI or benchmark; applied on the next
    /// RedrawRequested before rendering, since we need `&ActiveEventLoop`.
    pending_rebuild: Option<render_settings::RenderSettings>,
    /// Adapter names (surface-compatible) discovered for the current backend.
    available_adapters: Vec<String>,
    /// Present modes the current surface supports.
    supported_present_modes: Vec<wgpu::PresentMode>,
    /// Present mode actually in use after fallback resolution.
    active_present_mode: wgpu::PresentMode,
    /// Set when present mode / frame latency changed and the surface needs reconfigure.
    surface_reconfigure_needed: bool,
    /// (pass name, milliseconds) for the most recent pipeline build.
    pipeline_compile_ms: Vec<(String, f64)>,
    /// Rolling CPU frame-time history (ms), capped at STATS_HISTORY frames.
    cpu_frame_history: std::collections::VecDeque<f64>,
    /// Rolling GPU frame-time history (ms) from GpuTimer, same cap.
    gpu_frame_history: std::collections::VecDeque<f64>,
    last_frame_at: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq)]
enum ShaderInputLanguage {
    Wgsl,
    Glsl,
    Hlsl,
}

impl ShaderInputLanguage {
    fn label(&self) -> &'static str {
        match self {
            Self::Wgsl => "WGSL",
            Self::Glsl => "GLSL",
            Self::Hlsl => "HLSL",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceMode {
    LoadShaders,
    CreateShader,
}

impl WorkspaceMode {
    fn label(self) -> &'static str {
        match self {
            Self::LoadShaders => "Load Shaders",
            Self::CreateShader => "Create Shader",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreateShaderView {
    Landing,
    NewShader,
    OpenExisting,
    Editor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShaderFileKind {
    Wgsl,
    Glsl,
    Frag,
    Hlsl,
}

impl ShaderFileKind {
    fn label(self) -> &'static str {
        match self {
            Self::Wgsl => ".wgsl",
            Self::Glsl => ".glsl",
            Self::Frag => ".frag",
            Self::Hlsl => ".hlsl",
        }
    }

    fn compile_language(self) -> ShaderInputLanguage {
        match self {
            Self::Wgsl => ShaderInputLanguage::Wgsl,
            Self::Glsl | Self::Frag => ShaderInputLanguage::Glsl,
            Self::Hlsl => ShaderInputLanguage::Hlsl,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Wgsl => "wgsl",
            Self::Glsl => "glsl",
            Self::Frag => "frag",
            Self::Hlsl => "hlsl",
        }
    }

    fn starter_template(self) -> &'static str {
        match self {
            Self::Wgsl => {
                "fn mainImage(fragCoord: vec2<f32>) -> vec4<f32> {\n  let uv = fragCoord / u.iResolution.xy;\n  return vec4<f32>(uv, 0.5 + 0.5 * sin(u.iTime), 1.0);\n}\n"
            }
            Self::Glsl | Self::Frag => {
                "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n    vec2 uv = fragCoord / iResolution.xy;\n    fragColor = vec4(uv, 0.5 + 0.5 * sin(iTime), 1.0);\n}\n"
            }
            Self::Hlsl => {
                "// HLSL source files can be created and saved here.\n// Native compile support is not implemented yet.\n\nfloat4 mainImage(float2 fragCoord) : SV_Target0\n{\n    return float4(0.1, 0.1, 0.1, 1.0);\n}\n"
            }
        }
    }
}

#[derive(Clone)]
struct EditorDocument {
    path: PathBuf,
    display_name: String,
    kind: ShaderFileKind,
    buffer: String,
    dirty: bool,
}

struct NewShaderFormState {
    name: String,
    kind: ShaderFileKind,
}

#[derive(Clone)]
enum PendingEditorAction {
    SwitchWorkspace(WorkspaceMode),
    SetCreateView(CreateShaderView),
    OpenEditorFile(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayAspectPreset {
    Auto,
    Widescreen16x9,
    Classic4x3,
    Square1x1,
}

impl DisplayAspectPreset {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Widescreen16x9 => "16:9",
            Self::Classic4x3 => "4:3",
            Self::Square1x1 => "1:1",
        }
    }

    fn resolve_ratio(self, available_width: f32, available_height: f32) -> f32 {
        match self {
            Self::Auto => (available_width / available_height.max(1.0)).max(0.01),
            Self::Widescreen16x9 => 16.0 / 9.0,
            Self::Classic4x3 => 4.0 / 3.0,
            Self::Square1x1 => 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderScalePreset {
    Half,
    ThreeQuarter,
    Full,
}

impl RenderScalePreset {
    fn label(self) -> &'static str {
        match self {
            Self::Half => "0.5x",
            Self::ThreeQuarter => "0.75x",
            Self::Full => "1.0x",
        }
    }

    fn factor(self) -> f32 {
        match self {
            Self::Half => 0.5,
            Self::ThreeQuarter => 0.75,
            Self::Full => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PreviewGeometry {
    rect: egui::Rect,
    pixel_size: [u32; 2],
}

fn compute_preview_geometry(
    available_rect: egui::Rect,
    available_height: f32,
    display_aspect: DisplayAspectPreset,
    render_scale: RenderScalePreset,
    pixels_per_point: f32,
) -> PreviewGeometry {
    let graphic_height = (available_height - PREVIEW_LOG_HEIGHT - PREVIEW_LAYOUT_GUTTER).max(100.0);
    let aspect_ratio = display_aspect.resolve_ratio(available_rect.width(), graphic_height);
    let mut preview_width = available_rect.width().max(100.0);
    let mut preview_height = (preview_width / aspect_ratio).max(100.0);
    if preview_height > graphic_height {
        preview_height = graphic_height.max(100.0);
        preview_width = (preview_height * aspect_ratio).max(100.0);
    }

    let rect = egui::Rect::from_min_size(
        egui::pos2(
            available_rect.min.x + ((available_rect.width() - preview_width).max(0.0) * 0.5),
            available_rect.min.y,
        ),
        egui::vec2(preview_width, preview_height),
    );
    let scale = pixels_per_point * render_scale.factor();
    let pixel_size = [
        (rect.width() * scale).round().max(1.0) as u32,
        (rect.height() * scale).round().max(1.0) as u32,
    ];

    PreviewGeometry { rect, pixel_size }
}

#[derive(Clone, Copy)]
enum CompileTrigger {
    Startup,
    ShaderLoad,
    External,
    EditorCompile,
    #[allow(dead_code)]
    Resize,
}

impl CompileTrigger {
    fn should_log_success(self) -> bool {
        matches!(
            self,
            Self::Startup | Self::ShaderLoad | Self::External | Self::EditorCompile
        )
    }
}

fn keycode_to_shadertoy_index(key: KeyCode) -> Option<u8> {
    Some(match key {
        KeyCode::KeyA => b'A',
        KeyCode::KeyB => b'B',
        KeyCode::KeyC => b'C',
        KeyCode::KeyD => b'D',
        KeyCode::KeyE => b'E',
        KeyCode::KeyF => b'F',
        KeyCode::KeyG => b'G',
        KeyCode::KeyH => b'H',
        KeyCode::KeyI => b'I',
        KeyCode::KeyJ => b'J',
        KeyCode::KeyK => b'K',
        KeyCode::KeyL => b'L',
        KeyCode::KeyM => b'M',
        KeyCode::KeyN => b'N',
        KeyCode::KeyO => b'O',
        KeyCode::KeyP => b'P',
        KeyCode::KeyQ => b'Q',
        KeyCode::KeyR => b'R',
        KeyCode::KeyS => b'S',
        KeyCode::KeyT => b'T',
        KeyCode::KeyU => b'U',
        KeyCode::KeyV => b'V',
        KeyCode::KeyW => b'W',
        KeyCode::KeyX => b'X',
        KeyCode::KeyY => b'Y',
        KeyCode::KeyZ => b'Z',
        KeyCode::Digit0 => b'0',
        KeyCode::Digit1 => b'1',
        KeyCode::Digit2 => b'2',
        KeyCode::Digit3 => b'3',
        KeyCode::Digit4 => b'4',
        KeyCode::Digit5 => b'5',
        KeyCode::Digit6 => b'6',
        KeyCode::Digit7 => b'7',
        KeyCode::Digit8 => b'8',
        KeyCode::Digit9 => b'9',
        KeyCode::Space => 32,
        KeyCode::Enter => 13,
        KeyCode::Tab => 9,
        KeyCode::Backspace => 8,
        KeyCode::Escape => 27,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => 16,
        KeyCode::ControlLeft | KeyCode::ControlRight => 17,
        KeyCode::AltLeft | KeyCode::AltRight => 18,
        KeyCode::ArrowLeft => 37,
        KeyCode::ArrowUp => 38,
        KeyCode::ArrowRight => 39,
        KeyCode::ArrowDown => 40,
        KeyCode::Insert => 45,
        KeyCode::Delete => 46,
        KeyCode::Home => 36,
        KeyCode::End => 35,
        KeyCode::PageUp => 33,
        KeyCode::PageDown => 34,
        _ => return None,
    })
}

#[derive(Clone)]
struct DiagnosticEntry {
    level: DiagLevel,
    message: String,
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum DiagLevel {
    Info,
    Warning,
    Error,
    Success,
}

struct PreparedShader {
    passes: Vec<ParsedPass>,
    is_multi: bool,
    pass_count: usize,
}

enum CompileUpdate {
    Progress {
        job_id: u64,
        stage: String,
    },
    Finished {
        job_id: u64,
        result: Result<PreparedShader, String>,
    },
}

struct ActiveCompile {
    job_id: u64,
    trigger: CompileTrigger,
    started_at: Instant,
    stage: String,
}

impl PreviewApp {
    fn new(
        initial_settings: render_settings::RenderSettings,
        commands: Receiver<SidecarCommand>,
    ) -> Self {
        let (shader_watch_tx, shader_watch_rx) = mpsc::channel();
        let (compile_update_tx, compile_update_rx) = mpsc::channel();
        let multipass_diag_enabled = std::env::var_os("PST_MULTIPASS_DIAG").is_some();
        let shader_watcher = {
            let watcher_result: Result<RecommendedWatcher, notify::Error> =
                notify::recommended_watcher(move |_event: Result<NotifyEvent, notify::Error>| {
                    let _ = shader_watch_tx.send(());
                });
            match watcher_result {
                Ok(mut watcher) => {
                    if watcher
                        .watch(Path::new("shaders"), RecursiveMode::Recursive)
                        .is_ok()
                    {
                        Some(watcher)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        };

        Self {
            workspace_mode: WorkspaceMode::LoadShaders,
            create_view: CreateShaderView::Landing,
            settings: initial_settings,
            shader_source: DEFAULT_SHADER.to_string(),
            shader_language: ShaderInputLanguage::Wgsl,
            loaded_shader_name: "Built-in default".to_string(),
            loaded_shader_path: None,
            active_backend_name: "Initializing".to_string(),
            active_adapter_name: String::new(),
            active_driver_name: String::new(),
            commands,
            renderer: None,
            window: None,
            should_exit: false,
            last_stats_at: Instant::now(),
            start_time: Instant::now(),
            frame_counter: 0,
            total_frames: 0,
            last_frame_time: 0.0,
            keyboard_data: vec![0u8; 256 * 3 * 4],
            mouse_pos: [0.0; 4],
            mouse_down: false,
            mouse_click_origin: [0.0, 0.0],
            last_cursor_points: None,
            preview_focused: false,
            screenshot_requested: false,
            diagnostics: Vec::new(),
            show_diagnostics: true,
            shader_files: scan_shaders(),
            shader_watch_rx,
            shader_watcher,
            selected_folder: None,
            display_aspect: DisplayAspectPreset::Auto,
            render_scale: RenderScalePreset::Full,
            preview_pixel_size: [1280, 720],
            preview_size_dirty: true,
            pending_surface_size: None,
            last_resize_request_at: None,
            viewport_rect: egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(1280.0, 720.0),
            ),
            temporal_reset_pending: true,
            multipass_diag_enabled,
            multipass_diag_pending: multipass_diag_enabled,
            editor_document: None,
            new_shader_form: NewShaderFormState {
                name: String::new(),
                kind: ShaderFileKind::Wgsl,
            },
            pending_editor_action: None,
            compile_update_tx,
            compile_update_rx,
            active_compile: None,
            next_compile_job_id: 0,
            pending_rebuild: None,
            available_adapters: Vec::new(),
            supported_present_modes: Vec::new(),
            active_present_mode: wgpu::PresentMode::Fifo,
            surface_reconfigure_needed: false,
            pipeline_compile_ms: Vec::new(),
            cpu_frame_history: std::collections::VecDeque::new(),
            gpu_frame_history: std::collections::VecDeque::new(),
            last_frame_at: None,
        }
    }

    fn refresh_shader_library(&mut self) {
        self.shader_files = scan_shaders();
    }

    fn editor_is_dirty(&self) -> bool {
        self.editor_document
            .as_ref()
            .map(|document| document.dirty)
            .unwrap_or(false)
    }

    fn request_editor_action(&mut self, action: PendingEditorAction) {
        if self.editor_is_dirty() {
            self.pending_editor_action = Some(action);
        } else {
            self.apply_editor_action(action);
        }
    }

    fn apply_editor_action(&mut self, action: PendingEditorAction) {
        match action {
            PendingEditorAction::SwitchWorkspace(mode) => {
                self.pending_editor_action = None;
                self.editor_document = None;
                self.create_view = CreateShaderView::Landing;
                self.workspace_mode = mode;
            }
            PendingEditorAction::SetCreateView(view) => {
                self.pending_editor_action = None;
                if view != CreateShaderView::Editor {
                    self.editor_document = None;
                }
                self.workspace_mode = WorkspaceMode::CreateShader;
                self.create_view = view;
            }
            PendingEditorAction::OpenEditorFile(path) => {
                match self.open_existing_shader_in_editor(&path) {
                    Ok(()) => {
                        self.pending_editor_action = None;
                    }
                    Err(error) => {
                        self.push_diagnostic(DiagLevel::Error, error.clone());
                        emit(&SidecarEvent::Diagnostic {
                            level: "error",
                            message: error,
                        });
                    }
                }
            }
        }
    }

    fn save_editor_document(&mut self) -> Result<(), String> {
        let Some(document) = self.editor_document.as_mut() else {
            return Ok(());
        };

        if let Some(parent) = document.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&document.path, &document.buffer).map_err(|e| e.to_string())?;
        document.dirty = false;
        self.refresh_shader_library();
        Ok(())
    }

    fn open_existing_shader_in_editor(&mut self, path: &Path) -> Result<(), String> {
        let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let kind = shader_file_kind_from_path(path);
        self.editor_document = Some(EditorDocument {
            path: path.to_path_buf(),
            display_name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            kind,
            buffer: source,
            dirty: false,
        });
        self.workspace_mode = WorkspaceMode::CreateShader;
        self.create_view = CreateShaderView::Editor;
        self.push_diagnostic(
            DiagLevel::Info,
            format!(
                "Loaded '{}' into Create Shader. Press Compile to preview changes.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
        );
        Ok(())
    }

    fn create_new_shader_document(&mut self) -> Result<(), String> {
        let mut base_name = self.new_shader_form.name.trim().to_string();
        if base_name.is_empty() {
            return Err("Shader name is required.".to_string());
        }

        for ch in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
            base_name = base_name.replace(ch, "_");
        }

        let kind = self.new_shader_form.kind;
        let desired_suffix = format!(".{}", kind.extension());
        if !base_name.to_ascii_lowercase().ends_with(&desired_suffix) {
            base_name.push_str(&desired_suffix);
        }

        let user_dir = Path::new("shaders").join("User");
        fs::create_dir_all(&user_dir).map_err(|e| e.to_string())?;
        let path = user_dir.join(base_name);
        if path.exists() {
            return Err(format!(
                "A shader named '{}' already exists.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }

        let template = kind.starter_template().to_string();
        fs::write(&path, &template).map_err(|e| e.to_string())?;
        self.refresh_shader_library();
        self.editor_document = Some(EditorDocument {
            path: path.clone(),
            display_name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            kind,
            buffer: template,
            dirty: false,
        });
        self.workspace_mode = WorkspaceMode::CreateShader;
        self.create_view = CreateShaderView::Editor;
        self.new_shader_form.name.clear();
        Ok(())
    }

    fn load_shader_for_preview(
        &mut self,
        path: &Path,
        trigger: CompileTrigger,
    ) -> Result<(), String> {
        let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let kind = shader_file_kind_from_path(path);
        let path_says_shadertoy = path.components().any(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("Shadertoy")
        });
        // A multipass shader using `//! PASS: Buffer A/B/C/D` + `Image` is, in
        // practice, always a Shadertoy port — their compositions are authored
        // against Shadertoy's canonical 16:9 canvas. If we render them at the
        // window's native (often ultrawide or square-ish) aspect, the camera's
        // local_y scales by `res.y/res.x` and the subject drifts vertically.
        // Forcing 16:9 when the panel is on Auto keeps the framing identical
        // to the reference, regardless of where the file lives on disk.
        let looks_like_multipass_port = source
            .lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("//! PASS:") || t.starts_with("//!PASS:")
            })
            .count()
            >= 2;
        let is_shadertoy_shader = path_says_shadertoy || looks_like_multipass_port;
        self.shader_source = source;
        self.shader_language = kind.compile_language();
        self.loaded_shader_path = Some(path.to_path_buf());
        self.loaded_shader_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if is_shadertoy_shader && self.display_aspect == DisplayAspectPreset::Auto {
            self.display_aspect = DisplayAspectPreset::Widescreen16x9;
        }
        self.push_diagnostic(
            DiagLevel::Info,
            format!(
                "Loaded shader '{}' [{}]",
                self.loaded_shader_name,
                self.shader_language.label()
            ),
        );
        self.compile_and_rebuild(trigger);
        Ok(())
    }

    fn init_renderer(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.renderer.is_some() {
            return Ok(());
        }

        // Reuse the existing window when we are rebuilding the renderer (e.g.
        // after a runtime backend swap). Only create a new window on the very
        // first init from `resumed`.
        let window = if let Some(existing) = &self.window {
            existing.clone()
        } else {
            Arc::new(
                event_loop
                    .create_window(
                        WindowAttributes::default()
                            .with_title("PersonalShaderToy Native Preview")
                            .with_min_inner_size(PhysicalSize::new(
                                MIN_WINDOW_WIDTH,
                                MIN_WINDOW_HEIGHT,
                            ))
                            .with_inner_size(PhysicalSize::new(
                                DEFAULT_WINDOW_WIDTH,
                                DEFAULT_WINDOW_HEIGHT,
                            )),
                    )
                    .map_err(|e| e.to_string())?,
            )
        };

        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        let requested_backends = self.settings.backend.to_wgpu();
        instance_desc.backends = requested_backends;
        instance_desc.backend_options.dx12.shader_compiler = self.settings.dx12_compiler.to_wgpu();
        let instance = wgpu::Instance::new(instance_desc);

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let mut adapters = pollster::block_on(instance.enumerate_adapters(requested_backends));
        self.available_adapters = adapters
            .iter()
            .filter(|adapter| adapter.is_surface_supported(&surface))
            .map(|adapter| adapter.get_info().name)
            .collect();
        self.available_adapters.dedup();

        // Explicit adapter request by name takes priority over backend order.
        let named_index = self.settings.adapter_name.as_ref().and_then(|wanted| {
            adapters.iter().position(|adapter| {
                adapter.is_surface_supported(&surface) && adapter.get_info().name == *wanted
            })
        });
        if self.settings.adapter_name.is_some() && named_index.is_none() {
            self.push_diagnostic(
                DiagLevel::Warning,
                format!(
                    "Adapter '{}' not found on this backend; using automatic selection.",
                    self.settings.adapter_name.as_deref().unwrap_or_default()
                ),
            );
        }
        let preferred_order = self.settings.backend.preferred_backend_order();
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

        self.supported_present_modes = capabilities.present_modes.clone();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: self
                .settings
                .resolve_present_mode(&capabilities.present_modes),
            desired_maximum_frame_latency: self.settings.frame_latency,
            alpha_mode,
            view_formats: vec![],
        };
        self.active_present_mode = config.present_mode;
        surface.configure(&device, &config);

        // ── Single-pass layout (1 uniform buffer) ──

        let single_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("single-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let single_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("single-pl"),
            bind_group_layouts: &[Some(&single_bgl)],
            immediate_size: 0,
        });

        let single_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("single-uniform-buf"),
            size: std::mem::size_of::<SinglePassUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ── Multi-pass layout (uniform + 4 textures + sampler) ──

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

        let multi_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("multi-uniform-buf"),
            size: std::mem::size_of::<MultiPassUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        emit(&SidecarEvent::WindowReady {
            width: size.width,
            height: size.height,
            driver: adapter_info.driver.clone(),
            backend_type: format!("{:?}", adapter_info.backend),
        });

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

        let frame_gpu_timer = gpu_timer::GpuTimer::new(&device, &queue);
        if frame_gpu_timer.is_none() {
            self.push_diagnostic(
                DiagLevel::Info,
                "GPU timestamps unavailable on this backend (CPU timing only).".to_string(),
            );
        }

        self.active_backend_name = format!("{:?}", adapter_info.backend);
        self.active_adapter_name = adapter_info.name.clone();
        self.active_driver_name = adapter_info.driver.clone();
        self.window = Some(window);
        self.renderer = Some(RendererState {
            _instance: instance,
            surface,
            _adapter: adapter,
            device,
            queue,
            config,
            single_uniform_buf,
            single_bgl,
            single_pl,
            multi_uniform_buf,
            multi_bgl,
            multi_pl,
            linear_sampler,
            _dummy_texture: dummy_texture,
            dummy_view,
            keyboard_texture,
            keyboard_view,
            mode: ShaderMode::None,
            gpu_timer: frame_gpu_timer,
            egui_ctx,
            egui_state,
            egui_renderer,
        });

        self.push_diagnostic(
            DiagLevel::Info,
            format!(
                "Renderer ready: {} on {}{}",
                self.active_backend_name,
                self.active_adapter_name,
                if self.active_driver_name.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", self.active_driver_name)
                }
            ),
        );
        self.rebuild_shader(CompileTrigger::Startup);
        Ok(())
    }

    fn upload_keyboard_texture(&self) {
        if let Some(r) = &self.renderer {
            r.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &r.keyboard_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.keyboard_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256 * 4),
                    rows_per_image: Some(3),
                },
                wgpu::Extent3d {
                    width: 256,
                    height: 3,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn set_key_state(&mut self, key: u8, pressed: bool) {
        let index = key as usize * 4;
        self.keyboard_data[index] = if pressed { 255 } else { 0 };
        self.upload_keyboard_texture();
    }

    fn map_cursor_to_shader(
        &self,
        x_points: f32,
        y_points: f32,
        clamp_to_viewport: bool,
    ) -> Option<[f32; 2]> {
        let rect = self.viewport_rect;
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }

        let inside = rect.contains(egui::pos2(x_points, y_points));
        if !inside && !clamp_to_viewport {
            return None;
        }

        let local_x = if clamp_to_viewport {
            (x_points - rect.min.x).clamp(0.0, rect.width())
        } else {
            x_points - rect.min.x
        };
        let local_y = if clamp_to_viewport {
            (y_points - rect.min.y).clamp(0.0, rect.height())
        } else {
            y_points - rect.min.y
        };

        let norm_x = local_x / rect.width().max(1.0);
        let norm_y = 1.0 - (local_y / rect.height().max(1.0));
        Some([
            norm_x * self.preview_pixel_size[0] as f32,
            norm_y * self.preview_pixel_size[1] as f32,
        ])
    }

    fn sync_mouse_click_state(&mut self) {
        self.mouse_pos[2] = if self.mouse_down {
            self.mouse_click_origin[0]
        } else {
            -self.mouse_click_origin[0]
        };
        self.mouse_pos[3] = if self.mouse_down {
            self.mouse_click_origin[1]
        } else {
            -self.mouse_click_origin[1]
        };
    }

    fn cursor_in_viewport(&self, x_points: f32, y_points: f32) -> bool {
        self.viewport_rect.contains(egui::pos2(x_points, y_points))
    }

    // ── Command processing ──

    fn process_commands(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            match cmd {
                SidecarCommand::Ping => emit(&SidecarEvent::Pong),
                SidecarCommand::Shutdown => self.should_exit = true,

                SidecarCommand::SetShader { source } => {
                    self.shader_source = source;
                    self.loaded_shader_name = "External shader".to_string();
                    self.push_diagnostic(
                        DiagLevel::Info,
                        format!("Loaded shader source into {}", self.loaded_shader_name),
                    );
                    self.rebuild_shader(CompileTrigger::External);
                }

                SidecarCommand::SetResolution { width, height } => {
                    if let Some(w) = &self.window {
                        let _ =
                            w.request_inner_size(PhysicalSize::new(width.max(1), height.max(1)));
                    }
                }

                SidecarCommand::SetBackend { backend } => {
                    emit(&SidecarEvent::Diagnostic {
                        level: "error",
                        message: format!(
                            "Backend change to '{backend}' requires restarting the sidecar."
                        ),
                    });
                }

                #[allow(non_snake_case)]
                SidecarCommand::SetUniforms {
                    iTime,
                    iTimeDelta,
                    iResolution,
                    iMouse,
                    iFrame,
                    iDate,
                } => {
                    self.mouse_pos = iMouse;
                    if let Some(r) = &self.renderer {
                        match &r.mode {
                            ShaderMode::Single { .. } => {
                                let u = SinglePassUniforms {
                                    i_time: iTime,
                                    i_frame: iFrame as i32,
                                    _pad0: [0.0; 2],
                                    i_resolution: [
                                        iResolution[0],
                                        iResolution[1],
                                        iResolution[2],
                                        0.0,
                                    ],
                                    i_mouse: iMouse,
                                    i_date: iDate,
                                    i_viewport_origin: [0.0, 0.0],
                                    i_viewport_size: [iResolution[0], iResolution[1]],
                                };
                                r.queue.write_buffer(
                                    &r.single_uniform_buf,
                                    0,
                                    bytemuck::bytes_of(&u),
                                );
                            }
                            ShaderMode::Multi(passes, ..) => {
                                let mut chan_res = [[0.0f32; 4]; 4];
                                for cr in &mut chan_res {
                                    *cr = [iResolution[0], iResolution[1], iResolution[2], 0.0];
                                }
                                let u = MultiPassUniforms {
                                    i_time: iTime,
                                    i_time_delta: iTimeDelta,
                                    i_frame: iFrame as i32,
                                    _pad0: 0.0,
                                    i_resolution: [
                                        iResolution[0],
                                        iResolution[1],
                                        iResolution[2],
                                        0.0,
                                    ],
                                    i_mouse: iMouse,
                                    i_date: iDate,
                                    i_channel_resolution: chan_res,
                                    i_viewport_origin: [0.0, 0.0],
                                    i_viewport_size: [iResolution[0], iResolution[1]],
                                };
                                r.queue.write_buffer(
                                    &r.multi_uniform_buf,
                                    0,
                                    bytemuck::bytes_of(&u),
                                );
                                for pass in passes {
                                    r.queue.write_buffer(
                                        &pass.uniform_buf,
                                        0,
                                        bytemuck::bytes_of(&u),
                                    );
                                }
                            }
                            ShaderMode::None => {}
                        }
                    }
                }

                SidecarCommand::SetKeyboard { keys } => {
                    for i in 0..256 {
                        self.keyboard_data[i * 4] = if keys.contains(&(i as u8)) { 255 } else { 0 };
                    }
                    if let Some(r) = &self.renderer {
                        r.queue.write_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &r.keyboard_texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            &self.keyboard_data,
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(256 * 4),
                                rows_per_image: Some(3),
                            },
                            wgpu::Extent3d {
                                width: 256,
                                height: 3,
                                depth_or_array_layers: 1,
                            },
                        );
                    }
                }

                SidecarCommand::TakeScreenshot => {
                    self.screenshot_requested = true;
                }
            }
        }
    }

    // ── Shader compilation ──

    fn push_diagnostic(&mut self, level: DiagLevel, message: String) {
        self.diagnostics.push(DiagnosticEntry { level, message });
        if self.diagnostics.len() > 100 {
            self.diagnostics.remove(0);
        }
    }

    fn process_compile_updates(&mut self) {
        while let Ok(update) = self.compile_update_rx.try_recv() {
            match update {
                CompileUpdate::Progress { job_id, stage } => {
                    if let Some(active) = self.active_compile.as_mut()
                        && active.job_id == job_id
                    {
                        active.stage = stage;
                    }
                }
                CompileUpdate::Finished { job_id, result } => {
                    if self.active_compile.as_ref().map(|active| active.job_id) != Some(job_id) {
                        continue;
                    }

                    let active = self.active_compile.take().unwrap();
                    let elapsed_ms = active.started_at.elapsed().as_millis();
                    self.apply_prepared_shader(result, active.trigger, elapsed_ms);
                }
            }
        }
    }

    fn apply_prepared_shader(
        &mut self,
        result: Result<PreparedShader, String>,
        trigger: CompileTrigger,
        elapsed_ms: u128,
    ) {
        let prepared = match result {
            Ok(prepared) => prepared,
            Err(error) => {
                let err_msg = format!(
                    "Shader preparation failed for '{}' after {} ms:\n{}",
                    self.loaded_shader_name, elapsed_ms, error
                );
                self.push_diagnostic(DiagLevel::Error, err_msg.clone());
                emit(&SidecarEvent::Diagnostic {
                    level: "error",
                    message: err_msg,
                });
                emit(&SidecarEvent::ShaderUpdated { success: false });
                return;
            }
        };

        let pass_count = prepared.pass_count;
        let build_result = if prepared.is_multi {
            self.build_multi_pass(prepared.passes)
        } else {
            self.build_single_pass(&prepared.passes[0].source)
        };

        match build_result {
            Ok(()) => {
                emit(&SidecarEvent::ShaderUpdated { success: true });
                if trigger.should_log_success() {
                    let pass_label = if pass_count == 1 { "pass" } else { "passes" };
                    self.push_diagnostic(
                        DiagLevel::Success,
                        format!(
                            "Compiled '{}' in {} ms ({} {}, {}, {})",
                            self.loaded_shader_name,
                            elapsed_ms,
                            pass_count,
                            pass_label,
                            self.shader_language.label(),
                            self.active_backend_name
                        ),
                    );
                }
            }
            Err(error) => {
                let err_msg = format!(
                    "Compile failed for '{}' after {} ms: {}",
                    self.loaded_shader_name, elapsed_ms, error
                );
                self.push_diagnostic(DiagLevel::Error, err_msg.clone());
                emit(&SidecarEvent::Diagnostic {
                    level: "error",
                    message: err_msg,
                });
                emit(&SidecarEvent::ShaderUpdated { success: false });
            }
        }
    }

    fn compile_and_rebuild(&mut self, trigger: CompileTrigger) {
        if self.renderer.is_none() {
            return;
        }

        self.next_compile_job_id += 1;
        let job_id = self.next_compile_job_id;
        let source = self.shader_source.clone();
        let shader_language = self.shader_language;
        let compile_update_tx = self.compile_update_tx.clone();
        let panic_update_tx = compile_update_tx.clone();

        self.active_compile = Some(ActiveCompile {
            job_id,
            trigger,
            started_at: Instant::now(),
            stage: "Parsing shader".to_string(),
        });

        let spawn_result = std::thread::Builder::new()
            .name(format!("shader-compile-{job_id}"))
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    run_compile_job(job_id, source, shader_language, compile_update_tx)
                });
                if result.is_err() {
                    let _ = panic_update_tx.send(CompileUpdate::Finished {
                        job_id,
                        result: Err("Shader compile worker panicked while preparing the shader."
                            .to_string()),
                    });
                }
            });

        if let Err(error) = spawn_result {
            self.active_compile = None;
            let err_msg = format!("Failed to start shader compile worker: {error}");
            self.push_diagnostic(DiagLevel::Error, err_msg.clone());
            emit(&SidecarEvent::Diagnostic {
                level: "error",
                message: err_msg,
            });
            emit(&SidecarEvent::ShaderUpdated { success: false });
        }
    }

    fn rebuild_shader(&mut self, trigger: CompileTrigger) {
        self.compile_and_rebuild(trigger);
    }

    fn build_single_pass(&mut self, source: &str) -> Result<(), String> {
        let Some(r) = self.renderer.as_mut() else {
            return Ok(());
        };

        let pipeline_start = Instant::now();
        match create_pipeline(&r.device, r.config.format, &r.single_pl, source) {
            Ok(pipeline) => {
                let compile_ms = pipeline_start.elapsed().as_secs_f64() * 1000.0;
                let bind_group = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("single-bg"),
                    layout: &r.single_bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: r.single_uniform_buf.as_entire_binding(),
                    }],
                });
                r.mode = ShaderMode::Single(pipeline, bind_group);
                self.pipeline_compile_ms = vec![("Image".to_string(), compile_ms)];
                self.push_diagnostic(
                    DiagLevel::Info,
                    format!("Pipeline built in {compile_ms:.1} ms."),
                );
                self.total_frames = 0;
                self.temporal_reset_pending = true;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn build_multi_pass(&mut self, parsed: Vec<ParsedPass>) -> Result<(), String> {
        let Some(r) = self.renderer.as_mut() else {
            return Ok(());
        };

        let mut targets = HashMap::new();
        let mut pass_pipelines = Vec::new();

        for buffer_name in ["Buffer A", "Buffer B", "Buffer C", "Buffer D"] {
            targets.insert(
                buffer_name.to_string(),
                PingPongTarget::new(
                    &r.device,
                    &r.queue,
                    self.preview_pixel_size[0],
                    self.preview_pixel_size[1],
                ),
            );
        }

        let mut compile_times: Vec<(String, f64)> = Vec::new();
        for p in parsed {
            let is_image = p.name.to_lowercase() == "image";
            // Buffer passes render to OFFSCREEN_FORMAT (Rgba16Float) targets,
            // only the Image pass renders to the surface (Bgra8UnormSrgb etc.).
            let target_format = if is_image {
                r.config.format
            } else {
                OFFSCREEN_FORMAT
            };
            let pipeline_start = Instant::now();
            match create_pipeline(&r.device, target_format, &r.multi_pl, &p.source) {
                Ok(pipeline) => {
                    compile_times.push((
                        p.name.clone(),
                        pipeline_start.elapsed().as_secs_f64() * 1000.0,
                    ));
                    let uniform_label = format!("multi-pass-uniform-{}", p.name);
                    let uniform_buf = r.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(uniform_label.as_str()),
                        size: std::mem::size_of::<MultiPassUniforms>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    pass_pipelines.push(CompiledPass {
                        name: p.name.clone(),
                        pipeline,
                        channels: p.channels,
                        is_image,
                        uniform_buf,
                    });
                }
                Err(e) => {
                    return Err(format!("Pass '{}' failed: {}", p.name, e));
                }
            }
        }

        r.mode = ShaderMode::Multi(pass_pipelines, targets, false);
        let total_ms: f64 = compile_times.iter().map(|(_, ms)| ms).sum();
        let detail = compile_times
            .iter()
            .map(|(name, ms)| format!("{name}: {ms:.1}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.push_diagnostic(
            DiagLevel::Info,
            format!("Pipelines built in {total_ms:.1} ms ({detail})."),
        );
        self.pipeline_compile_ms = compile_times;
        self.total_frames = 0;
        self.temporal_reset_pending = true;
        self.multipass_diag_pending = self.multipass_diag_enabled;
        Ok(())
    }

    fn queue_resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.preview_size_dirty = true;
            self.pending_surface_size = Some(new_size);
            self.last_resize_request_at = Some(Instant::now());
        }
    }

    fn apply_queued_resize(&mut self) {
        let Some(new_size) = self.pending_surface_size.take() else {
            return;
        };
        self.last_resize_request_at = None;

        if let Some(r) = self.renderer.as_mut() {
            r.config.width = new_size.width;
            r.config.height = new_size.height;
            r.surface.configure(&r.device, &r.config);
            // Ping-pong textures are resized synchronously each frame in
            // render() — no async pipeline rebuild needed just for a resize.
        }
    }

    fn render(&mut self) {
        // CPU frame time = wall-clock delta between render() entries.
        let frame_now = Instant::now();
        if let Some(prev) = self.last_frame_at {
            let cpu_ms = frame_now.duration_since(prev).as_secs_f64() * 1000.0;
            self.cpu_frame_history.push_back(cpu_ms);
            while self.cpu_frame_history.len() > STATS_HISTORY {
                self.cpu_frame_history.pop_front();
            }
        }
        self.last_frame_at = Some(frame_now);

        self.process_compile_updates();

        // Cheap settings application: present mode and frame latency only need
        // a surface reconfigure, not a renderer rebuild.
        if self.surface_reconfigure_needed {
            let mut reconfigure_note = None;
            if let Some(r) = self.renderer.as_mut() {
                r.config.present_mode = self
                    .settings
                    .resolve_present_mode(&self.supported_present_modes);
                r.config.desired_maximum_frame_latency = self.settings.frame_latency;
                r.surface.configure(&r.device, &r.config);
                self.active_present_mode = r.config.present_mode;
                reconfigure_note = Some(format!(
                    "Surface reconfigured: {:?}, latency {}.",
                    r.config.present_mode, r.config.desired_maximum_frame_latency
                ));
            }
            if let Some(note) = reconfigure_note {
                self.push_diagnostic(DiagLevel::Info, note);
            }
            self.surface_reconfigure_needed = false;
        }

        let mut requested_editor_action: Option<PendingEditorAction> = None;
        let mut switch_to_create_mode = false;
        let mut preview_load_path: Option<PathBuf> = None;
        let mut editor_open_path: Option<PathBuf> = None;
        let mut create_new_submit = false;
        let mut save_editor = false;
        let mut compile_editor = false;
        let mut dialog_choice: Option<&'static str> = None;
        let mut display_aspect = self.display_aspect;
        let mut render_scale = self.render_scale;
        let previous_display_aspect = self.display_aspect;
        let previous_render_scale = self.render_scale;
        let mut ui_settings = self.settings.clone();
        let previous_settings = self.settings.clone();
        let available_adapters = self.available_adapters.clone();
        let supported_present_modes = self.supported_present_modes.clone();
        let active_present_mode = self.active_present_mode;
        let mut preview_pixel_size = self.preview_pixel_size;
        let _previous_preview_pixel_size = self.preview_pixel_size;
        let mut viewport_rect = self.viewport_rect;
        let mut multipass_diag_to_emit: Option<String> = None;
        let mut screenshot_taken_path: Option<String> = None;
        let mut screenshot_error_to_emit: Option<String> = None;
        let mut new_shader_name = self.new_shader_form.name.clone();
        let mut new_shader_kind = self.new_shader_form.kind;
        let mut editor_buffer = self.editor_document.as_ref().map(|doc| doc.buffer.clone());
        let mut editor_buffer_changed = false;
        let workspace_mode = self.workspace_mode;
        let create_view = self.create_view;
        let loaded_shader_name = self.loaded_shader_name.clone();
        let loaded_shader_language = self.shader_language;
        let active_backend_name = self.active_backend_name.clone();
        let active_adapter_name = self.active_adapter_name.clone();

        let Some(r) = self.renderer.as_mut() else {
            return;
        };
        // Harvest any completed GPU timestamp readbacks before encoding the
        // new frame so `latest()` reflects the freshest finished frame.
        if let Some(t) = r.gpu_timer.as_mut() {
            for total_ms in t.begin_frame() {
                self.gpu_frame_history.push_back(total_ms);
                while self.gpu_frame_history.len() > STATS_HISTORY {
                    self.gpu_frame_history.pop_front();
                }
            }
        }
        let gpu_latest_timing = r
            .gpu_timer
            .as_ref()
            .and_then(|t| t.latest())
            .cloned();
        let ppp = r.egui_ctx.pixels_per_point();

        let raw_input = r.egui_state.take_egui_input(self.window.as_ref().unwrap());
        r.egui_ctx.begin_pass(raw_input);

        #[allow(deprecated)]
        egui::TopBottomPanel::top("mode_switch")
            .resizable(false)
            .show(&r.egui_ctx, |ui| {
                ui.horizontal(|ui| {
                    for mode in [WorkspaceMode::LoadShaders, WorkspaceMode::CreateShader] {
                        let selected = workspace_mode == mode;
                        if ui.selectable_label(selected, mode.label()).clicked() && !selected {
                            match mode {
                                WorkspaceMode::LoadShaders => {
                                    requested_editor_action =
                                        Some(PendingEditorAction::SwitchWorkspace(
                                            WorkspaceMode::LoadShaders,
                                        ));
                                }
                                WorkspaceMode::CreateShader => {
                                    switch_to_create_mode = true;
                                }
                            }
                        }
                    }

                    // "Preview: WxH" label removed — resolution already shown in the toolbar.
                });
            });

        let render_shader_picker =
            |ui: &mut egui::Ui, title: &str, target: &mut Option<PathBuf>| {
                ui.add_space(8.0);
                ui.heading(title);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut folders: Vec<_> = self.shader_files.keys().cloned().collect();
                    folders.sort();
                    for folder in folders {
                        ui.collapsing(&folder, |ui| {
                            let mut files = self.shader_files.get(&folder).unwrap().clone();
                            files.sort();
                            for file in files {
                                let name = file.file_name().unwrap().to_string_lossy().to_string();
                                if ui.selectable_label(false, &name).clicked() {
                                    *target = Some(file.clone());
                                }
                            }
                        });
                    }
                });
            };

        let mut render_preview_workspace = |ui: &mut egui::Ui| {
            let cpu_samples: Vec<f64> = self.cpu_frame_history.iter().copied().collect();
            let cpu_stats = bench::FrameStats::from_samples_ms(&cpu_samples);
            let gpu_samples: Vec<f64> = self.gpu_frame_history.iter().copied().collect();
            let gpu_stats = bench::FrameStats::from_samples_ms(&gpu_samples);
            let runtime_label = format_runtime(self.start_time.elapsed());

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("📊 Stats").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let stats_text = match &cpu_stats {
                        Some(s) => format!(
                            "FPS: {:.0} | avg {:.2}ms p95 {:.2} p99 {:.2} | 1% low {:.0} fps | {}",
                            s.avg_fps(),
                            s.avg_ms,
                            s.p95_ms,
                            s.p99_ms,
                            s.low_1pct_fps(),
                            runtime_label,
                        ),
                        None => format!("Runtime: {runtime_label}"),
                    };
                    ui.label(
                        egui::RichText::new(stats_text)
                            .size(13.5)
                            .color(egui::Color32::from_rgb(145, 205, 145)),
                    );
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!("Renderer: {}", active_backend_name))
                        .size(12.5)
                        .color(egui::Color32::from_rgb(155, 175, 235)),
                );
                if !active_adapter_name.is_empty() {
                    ui.label(
                        egui::RichText::new(active_adapter_name.as_str())
                            .size(12.5)
                            .color(egui::Color32::from_rgb(155, 175, 235)),
                    );
                }
                let gpu_text = match &gpu_stats {
                    Some(s) => format!("GPU: {:.2}ms avg, p95 {:.2}", s.avg_ms, s.p95_ms),
                    None => "GPU: n/a".to_string(),
                };
                ui.label(
                    egui::RichText::new(gpu_text)
                        .size(12.5)
                        .color(egui::Color32::from_rgb(205, 175, 145)),
                );
                if let Some(timing) = &gpu_latest_timing {
                    if timing.pass_ms.len() > 1 {
                        let breakdown = timing
                            .pass_ms
                            .iter()
                            .enumerate()
                            .map(|(i, ms)| format!("p{i}: {ms:.2}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        ui.label(
                            egui::RichText::new(breakdown)
                                .size(11.0)
                                .color(egui::Color32::from_rgb(170, 150, 130)),
                        );
                    }
                }
            });
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "{} [{}]",
                    loaded_shader_name,
                    loaded_shader_language.label()
                ))
                .size(13.0)
                .color(egui::Color32::from_rgb(220, 220, 220)),
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), PREVIEW_STATUS_ROW_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if let Some(active_compile) = &self.active_compile {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(
                            egui::RichText::new(format!(
                                "Compiling: {} | {:.1}s",
                                active_compile.stage,
                                active_compile.started_at.elapsed().as_secs_f32()
                            ))
                            .size(12.0)
                            .color(egui::Color32::from_rgb(255, 210, 120)),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Ready")
                                .size(12.0)
                                .color(egui::Color32::from_rgb(120, 165, 120)),
                        );
                    }
                },
            );

            let geometry = compute_preview_geometry(
                ui.available_rect_before_wrap(),
                ui.available_height(),
                display_aspect,
                render_scale,
                ppp,
            );
            let graphic_rect = geometry.rect;
            ui.allocate_rect(graphic_rect, egui::Sense::click_and_drag());
            viewport_rect = graphic_rect;
            preview_pixel_size = geometry.pixel_size;

            ui.separator();

            // Bottom row: Logs (75%) on the left, Settings (25%) on the right.
            // Putting the dropdowns down here keeps their popups well clear of
            // the shader viewport.
            let bottom_total_w = ui.available_width();
            let gap = 8.0;
            let logs_w = ((bottom_total_w - gap) * 0.75).max(120.0);
            let settings_w = (bottom_total_w - gap - logs_w).max(180.0);
            let bottom_h = PREVIEW_LOG_HEIGHT;

            ui.horizontal_top(|ui| {
                // ── Logs (left, 75%) ───────────────────────────────────────
                ui.allocate_ui_with_layout(
                    egui::vec2(logs_w, bottom_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.label(egui::RichText::new("📋 Logs").strong());
                        egui::ScrollArea::vertical()
                            .id_salt("logs_scroll")
                            .max_height(bottom_h - 24.0)
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                if self.diagnostics.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No entries.")
                                            .italics()
                                            .color(egui::Color32::GRAY),
                                    );
                                }
                                for entry in &self.diagnostics {
                                    let (icon, color) = match entry.level {
                                        DiagLevel::Error => {
                                            ("✖", egui::Color32::from_rgb(255, 90, 90))
                                        }
                                        DiagLevel::Warning => {
                                            ("⚠", egui::Color32::from_rgb(255, 200, 80))
                                        }
                                        DiagLevel::Info => {
                                            ("ℹ", egui::Color32::from_rgb(140, 180, 255))
                                        }
                                        DiagLevel::Success => {
                                            ("✔", egui::Color32::from_rgb(90, 220, 120))
                                        }
                                    };
                                    ui.label(
                                        egui::RichText::new(format!("{icon} {}", entry.message))
                                            .color(color)
                                            .size(11.0)
                                            .font(egui::FontId::monospace(11.0)),
                                    );
                                }
                            });
                    },
                );

                ui.separator();

                // ── Settings (right, 25%) ──────────────────────────────────
                ui.allocate_ui_with_layout(
                    egui::vec2(settings_w, bottom_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.label(egui::RichText::new("⚙ Settings").strong());
                        egui::ScrollArea::vertical()
                            .id_salt("settings_scroll")
                            .max_height(bottom_h - 24.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 6.0;
                                let label_color = egui::Color32::from_rgb(190, 190, 190);

                                ui.label(
                                    egui::RichText::new("Aspect").size(12.0).color(label_color),
                                );
                                egui::ComboBox::from_id_salt("display_aspect")
                                    .width(settings_w - 16.0)
                                    .selected_text(display_aspect.label())
                                    .show_ui(ui, |ui| {
                                        for preset in [
                                            DisplayAspectPreset::Auto,
                                            DisplayAspectPreset::Widescreen16x9,
                                            DisplayAspectPreset::Classic4x3,
                                            DisplayAspectPreset::Square1x1,
                                        ] {
                                            ui.selectable_value(
                                                &mut display_aspect,
                                                preset,
                                                preset.label(),
                                            );
                                        }
                                    });

                                ui.label(
                                    egui::RichText::new("Scale").size(12.0).color(label_color),
                                );
                                egui::ComboBox::from_id_salt("render_scale")
                                    .width(settings_w - 16.0)
                                    .selected_text(render_scale.label())
                                    .show_ui(ui, |ui| {
                                        for preset in [
                                            RenderScalePreset::Half,
                                            RenderScalePreset::ThreeQuarter,
                                            RenderScalePreset::Full,
                                        ] {
                                            ui.selectable_value(
                                                &mut render_scale,
                                                preset,
                                                preset.label(),
                                            );
                                        }
                                    });

                                ui.label(
                                    egui::RichText::new("Backend").size(12.0).color(label_color),
                                );
                                egui::ComboBox::from_id_salt("backend_choice")
                                    .width(settings_w - 16.0)
                                    .selected_text(ui_settings.backend.label())
                                    .show_ui(ui, |ui| {
                                        for choice in BackendChoice::ui_choices() {
                                            ui.selectable_value(
                                                &mut ui_settings.backend,
                                                *choice,
                                                choice.label(),
                                            );
                                        }
                                    });

                                ui.label(
                                    egui::RichText::new("GPU").size(12.0).color(label_color),
                                );
                                let adapter_text = ui_settings
                                    .adapter_name
                                    .clone()
                                    .unwrap_or_else(|| "Auto".to_string());
                                egui::ComboBox::from_id_salt("adapter_choice")
                                    .width(settings_w - 16.0)
                                    .selected_text(adapter_text)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut ui_settings.adapter_name,
                                            None,
                                            "Auto",
                                        );
                                        for name in &available_adapters {
                                            ui.selectable_value(
                                                &mut ui_settings.adapter_name,
                                                Some(name.clone()),
                                                name,
                                            );
                                        }
                                    });

                                ui.label(
                                    egui::RichText::new("Present").size(12.0).color(label_color),
                                );
                                egui::ComboBox::from_id_salt("present_mode_choice")
                                    .width(settings_w - 16.0)
                                    .selected_text(ui_settings.present_mode.label())
                                    .show_ui(ui, |ui| {
                                        for choice in
                                            render_settings::PresentModeChoice::ui_choices()
                                        {
                                            let supported = match choice.to_wgpu() {
                                                None => true,
                                                Some(mode) => {
                                                    supported_present_modes.contains(&mode)
                                                }
                                            };
                                            ui.add_enabled_ui(supported, |ui| {
                                                let text = if supported {
                                                    choice.label().to_string()
                                                } else {
                                                    format!("{} (unsupported)", choice.label())
                                                };
                                                ui.selectable_value(
                                                    &mut ui_settings.present_mode,
                                                    *choice,
                                                    text,
                                                );
                                            });
                                        }
                                    });

                                ui.label(
                                    egui::RichText::new("Latency").size(12.0).color(label_color),
                                );
                                egui::ComboBox::from_id_salt("frame_latency_choice")
                                    .width(settings_w - 16.0)
                                    .selected_text(format!(
                                        "{} frames",
                                        ui_settings.frame_latency
                                    ))
                                    .show_ui(ui, |ui| {
                                        for latency in [1u32, 2, 3] {
                                            ui.selectable_value(
                                                &mut ui_settings.frame_latency,
                                                latency,
                                                format!("{latency} frames"),
                                            );
                                        }
                                    });

                                if active_backend_name == "Dx12" {
                                    ui.label(
                                        egui::RichText::new("DX12 compiler")
                                            .size(12.0)
                                            .color(label_color),
                                    );
                                    egui::ComboBox::from_id_salt("dx12_compiler_choice")
                                        .width(settings_w - 16.0)
                                        .selected_text(ui_settings.dx12_compiler.label())
                                        .show_ui(ui, |ui| {
                                            for choice in
                                                render_settings::DxCompilerChoice::ui_choices()
                                            {
                                                ui.selectable_value(
                                                    &mut ui_settings.dx12_compiler,
                                                    *choice,
                                                    choice.label(),
                                                );
                                            }
                                        });
                                }

                                ui.label(
                                    egui::RichText::new(
                                        "backend/GPU/compiler rebuild the renderer",
                                    )
                                    .size(10.5)
                                    .italics()
                                    .color(egui::Color32::from_rgb(140, 140, 140)),
                                );

                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Resolution: {}×{}",
                                        preview_pixel_size[0], preview_pixel_size[1]
                                    ))
                                    .size(11.5)
                                    .color(label_color),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Active: {:?} · latency {}",
                                        active_present_mode, ui_settings.frame_latency
                                    ))
                                    .size(11.5)
                                    .color(label_color),
                                );
                            });
                    },
                );
            });
        };

        match workspace_mode {
            WorkspaceMode::LoadShaders => {
                #[allow(deprecated)]
                egui::SidePanel::left("load_shader_browser")
                    .resizable(true)
                    .default_width(
                        (r.config.width as f32 * DEFAULT_SHADER_LIST_RATIO)
                            .clamp(MIN_SHADER_LIST_WIDTH, MAX_SHADER_LIST_WIDTH),
                    )
                    .min_width(MIN_SHADER_LIST_WIDTH)
                    .max_width(MAX_SHADER_LIST_WIDTH)
                    .show(&r.egui_ctx, |ui| {
                        render_shader_picker(ui, "📁 Shaders", &mut preview_load_path);
                    });

                #[allow(deprecated)]
                egui::CentralPanel::default().show(&r.egui_ctx, |ui| {
                    render_preview_workspace(ui);
                });
            }
            WorkspaceMode::CreateShader => {
                #[allow(deprecated)]
                egui::SidePanel::right("create_preview_panel")
                    .resizable(true)
                    .default_width(
                        (r.config.width as f32 * DEFAULT_PREVIEW_PANEL_RATIO)
                            .clamp(MIN_PREVIEW_PANEL_WIDTH, MAX_PREVIEW_PANEL_WIDTH),
                    )
                    .min_width(MIN_PREVIEW_PANEL_WIDTH)
                    .max_width(MAX_PREVIEW_PANEL_WIDTH)
                    .show(&r.egui_ctx, |ui| {
                        render_preview_workspace(ui);
                    });

                #[allow(deprecated)]
                egui::CentralPanel::default().show(&r.egui_ctx, |ui| {
                    ui.heading("Create Shader");
                    ui.label(
                        egui::RichText::new(
                            "Author here intentionally. Nothing recompiles until you press Compile.",
                        )
                        .size(12.0)
                        .color(egui::Color32::from_rgb(180, 180, 180)),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("New Shader").clicked() {
                            requested_editor_action =
                                Some(PendingEditorAction::SetCreateView(CreateShaderView::NewShader));
                        }
                        if ui.button("Load Existing Shader").clicked() {
                            requested_editor_action = Some(PendingEditorAction::SetCreateView(
                                CreateShaderView::OpenExisting,
                            ));
                        }
                        if ui.button("Back to Load").clicked() {
                            requested_editor_action = Some(PendingEditorAction::SwitchWorkspace(
                                WorkspaceMode::LoadShaders,
                            ));
                        }
                    });
                    ui.separator();

                    match create_view {
                        CreateShaderView::Landing => {
                            ui.add_space(12.0);
                            ui.heading("Choose a starting point");
                            ui.label(
                                "Create a new shader file in shaders/User, or intentionally load an existing shader into the editor.",
                            );
                            ui.add_space(8.0);
                            if ui
                                .add_sized([220.0, 44.0], egui::Button::new("New Shader"))
                                .clicked()
                            {
                                requested_editor_action = Some(PendingEditorAction::SetCreateView(
                                    CreateShaderView::NewShader,
                                ));
                            }
                            if ui
                                .add_sized(
                                    [220.0, 44.0],
                                    egui::Button::new("Load Existing Shader"),
                                )
                                .clicked()
                            {
                                requested_editor_action = Some(PendingEditorAction::SetCreateView(
                                    CreateShaderView::OpenExisting,
                                ));
                            }
                        }
                        CreateShaderView::NewShader => {
                            ui.label("Create a real file first, then edit it.");
                            ui.add_space(8.0);
                            ui.label("Shader name");
                            ui.text_edit_singleline(&mut new_shader_name);
                            ui.add_space(6.0);
                            ui.label("Shader language / extension");
                            egui::ComboBox::from_id_salt("new_shader_kind")
                                .selected_text(new_shader_kind.label())
                                .show_ui(ui, |ui| {
                                    for kind in [
                                        ShaderFileKind::Wgsl,
                                        ShaderFileKind::Glsl,
                                        ShaderFileKind::Frag,
                                        ShaderFileKind::Hlsl,
                                    ] {
                                        ui.selectable_value(&mut new_shader_kind, kind, kind.label());
                                    }
                                });
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Create File").clicked() {
                                    create_new_submit = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    requested_editor_action = Some(PendingEditorAction::SetCreateView(
                                        CreateShaderView::Landing,
                                    ));
                                }
                            });
                        }
                        CreateShaderView::OpenExisting => {
                            render_shader_picker(
                                ui,
                                "Open Existing Shader",
                                &mut editor_open_path,
                            );
                        }
                        CreateShaderView::Editor => {
                            if let Some(document) = &self.editor_document {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("File: {}", document.display_name))
                                            .strong(),
                                    );
                                    ui.label(format!("Language: {}", document.kind.label()));
                                    ui.label(if document.dirty {
                                        "Status: Unsaved changes"
                                    } else {
                                        "Status: Saved"
                                    });
                                });
                                ui.label(
                                    egui::RichText::new(document.path.display().to_string())
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(160, 160, 160)),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Compile").clicked() {
                                        compile_editor = true;
                                    }
                                    if ui
                                        .add_enabled(document.dirty, egui::Button::new("Save"))
                                        .clicked()
                                    {
                                        save_editor = true;
                                    }
                                });
                                ui.separator();
                                if let Some(buffer) = editor_buffer.as_mut() {
                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                        if ui
                                            .add(
                                                egui::TextEdit::multiline(buffer)
                                                    .font(egui::TextStyle::Monospace)
                                                    .desired_rows(40)
                                                    .lock_focus(true)
                                                    .desired_width(f32::INFINITY),
                                            )
                                            .changed()
                                        {
                                            editor_buffer_changed = true;
                                        }
                                    });
                                }
                            } else {
                                ui.label("No shader is currently open in the editor.");
                            }
                        }
                    }
                });
            }
        }

        if self.pending_editor_action.is_some() {
            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&r.egui_ctx, |ui| {
                    ui.label("You have unsaved changes in Create Shader.");
                    ui.label("Save before continuing?");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            dialog_choice = Some("save");
                        }
                        if ui.button("Discard").clicked() {
                            dialog_choice = Some("discard");
                        }
                        if ui.button("Cancel").clicked() {
                            dialog_choice = Some("cancel");
                        }
                    });
                });
        }

        let full_output = r.egui_ctx.end_pass();
        r.egui_state
            .handle_platform_output(self.window.as_ref().unwrap(), full_output.platform_output);

        // Collect popup-layer rects (Foreground/Tooltip/Debug). These are
        // ComboBox menus, tooltips, etc. — anything that should paint on top
        // of the shader. We split the tessellated output so that the shader
        // pass is rendered between the background layers and these foreground
        // popups, instead of overdrawing them.
        let popup_rects: Vec<egui::Rect> = r.egui_ctx.memory(|mem| {
            mem.areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|lid| lid.order >= egui::Order::Foreground)
                .filter_map(|lid| mem.area_rect(lid.id))
                .collect()
        });

        // Partition shapes by whether their clip_rect lies inside one of the
        // foreground area rects (with a small tolerance for sub-pixel rounding).
        // Panel/window clip rects are large and won't be contained in popup rects.
        let (popup_shapes, base_shapes): (Vec<_>, Vec<_>) =
            full_output.shapes.into_iter().partition(|cs| {
                if popup_rects.is_empty() {
                    return false;
                }
                let cr = cs.clip_rect;
                popup_rects.iter().any(|pr| {
                    pr.min.x - 0.5 <= cr.min.x
                        && pr.min.y - 0.5 <= cr.min.y
                        && cr.max.x <= pr.max.x + 0.5
                        && cr.max.y <= pr.max.y + 0.5
                })
            });

        let ppp_for_tess = r.egui_ctx.pixels_per_point();
        let base_paint_jobs = r.egui_ctx.tessellate(base_shapes, ppp_for_tess);
        let popup_paint_jobs: Vec<_> = if popup_shapes.is_empty() {
            Vec::new()
        } else {
            r.egui_ctx.tessellate(popup_shapes, ppp_for_tess)
        };

        {
            let ppp = r.egui_ctx.pixels_per_point();
            let elapsed = self.start_time.elapsed().as_secs_f32();
            let now = chrono_date_vec();

            let surface_w = r.config.width as f32;
            let surface_h = r.config.height as f32;
            let raw_vp_x = viewport_rect.min.x * ppp;
            let raw_vp_y = viewport_rect.min.y * ppp;
            let raw_vp_w = viewport_rect.width() * ppp;
            let raw_vp_h = viewport_rect.height() * ppp;

            let vp_x = raw_vp_x.clamp(0.0, surface_w.max(1.0));
            let vp_y = raw_vp_y.clamp(0.0, surface_h.max(1.0));
            let vp_w = raw_vp_w.max(0.0).min((surface_w - vp_x).max(0.0));
            let vp_h = raw_vp_h.max(0.0).min((surface_h - vp_y).max(0.0));
            let has_visible_viewport = vp_w >= 1.0 && vp_h >= 1.0;
            let scissor_x = vp_x.floor() as u32;
            let scissor_y = vp_y.floor() as u32;
            let scissor_w = vp_w.ceil().max(1.0) as u32;
            let scissor_h = vp_h.ceil().max(1.0) as u32;
            // Use THIS frame's locally-computed preview_pixel_size, not the
            // stale self.preview_pixel_size (which is only committed at the
            // end of render()). On window maximize, self.preview_pixel_size
            // lags by one frame, causing iResolution to disagree with the
            // ping-pong texture sizes that the previous frame sized itself
            // around. That disagreement is the other half of the duplicate-
            // black-hole bug.
            let logical_w = preview_pixel_size[0] as f32;
            let logical_h = preview_pixel_size[1] as f32;

            // Synchronously resize ping-pong targets if they disagree with
            // the current preview size. Without this, a window maximize
            // triggers an ASYNC shader rebuild (hundreds of ms), and every
            // frame in that window renders into OLD-sized textures using
            // NEW-sized iResolution — the shader's UV space only covers a
            // partial region, leaving the old frame's content visible in
            // the un-rendered region. That stale content is the "ghost"
            // duplicate of the black hole the user saw on maximize.
            // Any target that actually re-allocated triggers a shader-frame
            // counter reset below, so TAA self-feedback paths read their own
            // freshly-cleared texture instead of blending with whatever was
            // left over from the previous size.
            let mut any_target_resized = false;
            if let ShaderMode::Multi(_, targets, _) = &mut r.mode {
                for target in targets.values_mut() {
                    if target.resize(
                        &r.device,
                        &r.queue,
                        preview_pixel_size[0],
                        preview_pixel_size[1],
                    ) {
                        any_target_resized = true;
                    }
                }
            }
            let control_geometry_changed =
                display_aspect != previous_display_aspect || render_scale != previous_render_scale;
            let reset_temporal_state =
                self.temporal_reset_pending || any_target_resized || control_geometry_changed;
            if reset_temporal_state {
                // Reset total_frames so temporal/self-feedback shaders skip
                // one blend frame after size, aspect, scale, or shader changes.
                // Kerr-Newman stores camera/TAA data in the ping-pong buffers;
                // letting old dimensions survive a geometry change is what
                // turns a resize into stale bloom/history streaks.
                self.total_frames = 0;
                self.temporal_reset_pending = false;
                if self.multipass_diag_enabled {
                    self.multipass_diag_pending = true;
                }
            }
            let shader_frame = self.total_frames as i32;

            if self.multipass_diag_enabled && self.multipass_diag_pending {
                if let ShaderMode::Multi(passes, targets, ..) = &r.mode {
                    multipass_diag_to_emit = Some(format_multipass_diagnostic(
                        r.config.width,
                        r.config.height,
                        [vp_x, vp_y, vp_w, vp_h],
                        preview_pixel_size,
                        [logical_w, logical_h, logical_w / logical_h.max(1.0), 0.0],
                        passes,
                        targets,
                        reset_temporal_state,
                        shader_frame,
                    ));
                    self.multipass_diag_pending = false;
                }
            }

            match &r.mode {
                ShaderMode::Single { .. } => {
                    let u = SinglePassUniforms {
                        i_time: elapsed,
                        i_frame: shader_frame,
                        _pad0: [0.0; 2],
                        i_resolution: [logical_w, logical_h, logical_w / logical_h.max(1.0), 0.0],
                        i_mouse: self.mouse_pos,
                        i_date: now,
                        i_viewport_origin: [vp_x, vp_y],
                        i_viewport_size: [vp_w.max(1.0), vp_h.max(1.0)],
                    };
                    r.queue
                        .write_buffer(&r.single_uniform_buf, 0, bytemuck::bytes_of(&u));
                }
                _ => {}
            }

            let frame = match r.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) => f,
                wgpu::CurrentSurfaceTexture::Outdated => {
                    r.surface.configure(&r.device, &r.config);
                    return;
                }
                _ => return,
            };
            let view = frame.texture.create_view(&Default::default());
            let mut encoder = r
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("frame-encoder"),
                });

            // 1. Clear background
            {
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.01,
                                g: 0.01,
                                b: 0.02,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
            }

            // 2. Egui BASE layers (panels/windows). Popups (Foreground+) are
            // deferred to after the shader pass so dropdowns don't get clipped.
            let screen_descriptor = ScreenDescriptor {
                size_in_pixels: [r.config.width, r.config.height],
                pixels_per_point: ppp,
            };
            for (id, delta) in &full_output.textures_delta.set {
                r.egui_renderer
                    .update_texture(&r.device, &r.queue, *id, delta);
            }
            r.egui_renderer.update_buffers(
                &r.device,
                &r.queue,
                &mut encoder,
                &base_paint_jobs,
                &screen_descriptor,
            );

            {
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-base"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                r.egui_renderer.render(
                    &mut pass.forget_lifetime(),
                    &base_paint_jobs,
                    &screen_descriptor,
                );
            }

            // 3. Shader pass LAST (renders on top of egui in the viewport rect)
            let mut swap_names: Vec<String> = Vec::new();
            let mut rendered_buffer_names: Vec<String> = Vec::new();
            if has_visible_viewport {
                match &r.mode {
                    ShaderMode::Single(pipeline, bind_group) => {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("shader-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            timestamp_writes: r
                                .gpu_timer
                                .as_mut()
                                .and_then(|t| t.pass_timestamp_writes()),
                            ..Default::default()
                        });
                        pass.set_viewport(vp_x, vp_y, vp_w, vp_h, 0.0, 1.0);
                        pass.set_scissor_rect(scissor_x, scissor_y, scissor_w, scissor_h);
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    ShaderMode::Multi(passes, targets, ..) => {
                        for cp in passes.iter() {
                            let color_view = if cp.is_image {
                                &view
                            } else {
                                targets.get(&cp.name).unwrap().write_view()
                            };
                            let load_op = if cp.is_image {
                                wgpu::LoadOp::Load
                            } else {
                                wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                            };

                            let mut chan_res = [[logical_w, logical_h, 1.0, 0.0]; 4];

                            // Resolve per-pass channel bindings from the pass's channel config
                            let ch_views: [&wgpu::TextureView; 4] =
                                std::array::from_fn(|i| match &cp.channels[i] {
                                    Some(name) if name.eq_ignore_ascii_case("keyboard") => {
                                        chan_res[i] = [256.0, 3.0, 1.0, 0.0];
                                        &r.keyboard_view
                                    }
                                    Some(name) => {
                                        if let Some(target) = targets.get(name) {
                                            chan_res[i] = [
                                                target.width as f32,
                                                target.height as f32,
                                                target.width as f32
                                                    / (target.height as f32).max(1.0),
                                                0.0,
                                            ];

                                            // Shadertoy passes should see freshly rendered upstream
                                            // buffers from the current frame, but self-feedback must
                                            // continue sampling the previous frame.
                                            let use_current_frame_output = !name
                                                .eq_ignore_ascii_case(&cp.name)
                                                && rendered_buffer_names.iter().any(|rendered| {
                                                    rendered.eq_ignore_ascii_case(name)
                                                });

                                            if use_current_frame_output {
                                                target.write_view()
                                            } else {
                                                target.read_view()
                                            }
                                        } else {
                                            &r.dummy_view
                                        }
                                    }
                                    None => &r.dummy_view,
                                });

                            let pass_uniforms = MultiPassUniforms {
                                i_time: elapsed,
                                i_time_delta: self.last_frame_time as f32,
                                i_frame: shader_frame,
                                _pad0: 0.0,
                                i_resolution: [
                                    logical_w,
                                    logical_h,
                                    logical_w / logical_h.max(1.0),
                                    0.0,
                                ],
                                i_mouse: self.mouse_pos,
                                i_date: now,
                                i_channel_resolution: chan_res,
                                i_viewport_origin: if cp.is_image {
                                    [vp_x, vp_y]
                                } else {
                                    [0.0, 0.0]
                                },
                                i_viewport_size: if cp.is_image {
                                    [vp_w.max(1.0), vp_h.max(1.0)]
                                } else {
                                    [logical_w.max(1.0), logical_h.max(1.0)]
                                },
                            };
                            r.queue.write_buffer(
                                &cp.uniform_buf,
                                0,
                                bytemuck::bytes_of(&pass_uniforms),
                            );

                            let bind_group =
                                r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                    label: Some("multi-bg"),
                                    layout: &r.multi_bgl,
                                    entries: &[
                                        wgpu::BindGroupEntry {
                                            binding: 0,
                                            resource: cp.uniform_buf.as_entire_binding(),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 1,
                                            resource: wgpu::BindingResource::TextureView(
                                                ch_views[0],
                                            ),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 2,
                                            resource: wgpu::BindingResource::TextureView(
                                                ch_views[1],
                                            ),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 3,
                                            resource: wgpu::BindingResource::TextureView(
                                                ch_views[2],
                                            ),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 4,
                                            resource: wgpu::BindingResource::TextureView(
                                                ch_views[3],
                                            ),
                                        },
                                        wgpu::BindGroupEntry {
                                            binding: 5,
                                            resource: wgpu::BindingResource::Sampler(
                                                &r.linear_sampler,
                                            ),
                                        },
                                    ],
                                });

                            {
                                let mut pass =
                                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                        label: Some("multi-render-pass"),
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: color_view,
                                                depth_slice: None,
                                                resolve_target: None,
                                                ops: wgpu::Operations {
                                                    load: load_op,
                                                    store: wgpu::StoreOp::Store,
                                                },
                                            },
                                        )],
                                        timestamp_writes: r
                                            .gpu_timer
                                            .as_mut()
                                            .and_then(|t| t.pass_timestamp_writes()),
                                        ..Default::default()
                                    });
                                if cp.is_image {
                                    pass.set_viewport(vp_x, vp_y, vp_w, vp_h, 0.0, 1.0);
                                    pass.set_scissor_rect(
                                        scissor_x, scissor_y, scissor_w, scissor_h,
                                    );
                                }
                                pass.set_pipeline(&cp.pipeline);
                                pass.set_bind_group(0, &bind_group, &[]);
                                pass.draw(0..3, 0..1);
                            }

                            if !cp.is_image {
                                swap_names.push(cp.name.clone());
                                rendered_buffer_names.push(cp.name.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Submit clear + egui base + shader pass. The popup egui pass and
            // any screenshot copy go into a second encoder so popup paint can
            // overwrite shader pixels (correct z-order) and screenshots include
            // the popup overlay.
            let needs_post_encoder = !popup_paint_jobs.is_empty() || self.screenshot_requested;

            // Resolve GPU timestamps into a readback slot inside this frame's
            // command stream, then kick off the async map after submit.
            let timer_slot = r
                .gpu_timer
                .as_mut()
                .and_then(|t| t.resolve(&mut encoder));

            r.queue.submit(Some(encoder.finish()));

            if let (Some(t), Some(slot)) = (r.gpu_timer.as_mut(), timer_slot) {
                t.after_submit(slot);
            }

            // 4. Egui popup pass (Foreground/Tooltip layers) — drawn over the
            //    shader so dropdowns aren't clipped by the viewport.
            let screenshot_readback = if needs_post_encoder {
                let mut post_encoder = r
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("frame-encoder-post"),
                    });
                if !popup_paint_jobs.is_empty() {
                    r.egui_renderer.update_buffers(
                        &r.device,
                        &r.queue,
                        &mut post_encoder,
                        &popup_paint_jobs,
                        &screen_descriptor,
                    );
                    let pass = post_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("egui-popup"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    r.egui_renderer.render(
                        &mut pass.forget_lifetime(),
                        &popup_paint_jobs,
                        &screen_descriptor,
                    );
                }

                let readback = if self.screenshot_requested {
                    self.screenshot_requested = false;
                    match queue_screenshot_copy(
                        &r.device,
                        &mut post_encoder,
                        &frame.texture,
                        r.config.width,
                        r.config.height,
                        r.config.format,
                    ) {
                        Ok(rb) => Some(rb),
                        Err(error) => {
                            screenshot_error_to_emit = Some(error);
                            None
                        }
                    }
                } else {
                    None
                };

                r.queue.submit(Some(post_encoder.finish()));
                readback
            } else {
                None
            };

            for id in &full_output.textures_delta.free {
                r.egui_renderer.free_texture(id);
            }

            frame.present();

            if let Some(readback) = screenshot_readback {
                match finish_screenshot_readback(&r.device, readback) {
                    Ok(path) => {
                        screenshot_taken_path = Some(path.display().to_string());
                    }
                    Err(error) => {
                        screenshot_error_to_emit = Some(error);
                    }
                }
            }

            // Swap ping-pong targets AFTER submit so the next frame reads what was just written
            if let ShaderMode::Multi(_, targets, ..) = &mut r.mode {
                for name in &swap_names {
                    if let Some(target) = targets.get_mut(name) {
                        target.swap();
                    }
                }
            }
        }

        // Deferred actions
        self.preview_pixel_size = preview_pixel_size;
        self.preview_size_dirty = false;
        self.viewport_rect = viewport_rect;
        self.display_aspect = display_aspect;
        self.render_scale = render_scale;

        if ui_settings != previous_settings {
            if ui_settings.requires_renderer_rebuild(&previous_settings) {
                self.push_diagnostic(
                    DiagLevel::Info,
                    format!(
                        "Renderer rebuild requested: {} → {}. Rebuilding…",
                        previous_settings.backend.label(),
                        ui_settings.backend.label()
                    ),
                );
                self.pending_rebuild = Some(ui_settings);
            } else {
                // Present mode / frame latency apply with a cheap surface
                // reconfigure on the next frame — no device rebuild.
                self.settings = ui_settings;
                self.surface_reconfigure_needed = true;
            }
        }
        self.new_shader_form.name = new_shader_name;
        self.new_shader_form.kind = new_shader_kind;

        if let Some(message) = multipass_diag_to_emit {
            self.push_diagnostic(DiagLevel::Info, message.clone());
            emit(&SidecarEvent::Diagnostic {
                level: "info",
                message,
            });
        }
        if let Some(path) = screenshot_taken_path {
            self.push_diagnostic(DiagLevel::Success, format!("Screenshot saved to {path}"));
            emit(&SidecarEvent::ScreenshotTaken { path });
        }
        if let Some(error) = screenshot_error_to_emit {
            self.push_diagnostic(DiagLevel::Error, error.clone());
            emit(&SidecarEvent::Diagnostic {
                level: "error",
                message: error,
            });
        }

        if editor_buffer_changed
            && let (Some(buffer), Some(document)) = (editor_buffer, self.editor_document.as_mut())
            && document.buffer != buffer
        {
            document.buffer = buffer;
            document.dirty = true;
        }

        if let Some(choice) = dialog_choice {
            match choice {
                "save" => {
                    if let Err(error) = self.save_editor_document() {
                        self.push_diagnostic(DiagLevel::Error, error.clone());
                        emit(&SidecarEvent::Diagnostic {
                            level: "error",
                            message: error,
                        });
                    } else if let Some(action) = self.pending_editor_action.take() {
                        self.apply_editor_action(action);
                    }
                }
                "discard" => {
                    if let Some(action) = self.pending_editor_action.take() {
                        self.apply_editor_action(action);
                    }
                }
                "cancel" => {
                    self.pending_editor_action = None;
                }
                _ => {}
            }
        }

        if let Some(action) = requested_editor_action {
            self.request_editor_action(action);
        }

        if switch_to_create_mode {
            self.workspace_mode = WorkspaceMode::CreateShader;
            if self.editor_document.is_some() {
                self.create_view = CreateShaderView::Editor;
            } else {
                self.create_view = CreateShaderView::Landing;
            }
        }

        if let Some(path) = editor_open_path {
            self.request_editor_action(PendingEditorAction::OpenEditorFile(path));
        }

        if create_new_submit {
            if let Err(error) = self.create_new_shader_document() {
                self.push_diagnostic(DiagLevel::Error, error.clone());
                emit(&SidecarEvent::Diagnostic {
                    level: "error",
                    message: error,
                });
            }
        }

        if save_editor {
            if let Err(error) = self.save_editor_document() {
                self.push_diagnostic(DiagLevel::Error, error.clone());
                emit(&SidecarEvent::Diagnostic {
                    level: "error",
                    message: error,
                });
            } else {
                self.push_diagnostic(DiagLevel::Success, "Saved editor shader.".to_string());
            }
        }

        let mut _did_explicit_compile = false;
        if let Some(path) = preview_load_path {
            if let Err(error) = self.load_shader_for_preview(&path, CompileTrigger::ShaderLoad) {
                self.push_diagnostic(DiagLevel::Error, error.clone());
                emit(&SidecarEvent::Diagnostic {
                    level: "error",
                    message: error,
                });
            } else {
                _did_explicit_compile = true;
            }
        }

        if compile_editor {
            if let Some(document) = &self.editor_document {
                self.shader_source = document.buffer.clone();
                self.shader_language = document.kind.compile_language();
                self.loaded_shader_name = document.display_name.clone();
                self.loaded_shader_path = Some(document.path.clone());
                self.compile_and_rebuild(CompileTrigger::EditorCompile);
                _did_explicit_compile = true;
            }
        }

        // Resize-only: ping-pong targets are resized synchronously earlier in
        // render() — no need to trigger an async pipeline rebuild here. A full
        // rebuild is only needed when shader *source* changes, not window size.
        // (Removing the old rebuild_shader(Resize) call that caused the ghost-
        // duplicate bug: it started an async compile that took hundreds of ms,
        // during which old-sized textures were rendered with new-sized uniforms,
        // leaving stale content from the previous resolution visible.)

        if display_aspect != previous_display_aspect || render_scale != previous_render_scale {
            self.push_diagnostic(
                DiagLevel::Info,
                format!(
                    "Preview set to {} at {} ({}×{})",
                    display_aspect.label(),
                    render_scale.label(),
                    preview_pixel_size[0],
                    preview_pixel_size[1]
                ),
            );
        }

        self.total_frames += 1;
        self.frame_counter += 1;
        let elapsed = self.last_stats_at.elapsed();
        if elapsed.as_secs_f64() >= 1.0 {
            let fps = self.frame_counter as f64 / elapsed.as_secs_f64();
            self.last_frame_time = if fps > 0.0 { 1.0 / fps } else { 0.0 };

            // Explicitly use named fields for Stats variant
            emit(&SidecarEvent::Stats {
                fps,
                frame_time_ms: self.last_frame_time * 1000.0,
                frame: self.total_frames,
            });

            self.frame_counter = 0;
            self.last_stats_at = Instant::now();
        }
    }

    /// Tear down the current renderer and rebuild it with the requested
    /// settings (backend, adapter, DX12 compiler). The window is reused. The
    /// active shader source is recompiled against the new device.
    fn apply_pending_rebuild(&mut self, event_loop: &ActiveEventLoop) {
        let Some(new_settings) = self.pending_rebuild.take() else {
            return;
        };
        if new_settings == self.settings && self.renderer.is_some() {
            return;
        }

        let previous = self.settings.clone();
        self.settings = new_settings.clone();
        // Drop the old wgpu state (instance/surface/device/queue/egui_renderer).
        // The window is preserved on `self.window` so init_renderer reuses it.
        self.renderer = None;
        // Force a temporal/state reset: ping-pong textures will be recreated
        // on the new device, so any feedback history is gone anyway.
        self.temporal_reset_pending = true;
        self.preview_size_dirty = true;

        match self.init_renderer(event_loop) {
            Ok(()) => {
                self.push_diagnostic(
                    DiagLevel::Success,
                    format!(
                        "Renderer rebuilt on {} (resolved: {}).",
                        new_settings.backend.label(),
                        self.active_backend_name
                    ),
                );
                // Recompile the active shader against the new device. Pipelines,
                // bind groups, and ping-pong targets all live in the old renderer
                // we just dropped.
                self.compile_and_rebuild(CompileTrigger::ShaderLoad);
            }
            Err(error) => {
                let failed_backend = new_settings.backend.label();
                self.settings = previous;
                self.push_diagnostic(
                    DiagLevel::Error,
                    format!(
                        "Renderer rebuild on {} failed: {error}. Reverting to {}.",
                        failed_backend,
                        self.settings.backend.label()
                    ),
                );
                // Try to come back online on the previous settings.
                if let Err(retry_err) = self.init_renderer(event_loop) {
                    self.push_diagnostic(
                        DiagLevel::Error,
                        format!("Renderer recovery on previous backend also failed: {retry_err}"),
                    );
                    event_loop.exit();
                }
            }
        }
    }
}
// ═══════════════════════════════════════════════════════════════════════════════
// Event loop handler
// ═══════════════════════════════════════════════════════════════════════════════

impl ApplicationHandler for PreviewApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(e) = self.init_renderer(event_loop) {
            emit(&SidecarEvent::Diagnostic {
                level: "error",
                message: e,
            });
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _wid: WindowId, event: WindowEvent) {
        if let Some(r) = self.renderer.as_mut() {
            if let Some(w) = &self.window {
                let _ = r.egui_state.on_window_event(w, &event);
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                // Apply a queued renderer rebuild before drawing the next frame.
                // We can only do this here because `&ActiveEventLoop` is required
                // by `init_renderer`.
                if self.pending_rebuild.is_some() {
                    self.apply_pending_rebuild(event_loop);
                }
                if self.pending_surface_size.is_none() {
                    self.render();
                }
            }
            WindowEvent::Resized(size) => self.queue_resize(size),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0);
                let x_points = position.x as f32 / scale;
                let y_points = position.y as f32 / scale;
                self.last_cursor_points = Some([x_points, y_points]);
                if let Some([x, y]) = self.map_cursor_to_shader(
                    x_points,
                    y_points,
                    self.mouse_down || self.preview_focused,
                ) {
                    self.mouse_pos[0] = x;
                    self.mouse_pos[1] = y;
                    self.sync_mouse_click_state();
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let cursor_in_viewport = self
                    .last_cursor_points
                    .map(|[x, y]| self.cursor_in_viewport(x, y))
                    .unwrap_or(false);

                match state {
                    ElementState::Pressed => {
                        if cursor_in_viewport {
                            self.preview_focused = true;
                            self.mouse_down = true;
                            if let Some([x_points, y_points]) = self.last_cursor_points {
                                if let Some([x, y]) =
                                    self.map_cursor_to_shader(x_points, y_points, true)
                                {
                                    self.mouse_pos[0] = x;
                                    self.mouse_pos[1] = y;
                                }
                            }
                            self.mouse_click_origin = [self.mouse_pos[0], self.mouse_pos[1]];
                            self.sync_mouse_click_state();
                        } else {
                            self.preview_focused = false;
                        }
                    }
                    ElementState::Released => {
                        if self.mouse_down {
                            self.mouse_down = false;
                            self.sync_mouse_click_state();
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if self.preview_focused
                    && let PhysicalKey::Code(code) = event.physical_key
                {
                    if let Some(key) = keycode_to_shadertoy_index(code) {
                        self.set_key_state(key, event.state == ElementState::Pressed);
                    }
                }
            }
            WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. } => {
                // If the window loses focus (e.g. alt-tab mid-drag) winit may never
                // deliver the MouseInput Released event, so force-clear mouse_down
                // to keep iMouse.z from getting stuck in the "pressed" state.
                if self.mouse_down {
                    self.mouse_down = false;
                    self.sync_mouse_click_state();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.process_commands();
        if self.should_exit {
            event_loop.exit();
            return;
        }
        let mut shader_list_changed = false;
        while self.shader_watch_rx.try_recv().is_ok() {
            shader_list_changed = true;
        }
        if shader_list_changed {
            let fresh = scan_shaders();
            if fresh != self.shader_files {
                self.shader_files = fresh;
                if let Some(selected) = &self.selected_folder {
                    if !self.shader_files.contains_key(selected) {
                        self.selected_folder = None;
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
        if let Some(last_resize_request_at) = self.last_resize_request_at {
            if last_resize_request_at.elapsed() >= RESIZE_SETTLE_DELAY {
                self.apply_queued_resize();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            return;
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Pipeline creation
// ═══════════════════════════════════════════════════════════════════════════════

fn create_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    pipeline_layout: &wgpu::PipelineLayout,
    source: &str,
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
        return Err(errors.join("\n"));
    }

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("preview-pipeline"),
        layout: Some(pipeline_layout),
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

// ═══════════════════════════════════════════════════════════════════════════════
// Source wrapping — matching browser WebGPURenderer logic
// ═══════════════════════════════════════════════════════════════════════════════

fn wrap_single_source(source: &str) -> String {
    let has_uniform = source.contains("var<uniform> u :")
        || source.contains("var<uniform> u:")
        || source.contains("struct ShaderUniforms");
    let has_fs_main = source.contains("@fragment") && source.contains("fn fs_main");
    let has_vs_main = source.contains("@vertex") && source.contains("fn vs_main");
    let has_main_image = source.contains("fn mainImage");

    if has_uniform && has_fs_main && has_vs_main {
        return source.to_string();
    }

    let mut result = String::new();

    if !has_uniform {
        result.push_str(SINGLE_HEADER);
    }
    if !has_vs_main {
        result.push_str(VERTEX_WGSL);
    }

    result.push_str(source);

    if has_main_image && !has_fs_main {
        result.push_str(SINGLE_FOOTER);
    }

    result
}

fn wrap_multi_source(source: &str) -> String {
    let has_uniform = source.contains("var<uniform> u :")
        || source.contains("var<uniform> u:")
        || source.contains("struct MultiPassUniforms");
    let has_fs_main = source.contains("@fragment") && source.contains("fn fs_main");
    let has_vs_main = source.contains("@vertex") && source.contains("fn vs_main");
    let has_main_image = source.contains("fn mainImage");

    if has_uniform && has_fs_main && has_vs_main {
        return source.to_string();
    }

    let mut result = String::new();

    if !has_uniform {
        result.push_str(MULTI_HEADER);
    }
    if !has_vs_main {
        result.push_str(VERTEX_WGSL);
    }

    result.push_str(source);

    if has_main_image && !has_fs_main {
        result.push_str(MULTI_FOOTER);
    }

    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Pre-process Shadertoy-style GLSL for naga compatibility.
/// - Strips #version, precision, and uniform sampler2D declarations
/// - Rewrites functions that take `sampler2D` params to take split `texture2D + sampler` pairs
/// - Rewrites call sites of those functions to expand iChannelX into (t_iChannelX, iLinearSampler)
fn inline_const_array_sizes(source: &str) -> String {
    let const_re =
        Regex::new(r"\bconst\s+(?:int|uint)\s+([A-Za-z_]\w*)\s*=\s*(\d+)(?:u)?\s*;").unwrap();
    let array_re = Regex::new(r"\[\s*([A-Za-z_]\w*)\s*\]").unwrap();

    let mut scopes: Vec<HashMap<String, String>> = vec![HashMap::new()];
    let mut result = String::with_capacity(source.len());
    let mut segment = String::new();

    let apply_segment =
        |segment: &mut String, scopes: &mut Vec<HashMap<String, String>>, result: &mut String| {
            if segment.is_empty() {
                return;
            }

            for caps in const_re.captures_iter(segment.as_str()) {
                scopes
                    .last_mut()
                    .unwrap()
                    .insert(caps[1].to_string(), caps[2].to_string());
            }

            let rewritten = array_re.replace_all(segment.as_str(), |caps: &regex::Captures<'_>| {
                scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(&caps[1]))
                    .map(|value| format!("[{value}]"))
                    .unwrap_or_else(|| caps[0].to_string())
            });
            result.push_str(&rewritten);
            segment.clear();
        };

    for ch in source.chars() {
        match ch {
            '{' => {
                apply_segment(&mut segment, &mut scopes, &mut result);
                result.push(ch);
                scopes.push(HashMap::new());
            }
            '}' => {
                apply_segment(&mut segment, &mut scopes, &mut result);
                result.push(ch);
                if scopes.len() > 1 {
                    scopes.pop();
                }
            }
            _ => segment.push(ch),
        }
    }

    apply_segment(&mut segment, &mut scopes, &mut result);
    result
}

fn preprocess_glsl(source: &str) -> String {
    let mut lines: Vec<String> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Strip #version directives (our header provides it)
        if trimmed.starts_with("#version") {
            continue;
        }
        // Strip precision qualifiers (not valid in GLSL 450)
        if trimmed.starts_with("precision ") {
            continue;
        }
        // Strip uniform sampler2D declarations (our header provides split texture/sampler)
        if trimmed.contains("uniform")
            && trimmed.contains("sampler2D")
            && !trimmed.contains("layout")
        {
            continue;
        }

        lines.push(line.to_string());
    }

    let mut result = inline_const_array_sizes(&lines.join("\n"));
    result = result.replace("isnan(", "pst_isnan(");
    result = result.replace("isinf(", "pst_isinf(");
    result = result.replace("texture(", "pst_texture(");
    result = result.replace("texelFetch(", "pst_texelFetch(");

    // --- Phase 1: Collect functions that have sampler2D params ---
    // We need (func_name, param_name, param_position_in_arglist) for each
    let re_func_sig = regex::Regex::new(r"(?m)^\s*\w[\w\s*]*?\s+(\w+)\s*\(([^)]*)\)").unwrap();

    // Collect: (func_name, param_name)
    let mut s2d_funcs: Vec<(String, String)> = Vec::new();
    for caps in re_func_sig.captures_iter(&result.clone()) {
        let func_name = caps[1].to_string();
        let params_str = &caps[2];
        // Check if any param has sampler2D type
        for param_part in params_str.split(',') {
            let p = param_part.trim();
            if p.contains("sampler2D") {
                // Extract param name (last word after sampler2D)
                if let Some(name) = p.split_whitespace().last() {
                    s2d_funcs.push((func_name.clone(), name.to_string()));
                }
            }
        }
    }

    if s2d_funcs.is_empty() {
        return result;
    }

    // --- Phase 2: Rewrite function signatures ---
    // sampler2D pname -> texture2D pname_t, sampler pname_s
    let mut unique_params: Vec<String> = Vec::new();
    for (_, pname) in &s2d_funcs {
        if !unique_params.contains(pname) {
            unique_params.push(pname.clone());
        }
    }

    for pname in &unique_params {
        // Replace `in sampler2D pname` first (more specific)
        let sig_from2 = format!("in sampler2D {pname}");
        let sig_to2 = format!("in texture2D {pname}_t, sampler {pname}_s");
        result = result.replace(&sig_from2, &sig_to2);

        // Then bare `sampler2D pname`
        let sig_from = format!("sampler2D {pname}");
        let sig_to = format!("texture2D {pname}_t, sampler {pname}_s");
        result = result.replace(&sig_from, &sig_to);

        // --- Phase 3: Rewrite texture/texelFetch calls inside the function body ---
        let constructor = format!("sampler2D({pname}_t, {pname}_s)");

        for func in &[
            "texture",
            "texelFetch",
            "textureSize",
            "textureLod",
            "textureLodOffset",
            "textureGrad",
        ] {
            let usage = format!("{func}({pname},");
            let usage_to = format!("{func}({constructor},");
            result = result.replace(&usage, &usage_to);
        }
    }

    // --- Phase 4: Rewrite call sites ---
    // For each function that now takes (texture2D t, sampler s, ...) instead of (sampler2D, ...),
    // find calls like `FuncName(iChannel0, ...)` and expand to `FuncName(t_iChannel0, iLinearSampler, ...)`
    for (func_name, _pname) in &s2d_funcs {
        for ch in 0..4 {
            let call_from = format!("{func_name}(iChannel{ch},");
            let call_to = format!("{func_name}(t_iChannel{ch}, iLinearSampler,");
            result = result.replace(&call_from, &call_to);

            // Also handle with space: `FuncName( iChannel0,`
            let call_from2 = format!("{func_name}( iChannel{ch},");
            let call_to2 = format!("{func_name}( t_iChannel{ch}, iLinearSampler,");
            result = result.replace(&call_from2, &call_to2);
        }
    }

    result
}

fn looks_like_angle_hlsl_dump(source: &str) -> bool {
    source.len() > 50_000
        || source.contains("_KerrGeometry")
        || source.contains("#pragma pack_matrix")
        || source.contains("Generated by Microsoft (R) HLSL Shader Compiler")
        || source.contains("ps_")
        || source.contains("vs_")
}

fn wrap_single_source_glsl(source: &str) -> String {
    let cleaned = preprocess_glsl(source);
    let has_main = cleaned.contains("void main()");
    let has_main_image = cleaned.contains("void mainImage");

    let mut result = String::new();
    result.push_str(GLSL_SINGLE_HEADER);
    result.push_str(GLSL_COMPAT_HELPERS);
    result.push_str(&cleaned);

    if has_main_image && !has_main {
        result.push_str(GLSL_SINGLE_FOOTER);
    }

    result
}

fn wrap_multi_source_glsl(source: &str) -> String {
    let cleaned = preprocess_glsl(source);
    let has_main = cleaned.contains("void main()");
    let has_main_image = cleaned.contains("void mainImage");

    let mut result = String::new();
    result.push_str(GLSL_MULTI_HEADER);
    result.push_str(GLSL_COMPAT_HELPERS);
    result.push_str(&cleaned);

    if has_main_image && !has_main {
        result.push_str(GLSL_MULTI_FOOTER);
    }

    result
}

fn run_compile_job(
    job_id: u64,
    source: String,
    shader_language: ShaderInputLanguage,
    compile_update_tx: Sender<CompileUpdate>,
) {
    let send_progress = |stage: String| {
        let _ = compile_update_tx.send(CompileUpdate::Progress { job_id, stage });
    };

    send_progress("Parsing shader".to_string());

    let passes_raw = parse_passes(&source);
    let pass_count = passes_raw.len();
    let is_multi = passes_raw.len() > 1 || passes_raw.iter().any(pass_requires_multi_pipeline);
    let mut final_passes = Vec::with_capacity(pass_count);

    for (index, pass) in passes_raw.into_iter().enumerate() {
        let stage = match shader_language {
            ShaderInputLanguage::Wgsl => format!(
                "Preparing {} ({}/{})",
                pass.name,
                index + 1,
                pass_count.max(1)
            ),
            ShaderInputLanguage::Glsl => format!(
                "Transpiling {} ({}/{})",
                pass.name,
                index + 1,
                pass_count.max(1)
            ),
            ShaderInputLanguage::Hlsl => format!(
                "Checking {} ({}/{})",
                pass.name,
                index + 1,
                pass_count.max(1)
            ),
        };
        send_progress(stage);

        let wgsl_source = match shader_language {
            ShaderInputLanguage::Wgsl => {
                if is_multi {
                    wrap_multi_source(&pass.source)
                } else {
                    wrap_single_source(&pass.source)
                }
            }
            ShaderInputLanguage::Glsl => {
                let wrapped_glsl = if is_multi {
                    wrap_multi_source_glsl(&pass.source)
                } else {
                    wrap_single_source_glsl(&pass.source)
                };

                match glsl_to_wgsl(&wrapped_glsl) {
                    Ok(wgsl) => {
                        let wgsl = wgsl
                            .replace("@fragment \nfn main(", "@fragment \nfn fs_main(")
                            .replace("@fragment\nfn main(", "@fragment\nfn fs_main(");
                        let wgsl = fix_mainimage_signature_mismatch(&wgsl);
                        format!("{VERTEX_WGSL}\n{wgsl}")
                    }
                    Err(error) => {
                        let _ = compile_update_tx.send(CompileUpdate::Finished {
                            job_id,
                            result: Err(format!("GLSL → WGSL failed in {}:\n{}", pass.name, error)),
                        });
                        return;
                    }
                }
            }
            ShaderInputLanguage::Hlsl => {
                let detail = if looks_like_angle_hlsl_dump(&pass.source) {
                    "This looks like ANGLE or driver-generated DirectX shader output, not clean source HLSL."
                } else {
                    "The native preview can open .hlsl files now, but this build does not include a real HLSL-to-WGSL compilation path yet."
                };
                let _ = compile_update_tx.send(CompileUpdate::Finished {
                    job_id,
                    result: Err(format!(
                        "HLSL import is not executable yet in {}:\n{}\nUse the original Shadertoy GLSL or a hand-written WGSL shader for now.",
                        pass.name, detail
                    )),
                });
                return;
            }
        };

        final_passes.push(ParsedPass {
            name: pass.name,
            source: wgsl_source,
            channels: pass.channels,
        });
    }

    send_progress("Building pipelines".to_string());
    let _ = compile_update_tx.send(CompileUpdate::Finished {
        job_id,
        result: Ok(PreparedShader {
            passes: final_passes,
            is_multi,
            pass_count,
        }),
    });
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

#[derive(Clone, Copy, Debug)]
struct ScreenshotReadbackLayout {
    width: u32,
    height: u32,
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    buffer_size: u64,
}

impl ScreenshotReadbackLayout {
    fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Cannot capture a zero-sized screenshot.".to_string());
        }

        let unpadded_bytes_per_row = width
            .checked_mul(4)
            .ok_or_else(|| "Screenshot width is too large.".to_string())?;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| "Screenshot readback buffer would be too large.".to_string())?;

        Ok(Self {
            width,
            height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            buffer_size,
        })
    }
}

struct ScreenshotReadback {
    buffer: wgpu::Buffer,
    layout: ScreenshotReadbackLayout,
    format: wgpu::TextureFormat,
    path: PathBuf,
}

fn texture_copy_bytes_to_rgba(
    bytes: &[u8],
    layout: &ScreenshotReadbackLayout,
    format: wgpu::TextureFormat,
) -> Result<Vec<u8>, String> {
    if bytes.len() < layout.buffer_size as usize {
        return Err("Screenshot readback returned fewer bytes than expected.".to_string());
    }

    let mut rgba = vec![0_u8; layout.unpadded_bytes_per_row as usize * layout.height as usize];
    let padded_row = layout.padded_bytes_per_row as usize;
    let unpadded_row = layout.unpadded_bytes_per_row as usize;

    for y in 0..layout.height as usize {
        let src_row = y * padded_row;
        let dst_row = y * unpadded_row;
        for x in 0..layout.width as usize {
            let src = src_row + x * 4;
            let dst = dst_row + x * 4;
            match format {
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
                    rgba[dst..dst + 4].copy_from_slice(&bytes[src..src + 4]);
                }
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                    rgba[dst] = bytes[src + 2];
                    rgba[dst + 1] = bytes[src + 1];
                    rgba[dst + 2] = bytes[src];
                    rgba[dst + 3] = bytes[src + 3];
                }
                other => {
                    return Err(format!(
                        "Screenshots do not support the current surface format: {other:?}"
                    ));
                }
            }
        }
    }

    Ok(rgba)
}

fn screenshot_output_path() -> Result<PathBuf, String> {
    let dir = Path::new(SCREENSHOT_DIR);
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create screenshot directory: {e}"))?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Ok(dir.join(format!("native-capture-{millis}.png")))
}

fn queue_screenshot_copy(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Result<ScreenshotReadback, String> {
    let layout = ScreenshotReadbackLayout::new(width, height)?;
    let path = screenshot_output_path()?;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot-readback"),
        size: layout.buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(layout.padded_bytes_per_row),
                rows_per_image: Some(layout.height),
            },
        },
        wgpu::Extent3d {
            width: layout.width,
            height: layout.height,
            depth_or_array_layers: 1,
        },
    );

    Ok(ScreenshotReadback {
        buffer,
        layout,
        format,
        path,
    })
}

fn finish_screenshot_readback(
    device: &wgpu::Device,
    readback: ScreenshotReadback,
) -> Result<PathBuf, String> {
    let ScreenshotReadback {
        buffer,
        layout,
        format,
        path,
    } = readback;
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        })
        .map_err(|e| format!("Screenshot GPU readback poll failed: {e}"))?;
    let map_result = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("Screenshot GPU readback timed out: {e}"))?;
    map_result.map_err(|e| format!("Screenshot buffer map failed: {e}"))?;

    let mapped = slice.get_mapped_range();
    let rgba_result = texture_copy_bytes_to_rgba(&mapped, &layout, format);
    drop(mapped);
    buffer.unmap();
    let rgba = rgba_result?;

    save_png(&path, layout.width, layout.height, &rgba)?;
    Ok(path)
}

fn format_multipass_diagnostic(
    surface_width: u32,
    surface_height: u32,
    viewport: [f32; 4],
    preview_pixel_size: [u32; 2],
    i_resolution: [f32; 4],
    passes: &[CompiledPass],
    targets: &HashMap<String, PingPongTarget>,
    reset_temporal_state: bool,
    shader_frame: i32,
) -> String {
    let mut target_names: Vec<_> = targets.keys().cloned().collect();
    target_names.sort();
    let target_summary = target_names
        .iter()
        .filter_map(|name| {
            targets
                .get(name)
                .map(|target| format!("{name}={}x{}", target.width, target.height))
        })
        .collect::<Vec<_>>()
        .join(", ");

    let pass_summary = passes
        .iter()
        .map(|pass| {
            let channels = pass
                .channels
                .iter()
                .enumerate()
                .filter_map(|(index, channel)| {
                    let name = channel.as_ref()?;
                    let resolution = if name.eq_ignore_ascii_case("keyboard") {
                        "256x3".to_string()
                    } else if let Some(target) = targets.get(name) {
                        format!("{}x{}", target.width, target.height)
                    } else {
                        "missing".to_string()
                    };
                    Some(format!("iChannel{index}:{name}@{resolution}"))
                })
                .collect::<Vec<_>>()
                .join("|");
            format!("{}[{}]", pass.name, channels)
        })
        .collect::<Vec<_>>()
        .join("; ");

    format!(
        "Multipass diag: surface={}x{}, viewport=({:.1},{:.1},{:.1},{:.1}), preview={}x{}, iResolution=({:.1},{:.1},{:.4},{:.1}), targets=[{}], channels=[{}], reset_iFrame={}, iFrame={}",
        surface_width,
        surface_height,
        viewport[0],
        viewport[1],
        viewport[2],
        viewport[3],
        preview_pixel_size[0],
        preview_pixel_size[1],
        i_resolution[0],
        i_resolution[1],
        i_resolution[2],
        i_resolution[3],
        target_summary,
        pass_summary,
        reset_temporal_state,
        shader_frame
    )
}

fn save_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

fn spawn_stdin_thread() -> Receiver<SidecarCommand> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<SidecarCommand>(trimmed) {
                Ok(cmd) => {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
                Err(e) => emit(&SidecarEvent::Diagnostic {
                    level: "error",
                    message: format!("Failed to parse command: {e}"),
                }),
            }
        }
    });

    rx
}

fn emit(event: &SidecarEvent) {
    // Suppress noisy per-second stats from stdout; they're shown in the egui UI already.
    if matches!(event, SidecarEvent::Stats { .. }) {
        return;
    }
    if let Ok(text) = serde_json::to_string(event) {
        println!("{text}");
        let _ = io::stdout().flush();
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Native GLSL → WGSL transpilation via naga
// ═══════════════════════════════════════════════════════════════════════════════

/// Post-process naga WGSL output to fix a known inconsistency:
/// naga sometimes converts `void mainImage(out vec4 fragColor, vec2 fragCoord)` to a
/// 1-arg return-value form in the function definition, but leaves the call site in
/// fs_main using the 2-arg pointer form `mainImage((&color), coord)`, which wgpu rejects.
fn fix_mainimage_signature_mismatch(wgsl: &str) -> String {
    // Only apply if mainImage is a 1-arg function returning vec4<f32>
    let is_one_arg =
        regex::Regex::new(r"fn mainImage\(\s*\w+\s*:\s*vec2<f32>\s*\)\s*->\s*vec4<f32>")
            .unwrap()
            .is_match(wgsl);

    if !is_one_arg {
        return wgsl.to_string();
    }

    // Rewrite 2-arg pointer call:  mainImage((&varname), coord)  →  varname = mainImage(coord)
    // Also handles: mainImage(&varname, coord)
    let re = regex::Regex::new(r"mainImage\(\(?\s*&(\w+)\s*\)?,\s*(\w+)\)").unwrap();
    re.replace_all(wgsl, "$1 = mainImage($2)").to_string()
}

fn glsl_to_wgsl(glsl_source: &str) -> Result<String, String> {
    use naga::back::wgsl;
    use naga::front::glsl;

    let mut parser = glsl::Frontend::default();
    let options = glsl::Options::from(naga::ShaderStage::Fragment);

    let module = parser
        .parse(&options, glsl_source)
        .map_err(|errors| errors.emit_to_string(glsl_source))?;

    // Validate
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator
        .validate(&module)
        .map_err(|e| format!("Validation: {e}"))?;

    // Write WGSL
    let wgsl = wgsl::write_string(&module, &info, wgsl::WriterFlags::empty())
        .map_err(|e| format!("WGSL writer: {e}"))?;

    Ok(wgsl)
}

/// Return [year, month, day, seconds_since_midnight] like Shadertoy iDate
fn chrono_date_vec() -> [f32; 4] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Approximate — good enough for visual shaders
    let days = secs / 86400;
    let year = 1970.0 + (days as f32 / 365.25);
    let month = ((days % 365) / 30) as f32;
    let day = ((days % 365) % 30) as f32;
    let tod = (secs % 86400) as f32;
    [year, month, day, tod]
}

fn format_runtime(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_geometry_uses_current_layout_rect_for_16_9() {
        let container =
            egui::Rect::from_min_size(egui::pos2(20.0, 100.0), egui::vec2(1326.0, 1200.0));

        let geometry = compute_preview_geometry(
            container,
            1200.0,
            DisplayAspectPreset::Widescreen16x9,
            RenderScalePreset::Full,
            1.0,
        );

        assert_eq!(geometry.pixel_size, [1326, 746]);
        assert!((geometry.rect.min.x - 20.0).abs() < 0.01);
        assert!((geometry.rect.min.y - 100.0).abs() < 0.01);
        assert!((geometry.rect.width() - 1326.0).abs() < 0.01);
        assert!((geometry.rect.height() - 745.875).abs() < 0.01);
    }

    #[test]
    fn preview_geometry_height_limits_and_centers_wide_space() {
        let container =
            egui::Rect::from_min_size(egui::pos2(10.0, 50.0), egui::vec2(2000.0, 600.0));

        let geometry = compute_preview_geometry(
            container,
            600.0,
            DisplayAspectPreset::Widescreen16x9,
            RenderScalePreset::Full,
            1.0,
        );

        assert_eq!(geometry.pixel_size, [658, 370]);
        assert!((geometry.rect.min.x - 681.1111).abs() < 0.01);
        assert!((geometry.rect.min.y - 50.0).abs() < 0.01);
        assert!((geometry.rect.width() - 657.7778).abs() < 0.01);
        assert!((geometry.rect.height() - 370.0).abs() < 0.01);
    }

    #[test]
    fn screenshot_readback_layout_aligns_rows_to_wgpu_requirement() {
        let layout = ScreenshotReadbackLayout::new(1326, 746).unwrap();

        assert_eq!(layout.unpadded_bytes_per_row, 5304);
        assert_eq!(layout.padded_bytes_per_row, 5376);
        assert_eq!(layout.buffer_size, 4_010_496);
    }

    #[test]
    fn screenshot_copy_converts_bgra_surface_bytes_to_rgba_png_bytes() {
        let layout = ScreenshotReadbackLayout::new(2, 2).unwrap();
        let mut padded = vec![0_u8; layout.buffer_size as usize];
        padded[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let row_1 = layout.padded_bytes_per_row as usize;
        padded[row_1..row_1 + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);

        let rgba =
            texture_copy_bytes_to_rgba(&padded, &layout, wgpu::TextureFormat::Bgra8UnormSrgb)
                .unwrap();

        assert_eq!(
            rgba,
            vec![3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
    }
}

fn main() {
    env_logger::init();
    let initial_settings = render_settings::RenderSettings {
        backend: BackendChoice::parse(std::env::args().nth(1).as_deref()),
        ..Default::default()
    };
    let commands = spawn_stdin_thread();
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = PreviewApp::new(initial_settings, commands);
    event_loop
        .run_app(&mut app)
        .expect("failed to run PersonalShaderToy");
}
