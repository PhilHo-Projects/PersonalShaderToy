# Render Lab: Backend Settings + Benchmark Sweep — Design

Date: 2026-06-11
Status: Approved by user

## Goal

Turn the native shader lab's existing backend switcher into a full "render lab": every
interesting wgpu render setting exposed for tinkering, real GPU timing, and an automated
benchmark sweep that compares backends side by side on the current shader.

## Background

- The native app (root `src/main.rs`, ~4,300 lines) already supports runtime backend
  switching (Auto / DX12 / Vulkan / OpenGL) via `BackendChoice` and
  `apply_pending_backend_change`, which tears down and rebuilds the renderer on the same
  window.
- Present mode is hardcoded to `Fifo` (vsync), so FPS comparison between backends is
  currently meaningless (capped at refresh rate).
- Stats are a 1-second FPS average; no percentiles, no GPU timing, no recording.

## Approach (chosen: B — new modules)

New functionality lives in new focused modules; `main.rs` gets small, well-defined hooks.
No big-bang refactor of the monolith.

- `src/render_settings.rs` — settings model + wgpu mapping (BackendChoice moves here)
- `src/gpu_timer.rs` — GPU timestamp query wrapper
- `src/bench.rs` — frame stats math, sweep state machine, results records, JSON persistence

## Components

### 1. Render settings (`src/render_settings.rs`)

`RenderSettings` struct holding every knob, rendered as a settings section in the
Load Shaders sidebar:

- **Backend**: existing `BackendChoice` dropdown (moves into this module).
- **GPU adapter**: picker over adapters enumerated for the selected backend
  (Auto + each adapter by name). Stored as adapter-name match so it survives
  enumeration-order changes.
- **Present mode**: Auto / Fifo / FifoRelaxed / Mailbox / Immediate. Unsupported modes
  for the current surface are shown greyed out with "(unsupported)". Applied via
  `surface.configure()` only — no renderer rebuild.
- **Frame latency**: `desired_maximum_frame_latency` 1–3. Applied via `surface.configure()`.
- **DX12 shader compiler**: FXC vs DXC (only shown when DX12 active). Requires renderer
  rebuild. Falls back to FXC with a diagnostic if DXC unavailable.
- **Pipeline compile time**: measured around pipeline creation on every shader load,
  per pass, surfaced in diagnostics and recorded in benchmark results.

Settings that need a device rebuild reuse the existing `pending_backend_change` path,
generalized to "pending renderer rebuild". A status line shows the active config, e.g.
`Vulkan · RTX 4070 · Mailbox · latency 2`.

### 2. GPU timing (`src/gpu_timer.rs`)

- Request `wgpu::Features::TIMESTAMP_QUERY` at device creation when the adapter supports
  it; degrade to "GPU: n/a" otherwise (typical on GL).
- One query set with begin/end timestamps per shader pass (`timestamp_writes` on the
  render pass descriptor). Egui/clear passes are not timed.
- Timestamps resolve into a buffer, copied to a small ring of readback buffers with async
  map — results arrive a few frames late; no pipeline stalls.
- Output per frame: total shader GPU ms + per-pass breakdown (multipass shaders show each
  buffer pass).

### 3. Live stats upgrade

- Ring buffer of the last ~600 CPU frame times and GPU timings replaces the 1-second
  average as the source of truth.
- Stats panel shows: FPS, CPU frame avg / p95 / p99 / 1% low, GPU total ms (and per-pass
  on hover or expander), updating live as settings change.

### 4. Benchmark sweep (`src/bench.rs`)

- `Run Benchmark` button drives a state machine:
  `Idle → [per backend: Switching → Warmup → Measuring] → Restore → Done`.
- Config: warmup secs (default 3), measure secs (default 10), backend checkboxes
  (default: all available on this platform), force-uncapped toggle (default ON — forces
  Immediate present mode during the sweep, falling back Mailbox → Fifo if unsupported;
  the mode actually used is recorded per run).
- The state machine is pure logic (abstract clock, samples pushed in) so it is
  unit-testable without a GPU. `main.rs` drives it once per frame and executes the
  actions it returns (switch backend, record run, restore settings).
- Failure handling: a backend that fails to init or compile the shader is recorded as
  `error: <reason>` and the sweep continues. The sweep never crashes the app.
- After the sweep, the user's previous settings are restored.

Per run recorded: backend, adapter name, present mode actually used, render resolution,
shader name, frame count, CPU stats (avg/median/p95/p99/1% low ms), GPU stats,
pipeline compile ms.

### 5. Results table + persistence

- Side-by-side table in a Benchmark section: rows = backends, columns = key metrics,
  best value per column highlighted.
- Each sweep saved as JSON to `benchmarks/<shader>-<unix-ts>.json` (gitignored).
- Dropdown to reload a past sweep for comparison across sessions/drivers.

## Error handling

- Timestamp queries unsupported → GPU columns show "n/a"; CPU stats still recorded.
- Backend init failure during sweep → run recorded as failed, sweep continues, renderer
  restored to previous good settings at the end.
- DXC unavailable → fall back to FXC, diagnostic logged, recorded compiler noted.
- Present mode rejected by surface → fall back chain Immediate → Mailbox → Fifo.

## Testing

- Unit tests (no GPU): percentile/stats math, sweep state machine transitions,
  settings→wgpu mapping, results JSON round-trip.
- Manual verification: sweep `shaders/Test/test5.wgsl` (cheap multipass) and the
  Kerr-Newman port (heavy) on DX12 + Vulkan; check table renders and JSON lands in
  `benchmarks/`.

## Out of scope

- Engine export (still deferred per ROADMAP).
- Restructuring the rest of `main.rs`.
- Browser app (`apps/web/`) — untouched.
