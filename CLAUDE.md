# PersonalShaderToy Instructions

This file is the canonical project instruction source. `AGENTS.md` must mirror it.

## Product Direction

- The active product direction is a standalone Rust desktop shader lab built around `winit + egui + wgpu`.
- The editor UI and the shader preview live in the same native window. We are not currently targeting a `Tauri shell + separate sidecar preview window`.
- Inside that one window, previewing and authoring are now intentionally split into `Load Shaders` and `Create Shader`.
- The standalone browser app stays in the repo as a separate product surface for reference and experimentation, but it is not the primary architecture target.
- Early priorities are:
  - stable shader loading and file browsing in the native app
  - practical GLSL/WGSL workflows, including Shadertoy-style imports
  - useful diagnostics without noisy debug spam
  - safe shader-lab iteration with explicit compile/save authoring flow before polishing editor UX
- Engine export is intentionally deferred until the standalone shader lab is reliable.

## Implementation Priorities

1. Keep the standalone Rust app in `Cargo.toml` + `src/main.rs` functional while we iterate.
2. Improve GLSL/WGSL compatibility and multipass behavior in the native pipeline.
3. Treat the integrated native window as the only preview surface unless we explicitly decide otherwise later.
4. Use the browser/Tauri code as reference or fallback only; do not let it drive new architecture decisions by default.
5. Prefer reliable diagnostics and workflow clarity over editor polish.
6. Keep loaded-shader previewing separate from intentional source editing whenever that reduces heavy-shader UI overhead.
7. For native visual bugs, validate host geometry, uniforms, target sizes, and pass feedback with small diagnostic shaders before opening giant shader ports.

## Repo Conventions

- Root planning/status docs:
  - `ROADMAP.md` is the implementation plan and decision log.
  - `TIMELINE.md` is the current implementation tracker.
- Fast navigation docs:
  - `docs/PROJECT_MAP.md` is the cheapest repo entry map.
  - `docs/SHADER_INDEX.md` is the shader routing/index file.
  - `docs/WORKING_RULES.md` is the context-discipline guide.
- `AGENTS.md` must stay in sync with this file.
- The primary desktop entrypoint is the root Rust app (`Cargo.toml`, `src/main.rs`).
- The standalone browser app lives in `apps/web/` and only shares the root `shaders/` directory with the Rust app.
- Kerr-Newman reference materials live under `docs/kerr-newman-port/reference/` and `docs/kerr-newman-port/baselines/`.
- `shaders/Test/test5.wgsl` is the current cheap multipass centering/feedback diagnostic.
- Avoid coupling the Rust app and browser app beyond the shared shader corpus unless we explicitly choose to do so later.
- Avoid opening giant shader files first when a manifest or index doc can route the task.

## Recent Native Rendering Notes

- On `2026-04-24`, the long-running native multipass centering issue was traced to a host-side uniform lifetime bug, not to `test5.wgsl` shader math.
- Each compiled native multipass pass must own and bind its own uniform buffer. Reusing one shared `multi_uniform_buf` lets later Image-pass viewport uniforms overwrite earlier offscreen pass uniforms before the GPU executes the submitted command encoder.
- After that fix, remaining Kerr-Newman work should be treated as shader parity/visual-baseline work unless a small diagnostic shader proves a host regression.

## Render Lab Notes

- On `2026-06-12`, the native app gained render-lab features: render settings in `src/render_settings.rs` (backend, adapter picker, present mode, frame latency, DX12 compiler), per-pass GPU timing in `src/gpu_timer.rs`, and percentile stats plus an automated benchmark sweep in `src/bench.rs`.
- Present mode and frame latency apply via surface reconfigure; backend, adapter, and DX12 compiler changes rebuild the renderer, reusing the window except when leaving GL (see the third invariant below).
- Benchmark sweeps save JSON to `benchmarks/` (gitignored). Headless: set `PST_AUTO_BENCH=warmup,measure` and optionally `PST_AUTO_BENCH_SHADER=path` — the app sweeps and exits on its own.
- Two host invariants learned from the first sweeps: never request surface usages the capabilities don't list (GL lacks `COPY_SRC`; `Surface::configure` panics), and every `about_to_wait` path must request a redraw or the Wait-driven event loop parks before the first frame.
- Third host invariant (2026-06-12): WGL permanently sets the HWND pixel format, so DXGI refuses swapchain creation on any window the GL backend has used (`E_ACCESSDENIED`). Rebuilding away from GL therefore replaces the native window in `apply_pending_rebuild`, and `init_renderer` wraps `Surface::configure` in an error scope so failed swaps revert instead of panicking. Standalone repro: `examples/backend_swap_probe.rs`.
