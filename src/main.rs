mod bench;
mod capture;
mod document;
mod gpu_timer;
mod keyboard;
mod render_settings;
mod renderer;
mod viewer;
use chrono::{Datelike, Timelike};
use document::{CATALOG, Document};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use render_settings::{BackendChoice, DxCompilerChoice, PresentModeChoice, RenderSettings};
use renderer::Renderer;
use serde_json::{Value, json};
use shader_core::{Pass, Prepared};
use std::{
    collections::VecDeque,
    fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::PhysicalKey,
    window::{Fullscreen, Window, WindowId},
};

type Compiled = Result<(Vec<Pass>, Vec<Prepared>), String>;
fn event(kind: &str, mut fields: Value) {
    fields["type"] = json!(kind);
    println!("{fields}");
}
struct Active {
    doc: Document,
    passes: Vec<Pass>,
    prepared: Vec<Prepared>,
}
struct Inspector {
    open: bool,
    hlsl: bool,
    pass: usize,
    exports: Option<Vec<String>>,
    error: Option<String>,
}
#[derive(Default)]
struct Stats {
    samples: VecDeque<f64>,
    last: Option<Instant>,
    cpu: f64,
    compiled: f64,
    first: f64,
    cached: Option<bench::FrameStats>,
    updated: Option<Instant>,
}
impl Stats {
    fn clear(&mut self) {
        self.samples.clear();
        self.last = None;
        self.cached = None;
        self.updated = None;
        self.cpu = 0.;
    }
    fn frame(&mut self, at: Instant) -> Option<f64> {
        let delta = self
            .last
            .replace(at)
            .map(|t| (at - t).as_secs_f64() * 1000.);
        if let Some(ms) = delta {
            if self.samples.len() == 240 {
                self.samples.pop_front();
            }
            self.samples.push_back(ms);
        }
        if self
            .updated
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(250))
        {
            self.cached = bench::FrameStats::from_samples_ms(self.samples.make_contiguous());
            self.updated = Some(at);
        }
        delta
    }
}

#[derive(serde::Deserialize)]
struct FrameInput {
    #[serde(rename = "iTime")]
    time: f32,
    #[serde(rename = "iTimeDelta")]
    delta: f32,
    #[serde(rename = "iFrame")]
    frame: i32,
    #[serde(rename = "iDate")]
    date: [f32; 4],
}
struct App {
    manual: Option<FrameInput>,
    occluded: bool,
    last_ui: Instant,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    settings: RenderSettings,
    pending: Option<RenderSettings>,
    selected: Document,
    active: Option<Active>,
    serial: u64,
    loading: bool,
    started: Instant,
    compile_tx: Sender<(u64, Document)>,
    compile_rx: Receiver<(u64, Document, Compiled)>,
    commands: Receiver<Value>,
    watcher: Option<RecommendedWatcher>,
    file_events: Receiver<notify::Result<notify::Event>>,
    watch_at: Option<Instant>,
    message: String,
    failed: bool,
    playing: bool,
    needs_frame: bool,
    frame: u64,
    time: f64,
    stats: Stats,
    mouse: [f32; 4],
    cursor: [f32; 2],
    preview: egui::Rect,
    keys: [u8; 3072],
    advanced: bool,
    inspector: Inspector,
    scale: f32,
    fixed: Option<[u32; 2]>,
    capture_at: Option<u64>,
    capture_path: Option<PathBuf>,
    screenshot: bool,
    bench: Option<bench::BenchRunner>,
    prev: Option<RenderSettings>,
    saved: Option<bench::SweepResult>,
    bench_size: [u32; 2],
    benchmark_adapter: String,
    auto_bench: bool,
    warm: f64,
    measure: f64,
    uncapped: bool,
    quit: bool,
    exit_code: i32,
}
impl App {
    fn new() -> Result<Self, String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let value = |key: &str| {
            args.iter()
                .position(|a| a == key)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let path = value("--open")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("PST_AUTO_BENCH_SHADER").map(PathBuf::from));
        let example = value("--example").unwrap_or_default();
        let selected = if let Some(path) = path {
            Document::open(&path)?
        } else {
            Document::example(
                CATALOG
                    .iter()
                    .find(|e| {
                        !example.is_empty()
                            && e.title.to_lowercase().contains(&example.to_lowercase())
                    })
                    .unwrap_or(&CATALOG[0]),
            )
        };
        let backend =
            value("--backend").or_else(|| args.first().filter(|a| !a.starts_with('-')).cloned());
        let settings = RenderSettings {
            backend: BackendChoice::parse(backend.as_deref()),
            ..Default::default()
        };
        let fixed = value("--size").map(|v| parse_size(&v)).transpose()?;
        let capture_path = value("--capture").map(PathBuf::from);
        let capture_at = if capture_path.is_some() {
            Some(
                value("--frames")
                    .unwrap_or_else(|| "8".into())
                    .parse::<u64>()
                    .map_err(|_| "Invalid --frames")?
                    .max(1),
            )
        } else {
            None
        };
        let spec = std::env::var("PST_AUTO_BENCH").ok();
        let mut times = spec
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|n| n.parse::<f64>().ok());
        let (compile_tx, requests) = mpsc::channel::<(u64, Document)>();
        let (results, compile_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("shader compiler".into())
            .stack_size(64 << 20)
            .spawn(move || {
                while let Ok(mut request) = requests.recv() {
                    while let Ok(newer) = requests.try_recv() {
                        request = newer;
                    }
                    let (id, doc) = request;
                    let result = std::panic::catch_unwind(|| doc.compile())
                        .unwrap_or_else(|_| Err("Shader compiler panicked".into()));
                    if results.send((id, doc, result)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let (watch_tx, file_events) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |e| {
            let _ = watch_tx.send(e);
        })
        .ok();
        let (tx, commands) = mpsc::channel();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                if let Ok(cmd) = serde_json::from_str(&line) {
                    if tx.send(cmd).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Self {
            manual: None,
            occluded: false,
            last_ui: Instant::now(),
            window: None,
            renderer: None,
            settings,
            pending: None,
            selected,
            active: None,
            serial: 0,
            loading: false,
            started: Instant::now(),
            compile_tx,
            compile_rx,
            commands,
            watcher,
            file_events,
            watch_at: None,
            message: String::new(),
            failed: false,
            playing: true,
            needs_frame: true,
            frame: 0,
            time: 0.,
            stats: Stats::default(),
            mouse: [0.; 4],
            cursor: [0.; 2],
            preview: egui::Rect::NOTHING,
            keys: [0; 3072],
            advanced: false,
            inspector: Inspector {
                open: false,
                hlsl: false,
                pass: 0,
                exports: None,
                error: None,
            },
            scale: 1.,
            fixed,
            capture_at,
            capture_path,
            screenshot: false,
            bench: None,
            prev: None,
            saved: None,
            bench_size: [1280, 720],
            benchmark_adapter: String::new(),
            auto_bench: spec.is_some(),
            warm: times.next().flatten().unwrap_or(3.).clamp(0.5, 30.),
            measure: times.next().flatten().unwrap_or(10.).clamp(1., 60.),
            uncapped: true,
            quit: false,
            exit_code: 0,
        })
    }
    fn watch(&mut self, doc: &Document) {
        if let Some(w) = &mut self.watcher {
            if let Some(p) = self.selected.path.as_ref().and_then(|p| p.parent()) {
                let _ = w.unwatch(p);
            }
            if let Some(p) = doc.path.as_ref().and_then(|p| p.parent()) {
                if let Err(e) = w.watch(p, RecursiveMode::NonRecursive) {
                    self.message = e.to_string();
                }
            }
        }
    }
    fn load(&mut self, doc: Document) {
        self.watch(&doc);
        self.watch_at = None;
        self.serial += 1;
        self.loading = true;
        self.started = Instant::now();
        self.failed = false;
        self.message = format!("Compiling {}…", doc.title);
        self.selected = doc.clone();
        let _ = self.compile_tx.send((self.serial, doc));
    }
    fn error(&mut self, error: String) {
        self.failed = true;
        self.message = format!("{} · {error}", self.selected.title);
        if let Some(a) = &self.active {
            self.message += &format!("\nShowing last successful preview: {}", a.doc.title);
        }
        event(
            "Diagnostic",
            json!({"level":"error","message":self.message}),
        );
        event("ShaderUpdated", json!({"success":false}));
        if let Some(b) = &mut self.bench {
            b.backend_failed(error);
        }
        if self.capture_at.is_some() || self.auto_bench && self.bench.is_none() {
            self.exit_code = 1;
            self.quit = true;
        }
    }
    fn polls(&mut self) {
        while let Ok(result) = self.file_events.try_recv() {
            if let (Ok(e), Some(path)) = (result, &self.selected.path) {
                if e.paths
                    .iter()
                    .any(|p| p == path || p.canonicalize().ok().as_ref() == Some(path))
                {
                    self.watch_at = Some(Instant::now());
                }
            }
        }
        if self.bench.is_none()
            && self
                .watch_at
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(100))
        {
            self.watch_at = None;
            if let Some(path) = &self.selected.path {
                match Document::open(path) {
                    Ok(d) if d.source != self.selected.source || self.failed => self.load(d),
                    Err(e) => self.error(e),
                    _ => {}
                }
            }
        }
        while let Ok((id, doc, result)) = self.compile_rx.try_recv() {
            if id != self.serial {
                continue;
            }
            self.loading = false;
            let result = result.and_then(|(passes, prepared)| {
                self.renderer
                    .as_mut()
                    .ok_or("Renderer unavailable")?
                    .install(&prepared)?;
                Ok(Active {
                    doc,
                    passes,
                    prepared,
                })
            });
            match result {
                Err(e) => self.error(e),
                Ok(active) => {
                    self.active = Some(active);
                    self.inspector.exports = None;
                    self.inspector.error = None;
                    self.inspector.pass = 0;
                    self.stats.compiled = self.started.elapsed().as_secs_f64() * 1000.;
                    self.stats.first = 0.;
                    self.rewind(false);
                    self.failed = false;
                    self.message = if self.playing { "Ready" } else { "Paused" }.into();
                    event(
                        "ShaderUpdated",
                        json!({"success":true,"shader":self.selected.title,"compile_ms":self.stats.compiled}),
                    );
                    if let (Some(b), Some(r)) = (&mut self.bench, &self.renderer) {
                        b.backend_ready(
                            clock(),
                            format!("{:?}", r.info.backend),
                            r.info.name.clone(),
                            format!("{:?}", r.config.present_mode),
                            self.bench_size,
                            Some(self.stats.compiled),
                        );
                    }
                }
            }
        }
        while let Ok(c) = self.commands.try_recv() {
            match c["type"].as_str().unwrap_or("") {
                "shutdown" => self.quit = true,
                "ping" => event("Pong", json!({})),
                "set_shader" => {
                    if let Some(s) = c["source"].as_str() {
                        let mut d = Document::clipboard(s);
                        if let Some(l) = c["language"].as_str() {
                            d.language = l.into();
                        }
                        self.load(d);
                    }
                }
                "open" => {
                    if let Some(p) = c["path"].as_str() {
                        self.open(Path::new(p));
                    }
                }
                "set_resolution" => {
                    if let (Some(w), Some(h), Some(win)) =
                        (c["width"].as_u64(), c["height"].as_u64(), &self.window)
                    {
                        let _ = win.request_inner_size(PhysicalSize::new(w as u32, h as u32));
                    }
                }
                "set_backend" => {
                    let mut settings = self.settings.clone();
                    settings.backend = BackendChoice::parse(c["backend"].as_str());
                    self.pending = Some(settings);
                }
                "set_keyboard" => {
                    self.keys.fill(0);
                    if let Some(keys) = c["keys"].as_array() {
                        for k in keys.iter().filter_map(Value::as_u64).filter(|k| *k < 256) {
                            self.keys[k as usize * 4] = 255;
                        }
                    }
                }
                "set_render_scale" => {
                    if let Some(scale) = c["scale"].as_f64().filter(|s| (0.25..=2.).contains(s)) {
                        self.scale = scale as f32;
                    }
                }
                "set_uniforms" => {
                    if let (Some(w), Some(h)) =
                        (c["iResolution"][0].as_f64(), c["iResolution"][1].as_f64())
                    {
                        if (1.0..=8192.0).contains(&w) && (1.0..=8192.0).contains(&h) {
                            self.fixed = Some([w as u32, h as u32]);
                            self.scale = 1.;
                        }
                    }
                    self.manual = serde_json::from_value(c.clone()).ok();
                    self.needs_frame = true;
                    if let Some(m) = c["iMouse"].as_array() {
                        for (i, v) in m.iter().take(4).enumerate() {
                            self.mouse[i] = v.as_f64().unwrap_or(0.) as f32;
                        }
                    }
                }
                "take_screenshot" => self.screenshot = true,
                "pause" => self.toggle_play(),
                "restart" => self.rewind(true),
                _ => {}
            }
        }
    }
    fn rewind(&mut self, clear: bool) {
        self.time = 0.;
        self.frame = 0;
        self.needs_frame = true;
        self.stats.clear();
        if clear {
            if let Some(r) = &mut self.renderer {
                r.reset_feedback();
            }
        }
    }
    fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.stats.clear();
        if !self.failed && !self.loading {
            self.message = if self.playing { "Ready" } else { "Paused" }.into();
        }
    }
    fn open(&mut self, path: &Path) {
        match Document::open(path) {
            Ok(doc) => self.load(doc),
            Err(e) => self.error(format!("{}: {e}", path.display())),
        }
    }
    fn swap(&mut self, el: &ActiveEventLoop, settings: RenderSettings) -> Result<(), String> {
        let gl = self
            .renderer
            .as_ref()
            .is_some_and(|r| r.info.backend == wgpu::Backend::Gl);
        self.renderer.take();
        if gl && settings.backend != BackendChoice::Opengl {
            let old = self.window.take().unwrap();
            let mut attrs = window_attrs()
                .with_inner_size(old.inner_size())
                .with_maximized(old.is_maximized());
            if let Ok(pos) = old.outer_position() {
                attrs = attrs.with_position(pos);
            }
            self.window = Some(Arc::new(
                el.create_window(attrs).map_err(|e| e.to_string())?,
            ));
        }
        let window = self.window.clone().unwrap();
        let r = Renderer::new(window, &settings)?;
        event(
            "WindowReady",
            json!({"width":r.config.width,"height":r.config.height,"driver":r.info.driver,"backend_type":format!("{:?}",r.info.backend)}),
        );
        self.settings = settings;
        self.renderer = Some(r);
        let doc = self.selected.clone();
        self.load(doc);
        Ok(())
    }
    fn start_bench(&mut self) {
        if self.active.is_none() || self.failed || self.loading || self.bench.is_some() {
            return;
        }
        let r = self.renderer.as_ref().unwrap();
        self.bench_size = r.size;
        self.benchmark_adapter = r.info.name.clone();
        self.prev = Some(self.settings.clone());
        self.bench = Some(bench::BenchRunner::new(bench::BenchConfig {
            warmup_secs: self.warm,
            measure_secs: self.measure,
            backend_labels: BackendChoice::ui_choices()
                .iter()
                .filter(|b| **b != BackendChoice::Auto)
                .map(|b| b.label().into())
                .collect(),
            force_uncapped: self.uncapped,
        }));
        self.saved = None;
    }
    fn drive_bench(&mut self, interval: Option<f64>, gpu: Vec<f64>) {
        let Some(b) = &mut self.bench else { return };
        if let Some(ms) = interval {
            b.record_frame(clock(), ms, gpu);
        }
        match b.next_action() {
            bench::BenchAction::SwitchBackend { label } => {
                let mut s = self.settings.clone();
                s.backend = *BackendChoice::ui_choices()
                    .iter()
                    .find(|b| b.label() == label)
                    .unwrap();
                s.adapter_name = Some(self.benchmark_adapter.clone());
                if b.config().force_uncapped {
                    s.present_mode = PresentModeChoice::Immediate;
                }
                self.pending = Some(s);
            }
            bench::BenchAction::Finished => {
                self.saved = b.take_result(self.selected.title.clone());
                if let Some(result) = &self.saved {
                    match bench::save_sweep(result, Path::new("benchmarks")) {
                        Ok(path) => {
                            self.message = format!("Benchmark saved to {}", path.display());
                            event("BenchmarkSaved", json!({"path":path}));
                        }
                        Err(e) => self.error(e),
                    }
                }
                self.bench = None;
                self.pending = self.prev.take();
                if self.auto_bench {
                    self.quit = true;
                }
            }
            bench::BenchAction::None => {}
        }
    }
}
fn clock() -> f64 {
    use std::sync::OnceLock;
    static ZERO: OnceLock<Instant> = OnceLock::new();
    ZERO.get_or_init(Instant::now).elapsed().as_secs_f64()
}
fn parse_size(s: &str) -> Result<[u32; 2], String> {
    let (w, h) = s.split_once('x').ok_or("Expected WIDTHxHEIGHT")?;
    let size = [
        w.parse::<u32>().map_err(|_| "Invalid width")?,
        h.parse::<u32>().map_err(|_| "Invalid height")?,
    ];
    if size.iter().any(|v| *v == 0 || *v > 8192) {
        return Err("Render dimensions must be between 1 and 8192".into());
    }
    Ok(size)
}
fn window_attrs() -> winit::window::WindowAttributes {
    Window::default_attributes()
        .with_title("PersonalShaderToy")
        .with_inner_size(PhysicalSize::new(1350, 850))
        .with_min_inner_size(PhysicalSize::new(520, 375))
}
impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match el.create_window(window_attrs()) {
            Ok(w) => self.window = Some(Arc::new(w)),
            Err(e) => {
                self.error(e.to_string());
                self.exit_code = 1;
                el.exit();
                return;
            }
        }
        if let Err(e) = self.swap(el, self.settings.clone()) {
            self.error(e);
            self.exit_code = 1;
            el.exit();
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, e: WindowEvent) {
        let Some(w) = self.window.clone() else { return };
        if w.id() != id {
            return;
        }
        if let Some(r) = &mut self.renderer {
            let _ = r.egui_state.on_window_event(&w, &e);
        }
        match e {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Occluded(value) => {
                self.occluded = value;
                self.stats.clear();
            }
            WindowEvent::DroppedFile(p) => {
                if self.bench.is_none() {
                    self.open(&p)
                }
            }
            WindowEvent::Resized(s) => {
                if let Some(r) = &mut self.renderer {
                    if s.width > 0 && s.height > 0 {
                        r.config.width = s.width;
                        r.config.height = s.height;
                        r.surface.configure(&r.device, &r.config);
                    }
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.fill(0);
                self.stats.clear();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let ppp = self
                    .renderer
                    .as_ref()
                    .map_or(1., |r| r.egui_ctx.pixels_per_point());
                self.cursor = [position.x as f32 / ppp, position.y as f32 / ppp];
                if let Some(r) = &self.renderer {
                    if self
                        .preview
                        .contains(egui::pos2(self.cursor[0], self.cursor[1]))
                    {
                        self.mouse[0] = (self.cursor[0] - self.preview.left())
                            / self.preview.width()
                            * r.size[0] as f32;
                        self.mouse[1] = (self.preview.bottom() - self.cursor[1])
                            / self.preview.height()
                            * r.size[1] as f32;
                    }
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => {
                if state == ElementState::Pressed
                    && self
                        .preview
                        .contains(egui::pos2(self.cursor[0], self.cursor[1]))
                {
                    self.mouse[2] = self.mouse[0];
                    self.mouse[3] = self.mouse[1];
                }
                if state == ElementState::Released {
                    self.mouse[2] = -self.mouse[2].abs();
                    self.mouse[3] = -self.mouse[3].abs();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(i) = keyboard::index(code) {
                        let k = i as usize * 4;
                        let down = event.state.is_pressed();
                        if down && self.keys[k] == 0 {
                            self.keys[1024 + k] = 255;
                            self.keys[2048 + k] ^= 255;
                        }
                        self.keys[k] = if down { 255 } else { 0 };
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.polls();
        if self.quit {
            el.exit();
            return;
        }
        if let Some(s) = self.pending.take() {
            let old = self.settings.clone();
            if let Err(e) = self.swap(el, s) {
                if self.bench.is_some() {
                    self.bench.as_mut().unwrap().backend_failed(e.clone());
                }
                event("Diagnostic", json!({"level":"error","message":e}));
                if let Err(e) = self.swap(el, old) {
                    self.error(e);
                    self.exit_code = 1;
                    el.exit();
                    return;
                }
            }
        }
        let idle = self.occluded
            || (!self.playing && !self.needs_frame && !self.loading && self.bench.is_none());
        let next = self.last_ui + Duration::from_millis(80);
        if idle {
            el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                next.max(Instant::now() + Duration::from_millis(1)),
            ));
        } else {
            el.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }
        if let Some(w) = &self.window {
            if !idle || Instant::now() >= next {
                w.request_redraw();
            }
        }
    }
}
fn main() {
    env_logger::init();
    let result = App::new().and_then(|mut app| {
        let el = EventLoop::new().map_err(|e| e.to_string())?;
        el.run_app(&mut app).map_err(|e| e.to_string())?;
        Ok(app.exit_code)
    });
    match result {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
