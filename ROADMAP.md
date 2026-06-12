# PersonalShaderToy Roadmap

## Summary

- The primary target is now a standalone Rust desktop shader lab built in one native window with `winit + egui + wgpu`.
- We are no longer optimizing for `Tauri UI + separate native preview sidecar`; that path has been removed from the working tree.
- The immediate goal is a reliable native playground for loading, authoring, translating, and previewing shaders, especially Shadertoy-style GLSL/WGSL content.
- The current native UX direction is a split workflow inside the same window:
  - `Load Shaders` for browsing, selecting, compiling, previewing, and diagnostics
  - `Create Shader` for intentional editing with explicit `Compile` and `Save`
- A separate standalone browser app remains in the repo under `apps/web/`, but it is not the main product direction.

## Delivery Phases

### Phase 0: Documentation and repo hygiene

- Keep `CLAUDE.md`, `AGENTS.md`, `ROADMAP.md`, and `TIMELINE.md` aligned with the current standalone-Rust direction.
- Make the active entrypoints and shelved experiments obvious to future contributors.

### Phase 1: Browser/Tauri exploration (completed)

- The repo retains a standalone browser app in `apps/web/` as a separate surface that shares only the root `shaders/` directory.
- The old Tauri shell and native sidecar path are no longer part of the repository layout.

### Phase 2: Standalone Rust desktop foundation

- Build a root Rust application that owns:
  - the window shell
  - the preview surface
  - file browsing/loading
  - the authoring surface
  - diagnostics and stats
- Keep the first-party UX intentionally utilitarian if that helps us move faster on shader functionality.

### Phase 3: Shader-lab stabilization

- Improve native support for:
  - WGSL single-pass shaders
  - GLSL-to-WGSL translation for Shadertoy-style imports
  - multipass/ping-pong rendering
  - backend visibility and FPS realism
  - screenshots and basic workflow quality
- Prefer compatibility preprocessing and actionable diagnostics over opaque compiler failures.
- Keep large previewed shaders out of any always-live editing path so workflow features do not dominate shader load cost.
- For visual bugs, prove the native host path with small diagnostic shaders before blaming large ports. `shaders/Test/test5.wgsl` is the current multipass centering/feedback fixture.

### Phase 4: Workflow improvements

- Add the small quality-of-life features that help the lab feel reliable:
  - better diagnostics filtering
  - less noisy logs
  - cleaner shader browsing/loading behavior
  - explicit separation between previewing and authoring
  - dynamic preview sizing with fixed display aspect plus render scale
  - lightweight persistence where it helps iteration

### Phase 5: Engine export

- Continue to defer engine export until the standalone shader lab is stable.
- When export work starts, prefer a constrained intermediate representation and explicit validation over raw text dumping.

## Current Decisions

- The renderer and the editor should live in the same native app window.
- A second preview window is not part of the current plan.
- Loaded shaders should be preview-only in `Load Shaders`; editing should be intentional in `Create Shader`.
- Authoring should use explicit `Compile` and `Save`, not live recompilation on every text change.
- A fancy editor is optional; shader workflow reliability matters more.
- The standalone browser app should not drive native-app architecture decisions unless we intentionally choose to borrow from it.
- Engine export is still out of scope for the current stage.
- Native multipass passes must keep pass-local render state. In particular, each compiled pass owns its own uniform buffer so earlier offscreen passes cannot accidentally observe Image-pass viewport uniforms at GPU execution time.
- Remaining Kerr-Newman work is now mostly shader parity and visual-baseline work, not the previously observed native host centering drift.

## Implemented / Existing Paths

- The active native app lives at the repo root in `Cargo.toml` + `src/main.rs`.
- The standalone browser app lives in `apps/web/`.
- Both product surfaces share the root `shaders/` directory, but their rendering/runtime stacks are intentionally separate.
- Native authored shaders default to `shaders/User/`.
- The native app now uses split workspaces:
  - `Load Shaders` for preview/logs/backend info and shader selection
  - `Create Shader` for creating or intentionally opening source files in a plain editor
- The native preview now sizes rendering from the actual viewport with:
  - `Display Aspect`: `Auto`, `16:9`, `4:3`, `1:1`
  - `Render Scale`: `0.5x`, `0.75x`, `1.0x`
- Native multipass rendering now uses per-pass uniform buffers, which fixed the sampled-buffer drift exposed by `shaders/Test/test5.wgsl`.
- Native screenshots can be requested through the existing `TakeScreenshot`/`ScreenshotTaken` command path and are written under `target/native-captures/`.
- The native app is now a render lab:
  - `src/render_settings.rs` holds every renderer knob: backend, GPU adapter picker, present mode (with per-surface support detection and fallback), frame latency, and DX12 shader compiler (FXC vs statically linked DXC).
  - Present mode and frame latency apply via cheap surface reconfigure; backend/adapter/compiler changes rebuild the renderer on the same window.
  - `src/gpu_timer.rs` provides per-pass GPU timing through timestamp queries with a non-blocking readback ring; backends without support degrade to "GPU: n/a".
  - Live stats show rolling FPS, avg/p95/p99 frame times, 1% lows, and GPU pass breakdown from a 600-frame history (`src/bench.rs` stats math).
  - A benchmark sweep cycles selected backends (warmup + measure each), records CPU/GPU stats and pipeline compile times, renders a side-by-side results table, and saves JSON under `benchmarks/` (gitignored) with reload support.
  - `PST_AUTO_BENCH=warmup,measure` (optionally with `PST_AUTO_BENCH_SHADER=path`) runs a sweep headlessly and exits, for scripted comparisons.

## Recent Decision Log

### 2026-06-12

- Adopted the "render lab" direction: every interesting wgpu setting is exposed for tinkering and the benchmark sweep is the canonical way to compare backends.
- Benchmark sweeps force the Immediate present mode by default (falling back Mailbox → Fifo) so vsync does not flatten comparisons; the mode actually used is recorded per run.
- New code goes into focused modules (`src/bench.rs`, `src/render_settings.rs`, `src/gpu_timer.rs`) with small hooks in `main.rs`; no big-bang refactor of the monolith.
- The first sweep immediately exposed and fixed two host bugs: GL swapchains rejecting the screenshot `COPY_SRC` usage (now requested only when supported), and the resize-settle path parking the event loop before the first frame when no OS events arrive.

### 2026-04-24

- Keep both surfaces:
  - the root Rust native app remains the primary product direction
  - the web app under `apps/web/` remains a separate runnable reference/experimental surface
- Treat `test5.wgsl` as the cheap centering oracle before opening Kerr-Newman-sized shaders.
- The two-week native framing investigation resolved to a host multipass uniform lifetime bug: shared uniforms were overwritten before submitted GPU work consumed them. This is now fixed with per-pass uniform buffers.
