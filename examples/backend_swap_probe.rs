//! Diagnostic probe for the runtime backend-swap crash (2026-06-13 log):
//! `wgpu_hal::dx12 SwapChain creation error: Access is denied (0x80070005)`
//! after switching Gl → Dx12 on the same window.
//!
//! Hypothesis: WGL permanently sets the HWND's pixel format the first time the
//! GL backend touches the window, and DXGI then refuses to create a swapchain
//! on that HWND. If true, DX12 must fail on the GL-tainted window A but
//! succeed on a freshly created window B.
//!
//! Run with: `cargo run --example backend_swap_probe`

use std::sync::{Arc, Mutex};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Bring a backend fully up on the window (instance → surface → device →
/// configure → present one frame), then drop everything on return. Errors
/// from `Surface::configure` are captured through an error scope plus a
/// non-panicking uncaptured-error handler, mirroring the fix planned for the
/// app's `init_renderer`.
fn try_backend(window: Arc<Window>, backends: wgpu::Backends) -> Result<String, String> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);
    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| format!("create_surface: {e}"))?;
    let adapter = pollster::block_on(instance.enumerate_adapters(backends))
        .into_iter()
        .find(|a| a.is_surface_supported(&surface))
        .ok_or("no surface-compatible adapter")?;
    let adapter_name = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("probe-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| format!("request_device: {e}"))?;

    let uncaptured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = uncaptured.clone();
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
        sink.lock().unwrap().push(error.to_string());
    }));

    let caps = surface.get_capabilities(&adapter);
    let size = window.inner_size();
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: caps.formats[0],
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
    };
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(&device, &config);
    if let Some(err) = pollster::block_on(scope.pop()) {
        return Err(format!("Surface::configure (scoped): {err}"));
    }
    let pending = uncaptured.lock().unwrap().join(" | ");
    if !pending.is_empty() {
        return Err(format!("Surface::configure (uncaptured): {pending}"));
    }

    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        other => return Err(format!("get_current_texture: {other:?}")),
    };
    queue.submit([]);
    frame.present();
    Ok(adapter_name)
}

fn probe_window_attrs(title: &str) -> WindowAttributes {
    WindowAttributes::default()
        .with_title(title.to_string())
        .with_inner_size(PhysicalSize::new(640, 360))
}

fn run_probe(event_loop: &ActiveEventLoop) {
    let window_a = Arc::new(
        event_loop
            .create_window(probe_window_attrs("backend-swap probe A"))
            .expect("create window A"),
    );

    let gl = try_backend(window_a.clone(), wgpu::Backends::GL);
    println!("[probe] 1. GL on window A:                {gl:?}");

    let dx_tainted = try_backend(window_a.clone(), wgpu::Backends::DX12);
    println!("[probe] 2. DX12 on window A (after GL):   {dx_tainted:?}");

    drop(window_a);
    let window_b = Arc::new(
        event_loop
            .create_window(probe_window_attrs("backend-swap probe B"))
            .expect("create window B"),
    );
    let dx_fresh = try_backend(window_b.clone(), wgpu::Backends::DX12);
    println!("[probe] 3. DX12 on fresh window B:        {dx_fresh:?}");

    let hypothesis_confirmed = gl.is_ok() && dx_tainted.is_err() && dx_fresh.is_ok();
    println!(
        "[probe] hypothesis (GL taints HWND for DXGI): {}",
        if hypothesis_confirmed {
            "CONFIRMED — recreate the window when leaving GL"
        } else {
            "NOT confirmed — re-investigate"
        }
    );
}

struct Probe {
    done: bool,
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.done {
            self.done = true;
            run_probe(event_loop);
        }
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    event_loop
        .run_app(&mut Probe { done: false })
        .expect("run probe");
}
