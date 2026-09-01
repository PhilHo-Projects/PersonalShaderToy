# PersonalShaderToy Implementation Timeline

## Current Phase

- `Phase 3 — Standalone Rust shader-lab stabilization`

## Phase Status

- Docs refresh for the post-Tauri direction: `complete`
- Standalone native app (`winit + egui + wgpu`): `active`
- Standalone browser app split into `apps/web`: `complete`
- Browser app hosted as a public static site: `complete`
- Integrated single-window native workflow split (`Load Shaders` + `Create Shader`): `complete`
- Preview-only loading flow for large shaders: `complete`
- Explicit create/edit flow with manual `Compile` and `Save`: `complete`
- Dynamic preview sizing (`Display Aspect` + `Render Scale`): `complete`
- Compile progress/status surfaced in the UI: `complete`
- Compile-loop guard for preview-size churn: `complete`
- Native multipass viewport/uniform stability: `fixed`
- GLSL/WGSL translation and Shadertoy compatibility: `active work`
- Tauri/sidecar path removal: `complete`
- Render lab (settings + GPU timing + percentile stats + benchmark sweep): `complete`
- Engine export: `deferred`

## Exit Criteria For The Current Stage

- Root docs describe the standalone Rust app as the active architecture.
- The root native app can browse and load shaders without duplicate compile churn.
- Large loaded shaders stay out of the live editor path unless intentionally opened in `Create Shader`.
- WGSL shaders compile reliably in the integrated renderer.
- Shadertoy-style GLSL imports work for more real-world samples, including large shaders that need preprocessing before naga translation.
- Diagnostics stay useful without flooding the UI or stdout with low-signal noise.

## Next Milestone

- Re-check Kerr-Newman visual parity now that the native multipass host centering bug is fixed.
- Keep hardening the native shader pipeline against real imported shaders.
- Keep the standalone browser app usable without letting it drive native architecture choices.
- Use `shaders/Test/test5.wgsl` as the first native multipass centering/feedback diagnostic before opening giant shader ports.

## Current Blockers / Watch Items

- The repo still contains multiple architectural paths, so stale docs can easily send work in the wrong direction.
- GLSL compatibility depends on our preprocessing layer working around naga limitations.
- The browser app and Rust app now split cleanly, so shared-path assumptions should stay limited to the root `shaders/` corpus.
- Heavy imported shaders can still be slow enough that we need to separate true shader cost from translation/build cost and dev-profile overhead.
- Kerr-Newman still needs shader-level visual parity work, but the native host centering/resize artifact should no longer be treated as the primary suspect.
- Automated native screenshot capture through stdin remains flaky in local harnesses even though the screenshot command path exists.

## Verified Milestones

- Root standalone Rust app exists in `Cargo.toml` + `src/main.rs`.
- Standalone browser app exists in `apps/web/`.
- Integrated native window now separates previewing and authoring into `Load Shaders` and `Create Shader`.
- `Create Shader` writes real files under `shaders/User/` and compiles only when requested.
- Native preview resolution now comes from viewport size plus aspect/scale controls instead of the old fixed preset flow.
- Native multipass render passes now use per-pass uniform buffers, fixing the sampled-buffer centering drift seen in `test5.wgsl`.
- `shaders/Test/test5.wgsl` is now a compact multipass centering diagnostic for Buffer A/B/C/Image alignment.
- `cargo check`
- `cargo test`
- `cargo run --bin test_parse_wgsl shaders/Test/test5.wgsl`
- `cargo run --bin test_parse shaders/Shadertoy/kerr_newman_black_hole.glsl`

## Journal

### 2026-06-12 — Backend-swap crash fixed: GL taints the HWND for DXGI

- Symptom: switching backends at runtime Gl → Dx12 crashed: `wgpu_hal::dx12 SwapChain creation error: Access is denied (0x80070005)`, then a fatal `Surface::configure → Invalid surface` panic (exit 101).
- Diagnosis via `examples/backend_swap_probe.rs`: WGL permanently sets the window's pixel format the first time the GL backend touches it, and DXGI then refuses to create a swapchain on that HWND. The same DX12 request succeeds on a freshly created window.
- Fixes:
  - `apply_pending_rebuild` replaces the native window when leaving the GL backend on Windows, carrying over size, position, and maximized state.
  - `init_renderer` wraps `Surface::configure` in an error scope so configure failures return `Err` into the existing revert path instead of panicking (configure reports through the device error sink, not a `Result`).
  - Sidecar `set_backend` now drives the same live rebuild path as the UI instead of replying "requires restarting the sidecar".
  - Hygiene: removed the unread `FrameGpuTiming::total_ms` field, gated `BenchRunner::is_active` to tests; `cargo check` is warning-free again.
- Verification: scripted stdin run Dx12 → Gl → Dx12 exits 0 with three `WindowReady` events; quick headless sweep (`PST_AUTO_BENCH=0.5,1`) across DX12/Vulkan/GL still completes and saves JSON; `cargo test` 19 passed.

### 2026-08-31 — Browser app deployed as a public static site

- `apps/web` now ships as a static bundle at `https://shaderlab.philippeho.dev`, hosted as a Coolify Docker application (nginx on port 8000, manual releases, no webhook).
- Added `apps/web/scripts/build-shader-manifest.mjs`, a `prebuild` step copying the root `shaders/` corpus into `public/shaders/` alongside a `manifest.json` matching the shape `ShaderLibraryService.list()` already returns.
- Added `StaticShaderLibraryService`: lists and loads over plain fetch, keeps visitor edits in `localStorage` under `pst:shader:<provider>/<filename>`, and does no file watching. `main.ts` selects it via `import.meta.env.PROD`; dev keeps the Express-backed service untouched.
- Because `load()` prefers a local override, edited files now show a revert control; without it a visitor who saved a broken shader would be stuck with it permanently.
- A failed manifest fetch renders an error in the file browser rather than an empty tree, so a broken deploy is visibly broken.
- The Docker build context is the repository root, not `apps/web` — the manifest script reads `../../../shaders`, which sits above the web app.
- The repository was made public; the tracked instruction files and their full history were scrubbed of infrastructure details first.
- Dropped `naga-wasm`, which was declared but never imported.
- The corpus is 19 shaders across 6 providers; `shaders/User/` is empty by design and is skipped by the manifest.

### 2026-06-12 — Render lab: settings, GPU timing, benchmark sweep

- Added `src/render_settings.rs` (backend/adapter/present-mode/latency/DX12-compiler model), `src/gpu_timer.rs` (per-pass timestamp queries with async readback ring), and `src/bench.rs` (percentile stats, sweep state machine, JSON persistence) with unit tests for all pure logic.
- Live stats upgraded to rolling percentiles (avg/p95/p99/1% low) plus GPU pass breakdown.
- `Run Benchmark` sweeps selected backends with warmup/measure phases and renders a best-value-highlighted comparison table; sweeps save to `benchmarks/` and reload across sessions.
- Headless mode: `PST_AUTO_BENCH=1,4` + `PST_AUTO_BENCH_SHADER=shaders\Test\test5.wgsl` ran full DX12/Vulkan/GL sweeps and exited cleanly.
- The sweep caught two host bugs on its first runs:
  - GL swapchain panic in `Surface::configure`: the screenshot `COPY_SRC` usage is not universally supported; it is now requested only when the surface offers it and screenshots degrade with a warning.
  - Event-loop deadlock: the resize-settle branch in `about_to_wait` returned without requesting a redraw, parking the Wait-driven loop before the first frame whenever no further OS events arrived (headless runs or an untouched window).
- First real numbers (RTX 4060 Laptop, test5.wgsl, Immediate): Vulkan and GL p95 ≈ 6.2 ms vs DX12 8.1 ms; DX12 1% low 113 fps vs ~152 fps; pipeline compile DX12 33 ms / Vulkan 46 ms / GL 52 ms.

### 2026-04-24 — Native multipass centering bug finally isolated

- Symptom: `test5.wgsl` showed the final Image pass center marker in the correct place, while sampled Buffer A/B/C content was shifted right/down.
- Diagnosis: the shader math was not the main issue. Native multipass rendering recorded all passes into one command encoder while reusing one shared `multi_uniform_buf`; the Image pass then overwrote viewport-origin uniforms before the GPU executed earlier buffer passes.
- Fix: `CompiledPass` now owns a per-pass uniform buffer, and the render loop writes/binds `cp.uniform_buf` for that pass.
- Result: host-side sampled-buffer drift is fixed. Any remaining Kerr-Newman mismatch should be investigated as shader parity, pass feedback, or visual baseline work rather than a generic native centering bug.
- Verification run: `cargo check`, `cargo test`, `cargo run --bin test_parse_wgsl shaders/Test/test5.wgsl`, and `cargo run --bin test_parse shaders/Shadertoy/kerr_newman_black_hole.glsl`.

### 2026-04-24 — Native resize/framing stabilization pass

- Centralized preview geometry around the current egui viewport instead of stale stored rectangles.
- Synchronized ping-pong target sizes with the active preview pixel size.
- Reset temporal frame state on shader load, display-aspect/render-scale changes, and target resize.
- Added screenshot readback helpers that save captures under `target/native-captures/`; local stdin automation still needs a more reliable harness.

## Notes

- The current product direction values one-window workflow clarity over editor polish.
- The current native editor is intentionally plain and non-live so large previewed shaders do not pay for syntax/editor churn by default.
- The browser app should be treated as a separate secondary surface, not as the default architecture template for the Rust app.
