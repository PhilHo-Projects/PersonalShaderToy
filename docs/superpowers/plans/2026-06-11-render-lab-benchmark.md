# Render Lab: Backend Settings + Benchmark Sweep — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose every interesting wgpu render setting (present mode, adapter, frame latency, DX12 compiler) in the native shader lab, add real GPU timing and percentile stats, and ship an automated benchmark sweep that compares backends side by side.

**Architecture:** New focused modules (`src/render_settings.rs`, `src/gpu_timer.rs`, `src/bench.rs`) hold all new logic; `src/main.rs` (4,300-line monolith, untouched structurally) gets small hooks: settings adoption in `init_renderer`, a lightweight surface-reconfigure path, timestamp writes on shader passes, and a per-frame bench-runner drive loop. Pure logic (stats math, sweep state machine, settings mapping) is unit-tested without a GPU.

**Tech Stack:** Rust 2024, wgpu 29 (`TIMESTAMP_QUERY`, `Dx12Compiler`, `backend_options`), egui 0.34, serde_json (already a dep).

**Spec:** `docs/superpowers/specs/2026-06-11-render-lab-benchmark-design.md`

---

## File structure

- Create `src/bench.rs` — `FrameStats` (percentile math), `RunRecord`/`SweepResult` (+JSON save/load), `BenchRunner` (pure state machine driven by an abstract clock).
- Create `src/render_settings.rs` — `BackendChoice` (moved verbatim from main.rs), `PresentModeChoice`, `DxCompilerChoice`, `RenderSettings` with wgpu mapping + fallback logic.
- Create `src/gpu_timer.rs` — `GpuTimer` query-set wrapper with async readback ring.
- Modify `src/main.rs` — module decls, settings adoption, UI section, stats ring buffers, bench drive loop, results table.
- Modify `Cargo.toml` — wgpu `static-dxc` feature.
- Modify `.gitignore` — add `/benchmarks/`.

Key existing anchors in `src/main.rs` (line numbers from baseline commit `fece7bd`):
- `BackendChoice` enum + impl: lines 323–412
- `RendererState`: line 694; `PreviewApp`: line 729 (`requested_backend` 732, `pending_backend_change` 783)
- `init_renderer`: 1381 (instance 1410–1413, adapter pick 1418–1435, device 1438, config 1463–1473)
- settings UI column (Aspect/Scale/Backend combos): 2400–2474
- stats display: 2246–2266
- shader passes: single 2919, multi 3066; submit 3108
- frame-count/stats emit: 3356–3372
- `apply_pending_backend_change`: 3378–3431
- `build_single_pass` 2050, `build_multi_pass` 2074, `create_pipeline` 3590

---

### Task 1: `bench.rs` — FrameStats percentile math

**Files:**
- Create: `src/bench.rs`
- Modify: `src/main.rs` (add `mod bench;` near top, after the `use` block)

- [ ] **Step 1: Write the failing tests** (bottom of new `src/bench.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_stats_empty_is_none() {
        assert!(FrameStats::from_samples_ms(&[]).is_none());
    }

    #[test]
    fn frame_stats_single_sample() {
        let s = FrameStats::from_samples_ms(&[16.0]).unwrap();
        assert_eq!(s.count, 1);
        assert_eq!(s.avg_ms, 16.0);
        assert_eq!(s.median_ms, 16.0);
        assert_eq!(s.p95_ms, 16.0);
        assert_eq!(s.p99_ms, 16.0);
        assert_eq!(s.low_1pct_ms, 16.0);
    }

    #[test]
    fn frame_stats_uniform_1_to_200() {
        let samples: Vec<f64> = (1..=200).map(|v| v as f64).collect();
        let s = FrameStats::from_samples_ms(&samples).unwrap();
        assert_eq!(s.count, 200);
        assert!((s.avg_ms - 100.5).abs() < 1e-9);
        assert!((s.median_ms - 100.5).abs() < 1e-9);
        assert_eq!(s.p95_ms, 190.0);
        assert_eq!(s.p99_ms, 198.0);
        // slowest 1% of 200 = 2 samples: (199+200)/2
        assert!((s.low_1pct_ms - 199.5).abs() < 1e-9);
        assert_eq!(s.min_ms, 1.0);
        assert_eq!(s.max_ms, 200.0);
    }

    #[test]
    fn frame_stats_fps_helpers() {
        let s = FrameStats::from_samples_ms(&[10.0, 10.0]).unwrap();
        assert!((s.avg_fps() - 100.0).abs() < 1e-9);
        assert!((s.low_1pct_fps() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn frame_stats_unsorted_input() {
        let s = FrameStats::from_samples_ms(&[30.0, 10.0, 20.0]).unwrap();
        assert_eq!(s.median_ms, 20.0);
        assert_eq!(s.min_ms, 10.0);
        assert_eq!(s.max_ms, 30.0);
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test --bin personal-shadertoy bench::` → FAIL (FrameStats not defined).

- [ ] **Step 3: Implement** (top of `src/bench.rs`)

```rust
//! Benchmark support: frame statistics, sweep state machine, results persistence.
//! Pure logic — no GPU types — so everything here is unit-testable.

use serde::{Deserialize, Serialize};

/// Aggregate statistics over a set of frame-time samples, in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameStats {
    pub count: u32,
    pub avg_ms: f64,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    /// Average of the slowest 1% of frames (the "1% low" in FPS terms).
    pub low_1pct_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

impl FrameStats {
    pub fn from_samples_ms(samples: &[f64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<f64> = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = sorted.len();
        let avg = sorted.iter().sum::<f64>() / n as f64;
        let median = if n % 2 == 0 {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
        } else {
            sorted[n / 2]
        };
        let pct = |q: f64| -> f64 {
            let idx = ((q * n as f64).ceil() as usize).clamp(1, n) - 1;
            sorted[idx]
        };
        let worst_count = (n / 100).max(1);
        let low_1pct =
            sorted[n - worst_count..].iter().sum::<f64>() / worst_count as f64;
        Some(FrameStats {
            count: n as u32,
            avg_ms: avg,
            median_ms: median,
            p95_ms: pct(0.95),
            p99_ms: pct(0.99),
            low_1pct_ms: low_1pct,
            min_ms: sorted[0],
            max_ms: sorted[n - 1],
        })
    }

    pub fn avg_fps(&self) -> f64 {
        if self.avg_ms > 0.0 { 1000.0 / self.avg_ms } else { 0.0 }
    }

    pub fn low_1pct_fps(&self) -> f64 {
        if self.low_1pct_ms > 0.0 { 1000.0 / self.low_1pct_ms } else { 0.0 }
    }
}
```

In `src/main.rs`, after the existing `use` statements (around line 40), add:

```rust
mod bench;
```

- [ ] **Step 4: Run tests** — `cargo test --bin personal-shadertoy bench::` → all PASS. Expect dead-code warnings (not yet referenced); acceptable until Task 7/8 wire it in — silence with `#![allow(dead_code)]`? No: add `#[allow(dead_code)]` nothing; warnings are fine for intermediate commits, but keep `cargo test` green.

- [ ] **Step 5: Commit** — `git add src/bench.rs src/main.rs && git commit -m "feat(bench): frame statistics with percentiles and 1% lows"`

---

### Task 2: `bench.rs` — results records + JSON persistence

**Files:**
- Modify: `src/bench.rs`

- [ ] **Step 1: Failing tests** (append to tests module)

```rust
    #[test]
    fn sweep_result_json_round_trip() {
        let result = SweepResult {
            shader: "test5.wgsl".into(),
            timestamp_unix: 1_750_000_000,
            warmup_secs: 3.0,
            measure_secs: 10.0,
            runs: vec![RunRecord {
                backend: "Vulkan".into(),
                adapter: "Test GPU".into(),
                present_mode: "Immediate".into(),
                resolution: [1280, 720],
                frames: 1200,
                cpu: FrameStats::from_samples_ms(&[8.0, 9.0, 10.0]),
                gpu: None,
                pipeline_compile_ms: Some(42.5),
                error: None,
            }],
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let back: SweepResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.runs.len(), 1);
        assert_eq!(back.runs[0].backend, "Vulkan");
        assert_eq!(back.runs[0].cpu.as_ref().unwrap().count, 3);
    }

    #[test]
    fn sweep_filename_slugs_shader_name() {
        assert_eq!(
            sweep_filename("Kerr Newman (v2).glsl", 123),
            "kerr-newman-v2-glsl-123.json"
        );
    }

    #[test]
    fn save_and_load_sweep() {
        let dir = std::env::temp_dir().join(format!("pst-bench-{}", std::process::id()));
        let result = SweepResult {
            shader: "t.wgsl".into(),
            timestamp_unix: 42,
            warmup_secs: 1.0,
            measure_secs: 2.0,
            runs: vec![],
        };
        let path = save_sweep(&result, &dir).unwrap();
        let loaded = load_sweep(&path).unwrap();
        assert_eq!(loaded.timestamp_unix, 42);
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 2: Verify fail** — `cargo test --bin personal-shadertoy bench::` → FAIL (types not defined).

- [ ] **Step 3: Implement** (append after `FrameStats` impl)

```rust
/// One backend's benchmark run inside a sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub backend: String,
    pub adapter: String,
    /// Present mode actually used (after fallback), not necessarily the requested one.
    pub present_mode: String,
    pub resolution: [u32; 2],
    pub frames: u32,
    pub cpu: Option<FrameStats>,
    pub gpu: Option<FrameStats>,
    pub pipeline_compile_ms: Option<f64>,
    /// Set when the backend failed to init or compile; stats fields are None.
    pub error: Option<String>,
}

/// A full benchmark sweep over multiple backends for one shader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepResult {
    pub shader: String,
    pub timestamp_unix: u64,
    pub warmup_secs: f64,
    pub measure_secs: f64,
    pub runs: Vec<RunRecord>,
}

pub fn sweep_filename(shader: &str, timestamp_unix: u64) -> String {
    let slug: String = shader
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let mut collapsed = String::with_capacity(slug.len());
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push(c);
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    format!("{collapsed}-{timestamp_unix}.json")
}

pub fn save_sweep(result: &SweepResult, dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(sweep_filename(&result.shader, result.timestamp_unix));
    let json = serde_json::to_string_pretty(result).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn load_sweep(path: &std::path::Path) -> Result<SweepResult, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Saved sweeps in `dir`, newest first (by file name's trailing unix timestamp).
pub fn list_sweeps(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    files
}
```

- [ ] **Step 4: Run tests** — `cargo test --bin personal-shadertoy bench::` → PASS.

- [ ] **Step 5: Add `/benchmarks/` to `.gitignore`** (new line at end).

- [ ] **Step 6: Commit** — `git add src/bench.rs .gitignore && git commit -m "feat(bench): sweep result records with JSON persistence"`

---

### Task 3: `bench.rs` — BenchRunner state machine

Pure logic, abstract `now: f64` seconds clock. Protocol with the host (main.rs):

1. Host creates `BenchRunner::new(config)`.
2. Each frame host calls `next_action()` and executes:
   - `SwitchBackend { label }` → host queues a renderer rebuild on that backend (with forced present mode if configured), then later calls `backend_ready(...)` (after the shader compiled on the new device) or `backend_failed(...)`.
   - `Finished` → host calls `take_result(shader)`, saves it, restores user settings.
   - `None` → nothing.
3. Host calls `record_frame(now, cpu_ms, gpu_ms)` once per rendered frame; the runner ignores samples outside the Measuring phase and handles Warmup→Measuring transition itself.

**Files:**
- Modify: `src/bench.rs`

- [ ] **Step 1: Failing tests** (append; these define the full contract)

```rust
    fn cfg2() -> BenchConfig {
        BenchConfig {
            warmup_secs: 1.0,
            measure_secs: 2.0,
            backend_labels: vec!["DirectX 12".into(), "Vulkan".into()],
            force_uncapped: true,
        }
    }

    #[test]
    fn runner_happy_path_two_backends() {
        let mut r = BenchRunner::new(cfg2());
        assert!(r.is_active());

        // First action: switch to first backend. Returned exactly once.
        let BenchAction::SwitchBackend { label } = r.next_action() else { panic!() };
        assert_eq!(label, "DirectX 12");
        assert!(matches!(r.next_action(), BenchAction::None));

        r.backend_ready(0.0, "Dx12".into(), "GPU A".into(), "Immediate".into(), [800, 600], Some(10.0));
        // Warmup frames (t < 1.0) are not counted.
        r.record_frame(0.5, 4.0, Some(2.0));
        // Crossing into measure window.
        r.record_frame(1.1, 8.0, Some(4.0));
        r.record_frame(1.6, 8.0, None);
        // Crossing past measure end finalizes the run.
        r.record_frame(3.1, 8.0, Some(4.0));

        let BenchAction::SwitchBackend { label } = r.next_action() else { panic!() };
        assert_eq!(label, "Vulkan");
        r.backend_ready(4.0, "Vulkan".into(), "GPU A".into(), "Mailbox".into(), [800, 600], None);
        r.record_frame(5.1, 5.0, None);
        r.record_frame(7.2, 5.0, None);

        assert!(matches!(r.next_action(), BenchAction::Finished));
        let result = r.take_result("shader.wgsl".into()).unwrap();
        assert_eq!(result.runs.len(), 2);
        let dx = &result.runs[0];
        assert_eq!(dx.backend, "Dx12");
        assert_eq!(dx.present_mode, "Immediate");
        assert_eq!(dx.frames, 3);
        assert_eq!(dx.cpu.as_ref().unwrap().count, 3);
        // GPU samples: only 2 of the 3 measured frames had Some.
        assert_eq!(dx.gpu.as_ref().unwrap().count, 2);
        assert_eq!(dx.pipeline_compile_ms, Some(10.0));
        assert!(dx.error.is_none());
        assert_eq!(result.runs[1].backend, "Vulkan");
        assert!(!r.is_active());
    }

    #[test]
    fn runner_backend_failure_continues_sweep() {
        let mut r = BenchRunner::new(cfg2());
        let BenchAction::SwitchBackend { .. } = r.next_action() else { panic!() };
        r.backend_failed("no adapter".into());

        let BenchAction::SwitchBackend { label } = r.next_action() else { panic!() };
        assert_eq!(label, "Vulkan");
        r.backend_ready(0.0, "Vulkan".into(), "GPU".into(), "Fifo".into(), [64, 64], None);
        r.record_frame(1.5, 5.0, None);
        r.record_frame(3.5, 5.0, None);

        assert!(matches!(r.next_action(), BenchAction::Finished));
        let result = r.take_result("s".into()).unwrap();
        assert_eq!(result.runs[0].error.as_deref(), Some("no adapter"));
        assert!(result.runs[0].cpu.is_none());
        assert!(result.runs[1].error.is_none());
    }

    #[test]
    fn runner_status_line_mentions_phase() {
        let mut r = BenchRunner::new(cfg2());
        let _ = r.next_action();
        assert!(r.status_line().contains("DirectX 12"));
        r.backend_ready(0.0, "Dx12".into(), "G".into(), "Fifo".into(), [1, 1], None);
        r.record_frame(0.2, 1.0, None);
        assert!(r.status_line().to_lowercase().contains("warmup"));
        r.record_frame(1.2, 1.0, None);
        assert!(r.status_line().to_lowercase().contains("measur"));
    }
```

- [ ] **Step 2: Verify fail** — `cargo test --bin personal-shadertoy bench::` → FAIL.

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub warmup_secs: f64,
    pub measure_secs: f64,
    /// UI labels of the backends to sweep, in order (BackendChoice::label()).
    pub backend_labels: Vec<String>,
    /// Force Immediate (fallback Mailbox→Fifo) present mode during the sweep.
    pub force_uncapped: bool,
}

#[derive(Debug)]
pub enum BenchAction {
    None,
    SwitchBackend { label: String },
    Finished,
}

#[derive(Debug)]
enum Phase {
    /// Next backend switch not yet handed to the host.
    PendingSwitch(usize),
    /// Switch handed out; waiting for backend_ready / backend_failed.
    WaitingReady(usize),
    Warmup {
        index: usize,
        started: f64,
        meta: RunMeta,
    },
    Measuring {
        index: usize,
        started: f64,
        meta: RunMeta,
        cpu_ms: Vec<f64>,
        gpu_ms: Vec<f64>,
        frames: u32,
    },
    /// All backends processed; Finished not yet handed to the host.
    PendingFinish,
    Done,
}

#[derive(Debug, Clone)]
struct RunMeta {
    backend: String,
    adapter: String,
    present_mode: String,
    resolution: [u32; 2],
    pipeline_compile_ms: Option<f64>,
}

pub struct BenchRunner {
    config: BenchConfig,
    phase: Phase,
    runs: Vec<RunRecord>,
}

impl BenchRunner {
    pub fn new(config: BenchConfig) -> Self {
        let phase = if config.backend_labels.is_empty() {
            Phase::PendingFinish
        } else {
            Phase::PendingSwitch(0)
        };
        Self { config, phase, runs: Vec::new() }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Done)
    }

    pub fn config(&self) -> &BenchConfig {
        &self.config
    }

    /// Poll once per frame; transitions internal one-shot states.
    pub fn next_action(&mut self) -> BenchAction {
        match &self.phase {
            Phase::PendingSwitch(i) => {
                let label = self.config.backend_labels[*i].clone();
                self.phase = Phase::WaitingReady(*i);
                BenchAction::SwitchBackend { label }
            }
            Phase::PendingFinish => BenchAction::Finished,
            _ => BenchAction::None,
        }
    }

    pub fn backend_ready(
        &mut self,
        now: f64,
        backend: String,
        adapter: String,
        present_mode: String,
        resolution: [u32; 2],
        pipeline_compile_ms: Option<f64>,
    ) {
        if let Phase::WaitingReady(i) = self.phase {
            self.phase = Phase::Warmup {
                index: i,
                started: now,
                meta: RunMeta { backend, adapter, present_mode, resolution, pipeline_compile_ms },
            };
        }
    }

    pub fn backend_failed(&mut self, reason: String) {
        if let Phase::WaitingReady(i) = self.phase {
            let label = self.config.backend_labels[i].clone();
            self.runs.push(RunRecord {
                backend: label,
                adapter: String::new(),
                present_mode: String::new(),
                resolution: [0, 0],
                frames: 0,
                cpu: None,
                gpu: None,
                pipeline_compile_ms: None,
                error: Some(reason),
            });
            self.advance(i);
        }
    }

    pub fn record_frame(&mut self, now: f64, cpu_ms: f64, gpu_ms: Option<f64>) {
        match &mut self.phase {
            Phase::Warmup { index, started, meta } => {
                if now - *started >= self.config.warmup_secs {
                    self.phase = Phase::Measuring {
                        index: *index,
                        started: now,
                        meta: meta.clone(),
                        cpu_ms: Vec::new(),
                        gpu_ms: Vec::new(),
                        frames: 0,
                    };
                }
            }
            Phase::Measuring { index, started, meta, cpu_ms: cpu, gpu_ms: gpu, frames } => {
                if now - *started >= self.config.measure_secs {
                    let record = RunRecord {
                        backend: meta.backend.clone(),
                        adapter: meta.adapter.clone(),
                        present_mode: meta.present_mode.clone(),
                        resolution: meta.resolution,
                        frames: *frames,
                        cpu: FrameStats::from_samples_ms(cpu),
                        gpu: FrameStats::from_samples_ms(gpu),
                        pipeline_compile_ms: meta.pipeline_compile_ms,
                        error: None,
                    };
                    let i = *index;
                    self.runs.push(record);
                    self.advance(i);
                } else {
                    cpu.push(cpu_ms);
                    if let Some(g) = gpu_ms {
                        gpu.push(g);
                    }
                    *frames += 1;
                }
            }
            _ => {}
        }
    }

    pub fn take_result(&mut self, shader: String) -> Option<SweepResult> {
        if !matches!(self.phase, Phase::PendingFinish) {
            return None;
        }
        self.phase = Phase::Done;
        Some(SweepResult {
            shader,
            timestamp_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            warmup_secs: self.config.warmup_secs,
            measure_secs: self.config.measure_secs,
            runs: std::mem::take(&mut self.runs),
        })
    }

    pub fn status_line(&self) -> String {
        match &self.phase {
            Phase::PendingSwitch(i) | Phase::WaitingReady(i) => {
                format!("Benchmark: switching to {}…", self.config.backend_labels[*i])
            }
            Phase::Warmup { index, .. } => {
                format!("Benchmark: {} warmup…", self.config.backend_labels[*index])
            }
            Phase::Measuring { index, frames, .. } => {
                format!(
                    "Benchmark: measuring {} ({} frames)…",
                    self.config.backend_labels[*index], frames
                )
            }
            Phase::PendingFinish => "Benchmark: finishing…".to_string(),
            Phase::Done => "Benchmark: done".to_string(),
        }
    }

    fn advance(&mut self, finished_index: usize) {
        let next = finished_index + 1;
        self.phase = if next < self.config.backend_labels.len() {
            Phase::PendingSwitch(next)
        } else {
            Phase::PendingFinish
        };
    }
}
```

Note: the happy-path test calls `take_result` after `next_action()` returned `Finished`; `next_action` leaves the phase at `PendingFinish` (Finished may be returned repeatedly until `take_result` flips to Done). That is intentional so the host can't miss it.

- [ ] **Step 4: Run tests** — `cargo test --bin personal-shadertoy bench::` → PASS. Also run full `cargo test` → everything green.

- [ ] **Step 5: Commit** — `git add src/bench.rs && git commit -m "feat(bench): sweep state machine with warmup/measure phases"`

---

### Task 4: `render_settings.rs` — settings model + wgpu mapping

**Files:**
- Create: `src/render_settings.rs`
- Modify: `src/main.rs` — delete `BackendChoice` (lines 323–412), add `mod render_settings; use render_settings::BackendChoice;`

- [ ] **Step 1: Move `BackendChoice`** verbatim from `src/main.rs:323-412` into `src/render_settings.rs` with `pub` on the enum, its variants stay as-is, and `pub` on `label/ui_choices/parse/to_wgpu/preferred_backend_order`. In main.rs add `mod render_settings;` + `use render_settings::BackendChoice;`. Run `cargo check` → green before continuing.

- [ ] **Step 2: Failing tests** (bottom of `src/render_settings.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_mode_auto_resolves_to_fifo() {
        let s = RenderSettings::default();
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn present_mode_supported_choice_is_used() {
        let mut s = RenderSettings::default();
        s.present_mode = PresentModeChoice::Mailbox;
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Mailbox
        );
    }

    #[test]
    fn present_mode_unsupported_falls_back_in_order() {
        let mut s = RenderSettings::default();
        s.present_mode = PresentModeChoice::Immediate;
        // Immediate unsupported, Mailbox supported → Mailbox.
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Mailbox
        );
        // Only Fifo supported → Fifo.
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo]),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn rebuild_required_only_for_device_level_changes() {
        let a = RenderSettings::default();

        let mut present_only = a.clone();
        present_only.present_mode = PresentModeChoice::Immediate;
        present_only.frame_latency = 3;
        assert!(!present_only.requires_renderer_rebuild(&a));

        let mut backend = a.clone();
        backend.backend = BackendChoice::Vulkan;
        assert!(backend.requires_renderer_rebuild(&a));

        let mut adapter = a.clone();
        adapter.adapter_name = Some("Radeon".into());
        assert!(adapter.requires_renderer_rebuild(&a));

        let mut compiler = a.clone();
        compiler.dx12_compiler = DxCompilerChoice::StaticDxc;
        assert!(compiler.requires_renderer_rebuild(&a));
    }
}
```

- [ ] **Step 3: Verify fail** — `cargo test --bin personal-shadertoy render_settings::` → FAIL.

- [ ] **Step 4: Implement** (in `src/render_settings.rs`, above tests)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentModeChoice {
    Auto,
    Fifo,
    FifoRelaxed,
    Mailbox,
    Immediate,
}

impl PresentModeChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Fifo)",
            Self::Fifo => "Fifo (vsync)",
            Self::FifoRelaxed => "FifoRelaxed",
            Self::Mailbox => "Mailbox (fast vsync)",
            Self::Immediate => "Immediate (uncapped)",
        }
    }

    pub fn ui_choices() -> &'static [Self] {
        &[Self::Auto, Self::Fifo, Self::FifoRelaxed, Self::Mailbox, Self::Immediate]
    }

    pub fn to_wgpu(self) -> Option<wgpu::PresentMode> {
        match self {
            Self::Auto => None,
            Self::Fifo => Some(wgpu::PresentMode::Fifo),
            Self::FifoRelaxed => Some(wgpu::PresentMode::FifoRelaxed),
            Self::Mailbox => Some(wgpu::PresentMode::Mailbox),
            Self::Immediate => Some(wgpu::PresentMode::Immediate),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DxCompilerChoice {
    /// wgpu picks: static DXC if linked, then dynamic, then FXC.
    Auto,
    Fxc,
    StaticDxc,
}

impl DxCompilerChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Fxc => "FXC (legacy)",
            Self::StaticDxc => "DXC (modern)",
        }
    }

    pub fn ui_choices() -> &'static [Self] {
        &[Self::Auto, Self::Fxc, Self::StaticDxc]
    }

    pub fn to_wgpu(self) -> wgpu::Dx12Compiler {
        match self {
            Self::Auto => wgpu::Dx12Compiler::Auto,
            Self::Fxc => wgpu::Dx12Compiler::Fxc,
            Self::StaticDxc => wgpu::Dx12Compiler::StaticDxc,
        }
    }
}

/// Every renderer-affecting knob in one place. Present mode and frame latency
/// apply via surface reconfigure; the rest require a full renderer rebuild.
#[derive(Clone, PartialEq, Debug)]
pub struct RenderSettings {
    pub backend: BackendChoice,
    /// None = automatic adapter selection; Some(name) = match by adapter name.
    pub adapter_name: Option<String>,
    pub present_mode: PresentModeChoice,
    /// desired_maximum_frame_latency, clamped 1..=3 by the UI.
    pub frame_latency: u32,
    pub dx12_compiler: DxCompilerChoice,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            backend: BackendChoice::Auto,
            adapter_name: None,
            present_mode: PresentModeChoice::Auto,
            frame_latency: 2,
            dx12_compiler: DxCompilerChoice::Auto,
        }
    }
}

impl RenderSettings {
    /// Resolve the requested present mode against what the surface supports,
    /// falling back Immediate → Mailbox → FifoRelaxed → Fifo.
    pub fn resolve_present_mode(&self, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
        let requested = match self.present_mode.to_wgpu() {
            None => return wgpu::PresentMode::Fifo,
            Some(mode) => mode,
        };
        if supported.contains(&requested) {
            return requested;
        }
        for fallback in [
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::FifoRelaxed,
            wgpu::PresentMode::Fifo,
        ] {
            if supported.contains(&fallback) {
                return fallback;
            }
        }
        wgpu::PresentMode::Fifo
    }

    /// True when switching from `previous` to `self` needs a device/instance
    /// rebuild (backend, adapter, or DX12 compiler changed) rather than just a
    /// surface reconfigure.
    pub fn requires_renderer_rebuild(&self, previous: &RenderSettings) -> bool {
        self.backend != previous.backend
            || self.adapter_name != previous.adapter_name
            || self.dx12_compiler != previous.dx12_compiler
    }
}
```

- [ ] **Step 5: Enable static DXC in `Cargo.toml`** — change the wgpu line to:

```toml
wgpu = { version = "29.0", features = ["static-dxc"] }
```

- [ ] **Step 6: Run** — `cargo test --bin personal-shadertoy render_settings::` → PASS; `cargo check` → green (expect unused warnings for not-yet-wired types).

- [ ] **Step 7: Commit** — `git add src/render_settings.rs src/main.rs Cargo.toml Cargo.lock && git commit -m "feat(settings): render settings model with present-mode fallback and DX12 compiler choice"`

---

### Task 5: main.rs — adopt RenderSettings (settings UI, surface reconfigure, adapter picker, compile timing)

**Files:**
- Modify: `src/main.rs` only. Verify with `cargo check` after each numbered step; full manual run at the end.

- [ ] **Step 1: Replace backend fields on `PreviewApp`** (struct at line 729, constructor at 1107):
  - `requested_backend: BackendChoice` → `settings: render_settings::RenderSettings` (init from `BackendChoice::parse(...)` CLI arg: `RenderSettings { backend: parsed, ..Default::default() }`; `PreviewApp::new` signature changes from `new(requested_backend: BackendChoice, ...)` to `new(initial_settings: RenderSettings, ...)`; update the call in `main()`).
  - `pending_backend_change: Option<BackendChoice>` → `pending_rebuild: Option<render_settings::RenderSettings>`.
  - Add fields:

```rust
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
```

- [ ] **Step 2: `init_renderer` honors settings** (lines 1410–1473):
  - Instance: after `instance_desc.backends = ...`, add `instance_desc.backend_options.dx12.shader_compiler = self.settings.dx12_compiler.to_wgpu();`
  - Adapter pick: after enumerating, populate `self.available_adapters` with names of surface-supported adapters. If `self.settings.adapter_name` is `Some(name)`, prefer the surface-supported adapter with that exact name (then fall through to existing preferred-order logic if absent, pushing a Warning diagnostic "Adapter '<name>' not found, using auto selection").
  - Config: `self.supported_present_modes = capabilities.present_modes.clone();` then
    `present_mode: self.settings.resolve_present_mode(&capabilities.present_modes)` and
    `desired_maximum_frame_latency: self.settings.frame_latency`. Store `self.active_present_mode = config.present_mode;`
  - Device features: request timestamp support when available (used by Task 6):

```rust
        let adapter_features = adapter.features();
        let mut required_features = wgpu::Features::empty();
        if adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
        }
```

  (pass `required_features` in the `DeviceDescriptor`).

- [ ] **Step 3: Generalize the rebuild path** (3378–3431): rename `apply_pending_backend_change` → `apply_pending_rebuild`; it takes `self.pending_rebuild`, saves `let previous = self.settings.clone()`, sets `self.settings = new_settings`, drops the renderer, calls `init_renderer`, reverts to `previous` on failure. Update the call site (search for `apply_pending_backend_change(` — it is called from `window_event`/`about_to_wait` RedrawRequested handling).

- [ ] **Step 4: Lightweight surface reconfigure** — at the top of `render()` (before `get_current_texture` usage, i.e. right after the `let Some(r) = self.renderer.as_mut()` at 2189 — but the flag is applied where renderer is mutably available; simplest: immediately after `process_compile_updates()` at 2157):

```rust
        if self.surface_reconfigure_needed {
            if let Some(r) = self.renderer.as_mut() {
                r.config.present_mode =
                    self.settings.resolve_present_mode(&self.supported_present_modes);
                r.config.desired_maximum_frame_latency = self.settings.frame_latency;
                r.surface.configure(&r.device, &r.config);
                self.active_present_mode = r.config.present_mode;
            }
            self.surface_reconfigure_needed = false;
        }
```

- [ ] **Step 5: Settings UI** — in the settings column (after the Backend combo at 2443–2460), using locals mirrored at the top of `render()` (follow the existing `selected_backend`/`previous_backend` pattern: add `let mut ui_settings = self.settings.clone(); let previous_settings = self.settings.clone();` and replace the existing `selected_backend` locals with `ui_settings.backend`):
  - **Adapter combo**: "Auto" + one entry per `self.available_adapters` name; sets `ui_settings.adapter_name`.
  - **Present mode combo**: iterate `PresentModeChoice::ui_choices()`; for non-Auto choices, disable entries whose `to_wgpu().unwrap()` is not in `supported_present_modes`:

```rust
        for choice in render_settings::PresentModeChoice::ui_choices() {
            let supported = match choice.to_wgpu() {
                None => true,
                Some(mode) => supported_present_modes.contains(&mode),
            };
            ui.add_enabled_ui(supported, |ui| {
                let text = if supported {
                    choice.label().to_string()
                } else {
                    format!("{} (unsupported)", choice.label())
                };
                ui.selectable_value(&mut ui_settings.present_mode, *choice, text);
            });
        }
```

  - **Frame latency combo**: values 1, 2, 3 into `ui_settings.frame_latency`.
  - **DX12 compiler combo**: only rendered when `active_backend_name == "Dx12"`; iterates `DxCompilerChoice::ui_choices()`.
  - Status line under the combos: `format!("{} · {} · {:?} · latency {}", active_backend_name, active_adapter_name, active_present_mode, frame_latency)` (pass the needed values in as locals, same pattern as `active_backend_name` at 2186).
- [ ] **Step 6: Apply settings changes** — replace the `selected_backend != previous_backend` block (3206–3214) with:

```rust
        if ui_settings != previous_settings {
            if ui_settings.requires_renderer_rebuild(&previous_settings) {
                self.pending_rebuild = Some(ui_settings.clone());
                self.push_diagnostic(
                    DiagLevel::Info,
                    format!(
                        "Renderer rebuild requested: {} → {}.",
                        previous_settings.backend.label(),
                        ui_settings.backend.label()
                    ),
                );
            } else {
                self.settings = ui_settings.clone();
                self.surface_reconfigure_needed = true;
            }
        }
```

- [ ] **Step 7: Pipeline compile timing** — in `build_single_pass` (2050) and `build_multi_pass` (2074), wrap each `create_pipeline(...)` call:

```rust
        let pipeline_start = std::time::Instant::now();
        // ... existing create_pipeline call ...
        let compile_ms = pipeline_start.elapsed().as_secs_f64() * 1000.0;
```

  Clear `self.pipeline_compile_ms` at the start of each build, push `(pass_name, compile_ms)` per pipeline ("Image" for single pass), and after a successful build push one Info diagnostic: `"Pipelines built in {total:.1} ms (FXC/DXC per settings)"` with per-pass values when multipass.

- [ ] **Step 8: Verify** — `cargo check` green, `cargo test` green, then `cargo run --bin personal-shadertoy`:
  - load `shaders/Test/test5.wgsl`; flip Present Mode to Immediate → FPS should exceed monitor refresh; flip back to Fifo → capped again.
  - switch Backend DX12 ↔ Vulkan → still works; adapter combo lists your GPU(s); DX12 compiler combo appears only on DX12.

- [ ] **Step 9: Commit** — `git add src/main.rs && git commit -m "feat(settings): adapter picker, present mode, frame latency, DX12 compiler in native UI"`

---

### Task 6: `gpu_timer.rs` — GPU timestamp queries

**Files:**
- Create: `src/gpu_timer.rs`
- Modify: `src/main.rs` — `mod gpu_timer;`, field on `RendererState`, hooks in `init_renderer` and `render()`

- [ ] **Step 1: Implement `GpuTimer`** (GPU-coupled; no unit tests — verified by running):

```rust
//! GPU pass timing via wgpu timestamp queries with a non-blocking readback ring.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const MAX_PASSES: u32 = 16;
const RING_SIZE: usize = 4;

/// GPU timings for one frame, in pass-encoding order.
#[derive(Debug, Clone)]
pub struct FrameGpuTiming {
    pub pass_ms: Vec<f64>,
    pub total_ms: f64,
}

pub struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve_buf: wgpu::Buffer,
    readback: Vec<wgpu::Buffer>,
    /// Slots currently mapped or queued for mapping: (slot, pass_count).
    in_flight: VecDeque<(usize, u32)>,
    free_slots: Vec<usize>,
    completed: Arc<Mutex<Vec<(usize, Vec<u64>)>>>,
    period_ns: f32,
    passes_this_frame: u32,
    latest: Option<FrameGpuTiming>,
}

impl GpuTimer {
    /// Returns None when the device lacks TIMESTAMP_QUERY (e.g. most GL).
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu-timer-queries"),
            ty: wgpu::QueryType::Timestamp,
            count: MAX_PASSES * 2,
        });
        let resolve_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-timer-resolve"),
            size: (MAX_PASSES as u64 * 2) * 8,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = (0..RING_SIZE)
            .map(|i| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("gpu-timer-readback-{i}")),
                    size: (MAX_PASSES as u64 * 2) * 8,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Some(Self {
            query_set,
            resolve_buf,
            readback,
            in_flight: VecDeque::new(),
            free_slots: (0..RING_SIZE).collect(),
            completed: Arc::new(Mutex::new(Vec::new())),
            period_ns: queue.get_timestamp_period(),
            passes_this_frame: 0,
            latest: None,
        })
    }

    /// Harvest finished readbacks and reset per-frame pass counter.
    pub fn begin_frame(&mut self) {
        self.passes_this_frame = 0;
        let mut done = self.completed.lock().unwrap();
        for (slot, raw) in done.drain(..) {
            // Find matching in-flight entry for the pass count.
            if let Some(pos) = self.in_flight.iter().position(|(s, _)| *s == slot) {
                let (_, pass_count) = self.in_flight.remove(pos).unwrap();
                let mut pass_ms = Vec::with_capacity(pass_count as usize);
                for p in 0..pass_count as usize {
                    let begin = raw[p * 2];
                    let end = raw[p * 2 + 1];
                    let delta_ns = end.saturating_sub(begin) as f64 * self.period_ns as f64;
                    pass_ms.push(delta_ns / 1_000_000.0);
                }
                let total_ms = pass_ms.iter().sum();
                self.latest = Some(FrameGpuTiming { pass_ms, total_ms });
                self.free_slots.push(slot);
            }
        }
    }

    /// Allocate begin/end query indices for the next shader pass this frame.
    pub fn pass_timestamp_writes(&mut self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if self.passes_this_frame >= MAX_PASSES {
            return None;
        }
        let base = self.passes_this_frame * 2;
        self.passes_this_frame += 1;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(base),
            end_of_pass_write_index: Some(base + 1),
        })
    }

    /// Encode resolve + copy into a free readback slot. Call after the shader
    /// passes, before submit. Skips silently when the ring is saturated.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<usize> {
        if self.passes_this_frame == 0 {
            return None;
        }
        let slot = self.free_slots.pop()?;
        let count = self.passes_this_frame * 2;
        encoder.resolve_query_set(&self.query_set, 0..count, &self.resolve_buf, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve_buf,
            0,
            &self.readback[slot],
            0,
            count as u64 * 8,
        );
        self.in_flight.push_back((slot, self.passes_this_frame));
        Some(slot)
    }

    /// Kick off the async map for the slot returned by `resolve`. Call after
    /// `queue.submit`.
    pub fn after_submit(&mut self, slot: usize) {
        let pass_count = self
            .in_flight
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        if pass_count == 0 {
            return;
        }
        let byte_len = (pass_count as u64 * 2) * 8;
        let buffer = self.readback[slot].clone();
        let completed = self.completed.clone();
        let buffer_for_read = self.readback[slot].clone();
        buffer.slice(0..byte_len).map_async(wgpu::MapMode::Read, move |result| {
            if result.is_ok() {
                let raw: Vec<u64> = {
                    let view = buffer_for_read.slice(0..byte_len).get_mapped_range();
                    bytemuck::cast_slice::<u8, u64>(&view).to_vec()
                };
                buffer_for_read.unmap();
                completed.lock().unwrap().push((slot, raw));
            }
        });
    }

    pub fn latest(&self) -> Option<&FrameGpuTiming> {
        self.latest.as_ref()
    }
}
```

  Note: `wgpu::Buffer` is `Clone` in wgpu 29 (internally ref-counted). If `cargo check` disagrees, wrap the buffers in `Arc` instead. If a mapping error path leaves a slot stranded, it stays in `in_flight` harmlessly (3 remaining slots keep working); acceptable for a diagnostics feature.

- [ ] **Step 2: Wire into `RendererState`** — add field `gpu_timer: Option<gpu_timer::GpuTimer>`; in `init_renderer` after device creation: `let gpu_timer = gpu_timer::GpuTimer::new(&device, &queue);`, store it; add a diagnostic when `None`: "GPU timestamps unavailable on this backend (CPU timing only)".

- [ ] **Step 3: Hook the render loop** in `render()`:
  - Right after the `let Some(r) = self.renderer.as_mut()` (line 2189): `if let Some(t) = r.gpu_timer.as_mut() { t.begin_frame(); }`
  - Single-pass descriptor (2919) and multi-pass descriptor (3066): replace `..Default::default()` with

```rust
                            timestamp_writes: r
                                .gpu_timer
                                .as_mut()
                                .and_then(|t| t.pass_timestamp_writes()),
                            ..Default::default()
```

  ⚠️ Borrow check: at those points `r.mode` is borrowed (`match &r.mode`). `gpu_timer` is a sibling field so a disjoint borrow is fine **only** through `r` directly; if the borrow checker objects because of the closure structure, hoist `let mut timer = r.gpu_timer.take();` before the match and restore `r.gpu_timer = timer;` after the submit block, calling methods on the local.
  - Before `r.queue.submit(Some(encoder.finish()))` (3108): `let timer_slot = r.gpu_timer.as_mut().and_then(|t| t.resolve(&mut encoder));`
  - After that submit: `if let (Some(t), Some(slot)) = (r.gpu_timer.as_mut(), timer_slot) { t.after_submit(slot); }`

- [ ] **Step 4: Verify** — `cargo check`, then run the app: load `test5.wgsl` on DX12 and Vulkan, confirm a plausible GPU ms appears (Task 7 displays it; for now `log::info!` once per second or check via debugger — simplest: temporarily display `latest().total_ms` in the stats line, which Task 7 makes permanent).

- [ ] **Step 5: Commit** — `git add src/gpu_timer.rs src/main.rs && git commit -m "feat(gpu): per-pass GPU timing via timestamp queries"`

---

### Task 7: Live stats upgrade (percentiles + GPU ms)

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Frame history fields** on `PreviewApp` (+ init in `new`):

```rust
    /// Rolling CPU frame-time history (ms), capped at STATS_HISTORY frames.
    cpu_frame_history: std::collections::VecDeque<f64>,
    /// Rolling GPU frame-time history (ms) from GpuTimer, same cap.
    gpu_frame_history: std::collections::VecDeque<f64>,
    last_frame_at: Option<Instant>,
```

  with `const STATS_HISTORY: usize = 600;` near the other consts.

- [ ] **Step 2: Record per-frame samples** — at the very top of `render()` (before `process_compile_updates`):

```rust
        let frame_now = Instant::now();
        if let Some(prev) = self.last_frame_at {
            let cpu_ms = frame_now.duration_since(prev).as_secs_f64() * 1000.0;
            self.cpu_frame_history.push_back(cpu_ms);
            if self.cpu_frame_history.len() > STATS_HISTORY {
                self.cpu_frame_history.pop_front();
            }
        }
        self.last_frame_at = Some(frame_now);
```

  and after `begin_frame` harvesting (Step 3 hook of Task 6 — i.e. wherever `gpu_timer.begin_frame()` runs, the renderer borrow ends before UI code): grab `let gpu_latest_ms = r.gpu_timer.as_ref().and_then(|t| t.latest()).map(|t| t.total_ms);` then push into `self.gpu_frame_history` with the same cap. (Note `begin_frame` only updates `latest` when a new readback landed; pushing the same value twice across a frame gap is acceptable noise for live stats — the benchmark path in Task 8 uses the same source.)

- [ ] **Step 3: Display** — replace the stats row (2246–2266) with values computed from the histories:

```rust
            let cpu_samples: Vec<f64> = self.cpu_frame_history.iter().copied().collect();
            let cpu_stats = bench::FrameStats::from_samples_ms(&cpu_samples);
            let gpu_samples: Vec<f64> = self.gpu_frame_history.iter().copied().collect();
            let gpu_stats = bench::FrameStats::from_samples_ms(&gpu_samples);
```

  First line (keep the green style): `FPS: {avg_fps:.0} | avg {avg_ms:.2}ms p95 {p95_ms:.2} p99 {p99_ms:.2} | 1% low {low_1pct_fps:.0} fps | Frame: {total}`; second line: `GPU: {:.2}ms` from `gpu_stats.avg_ms` or `GPU: n/a`. Show per-pass GPU breakdown when multipass: from `r.gpu_timer latest().pass_ms` zipped with pass names — render as a small `ui.collapsing("GPU passes", ...)` listing `name: {:.2}ms`. Keep the existing 1-second `SidecarEvent::Stats` emit (3356–3372) untouched.

- [ ] **Step 4: Verify** — run app; stats line shows percentiles changing live; flip present modes and watch p95 react; GL fallback (if selectable) shows `GPU: n/a`.

- [ ] **Step 5: Commit** — `git add src/main.rs && git commit -m "feat(stats): live percentile frame stats and GPU pass timing display"`

---

### Task 8: Benchmark sweep integration + results table

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Bench state on `PreviewApp`** (+ init in `new`):

```rust
    bench_runner: Option<bench::BenchRunner>,
    /// Settings to restore after the sweep finishes.
    bench_prev_settings: Option<render_settings::RenderSettings>,
    bench_result: Option<bench::SweepResult>,
    bench_saved_path: Option<PathBuf>,
    // UI state
    bench_warmup_secs: f64,    // default 3.0
    bench_measure_secs: f64,   // default 10.0
    bench_force_uncapped: bool, // default true
    bench_selected_backends: Vec<(render_settings::BackendChoice, bool)>, // from ui_choices() minus Auto, all true
```

- [ ] **Step 2: Bench clock + drive loop** — add `fn drive_benchmark(&mut self)` called once per frame from `render()` *after* the frame history push (Task 7 Step 2) and *before* the UI pass (so status lines are fresh):

```rust
    fn drive_benchmark(&mut self) {
        let Some(runner) = self.bench_runner.as_mut() else {
            return;
        };
        let now = self.start_time.elapsed().as_secs_f64();
        // Feed the frame that just completed (last history entries).
        if let Some(cpu_ms) = self.cpu_frame_history.back().copied() {
            let gpu_ms = self.gpu_frame_history.back().copied();
            runner.record_frame(now, cpu_ms, gpu_ms);
        }
        match runner.next_action() {
            bench::BenchAction::None => {}
            bench::BenchAction::SwitchBackend { label } => {
                let choice = render_settings::BackendChoice::ui_choices()
                    .iter()
                    .copied()
                    .find(|c| c.label() == label);
                match choice {
                    Some(backend) => {
                        let mut s = self.settings.clone();
                        s.backend = backend;
                        s.adapter_name = self
                            .bench_prev_settings
                            .as_ref()
                            .and_then(|p| p.adapter_name.clone());
                        if runner.config().force_uncapped {
                            s.present_mode = render_settings::PresentModeChoice::Immediate;
                        }
                        self.pending_rebuild = Some(s);
                    }
                    None => runner.backend_failed(format!("Unknown backend label '{label}'")),
                }
            }
            bench::BenchAction::Finished => {
                let shader = self.loaded_shader_name.clone();
                if let Some(result) = runner.take_result(shader) {
                    match bench::save_sweep(&result, std::path::Path::new("benchmarks")) {
                        Ok(path) => self.bench_saved_path = Some(path),
                        Err(e) => self.push_diagnostic(
                            DiagLevel::Warning,
                            format!("Could not save benchmark: {e}"),
                        ),
                    }
                    self.bench_result = Some(result);
                }
                self.bench_runner = None;
                if let Some(prev) = self.bench_prev_settings.take() {
                    self.pending_rebuild = Some(prev);
                }
                self.push_diagnostic(DiagLevel::Success, "Benchmark sweep complete.".into());
            }
        }
    }
```

  (If `push_diagnostic` calls conflict with the `runner` borrow, restructure to compute the action first, drop the borrow, then act — the state machine API was designed for that.)

- [ ] **Step 3: Notify ready/failed** — the runner must learn when the swapped backend is actually rendering the compiled shader:
  - In `apply_pending_rebuild` (Task 5 Step 3): on init failure during an active bench → `runner.backend_failed(error)`.
  - In `apply_prepared_shader`/compile-update handling (`process_compile_updates`, 1912): where a compile **success** lands and bench is active and the runner is in WaitingReady → call:

```rust
        if let Some(runner) = self.bench_runner.as_mut() {
            let compile_total: f64 = self.pipeline_compile_ms.iter().map(|(_, ms)| ms).sum();
            runner.backend_ready(
                self.start_time.elapsed().as_secs_f64(),
                self.active_backend_name.clone(),
                self.active_adapter_name.clone(),
                format!("{:?}", self.active_present_mode),
                self.preview_pixel_size,
                if compile_total > 0.0 { Some(compile_total) } else { None },
            );
        }
```

    where a compile **error** lands during bench → `runner.backend_failed(error message)`. `backend_ready`/`backend_failed` are no-ops outside WaitingReady, so unconditional calls are safe.

- [ ] **Step 4: Benchmark UI section** — in `render_preview_workspace` (after the settings column, ~2474), an `ui.collapsing("🏁 Benchmark", ...)` containing:
  - Warmup secs (`egui::DragValue` 1.0–30.0), Measure secs (2.0–60.0), `force uncapped` checkbox, one checkbox per `bench_selected_backends` entry.
  - `Run Benchmark` button — disabled while `bench_runner.is_some()` or `active_compile.is_some()`; sets a `start_bench = true` local applied after the UI pass:

```rust
        if start_bench {
            let labels: Vec<String> = self
                .bench_selected_backends
                .iter()
                .filter(|(_, on)| *on)
                .map(|(b, _)| b.label().to_string())
                .collect();
            if labels.is_empty() {
                self.push_diagnostic(DiagLevel::Warning, "No backends selected.".into());
            } else {
                self.bench_prev_settings = Some(self.settings.clone());
                self.bench_result = None;
                self.bench_saved_path = None;
                self.bench_runner = Some(bench::BenchRunner::new(bench::BenchConfig {
                    warmup_secs: self.bench_warmup_secs,
                    measure_secs: self.bench_measure_secs,
                    backend_labels: labels,
                    force_uncapped: self.bench_force_uncapped,
                }));
            }
        }
```

  - While running: `ui.spinner()` + `runner.status_line()`.
  - Results table when `bench_result` is `Some`: `egui::Grid::new("bench_results").striped(true)` — header row `Backend | Adapter | Mode | Avg FPS | Avg ms | p95 ms | p99 ms | 1% low | GPU ms | Compile ms`; one row per run (`error` runs render the error string in red across the metric cells). Highlight the best Avg FPS and best p95 in green (`egui::Color32::from_rgb(145, 205, 145)`); saved-path caption under the table. A `Load previous…` ComboBox listing `bench::list_sweeps(Path::new("benchmarks"))` file names; choosing one loads it into `bench_result` via `bench::load_sweep`.

- [ ] **Step 5: Full verification**
  - `cargo test` → green; `cargo check` → no errors.
  - Run app → load `shaders/Test/test5.wgsl` → Run Benchmark with DX12 + Vulkan, warmup 2 s, measure 5 s → watch it cycle, end on a table with two rows, JSON file in `benchmarks/`, settings restored to pre-bench state.
  - Load the Kerr-Newman shader → run a sweep → confirm multipass GPU per-pass numbers populate and the heavier load shows realistic FPS deltas between backends.
  - Kill/relaunch app → `Load previous…` lists and loads the saved JSONs.

- [ ] **Step 6: Commit** — `git add src/main.rs && git commit -m "feat(bench): automated backend sweep with results table and persistence"`

---

### Task 9: Documentation + roadmap sync

**Files:**
- Modify: `ROADMAP.md` (Implemented section + decision log), `TIMELINE.md`, `docs/PROJECT_MAP.md` (Working Areas: add the three new modules), `CLAUDE.md` + `AGENTS.md` (Recent Native Rendering Notes: settings/bench exist; keep both files mirrored)

- [ ] **Step 1:** Add to `ROADMAP.md` "Implemented / Existing Paths": render settings (adapter/present mode/latency/DX12 compiler), GPU timestamp timing, percentile stats, benchmark sweep with JSON persistence under `benchmarks/`. Decision log entry dated 2026-06-11: render-lab direction adopted; sweeps force uncapped present mode by default.
- [ ] **Step 2:** `TIMELINE.md`: mark the render-lab milestone complete with date.
- [ ] **Step 3:** `docs/PROJECT_MAP.md`: list `src/bench.rs`, `src/render_settings.rs`, `src/gpu_timer.rs` under Working Areas.
- [ ] **Step 4:** Update the matching paragraph in both `CLAUDE.md` and `AGENTS.md` (they must stay in sync).
- [ ] **Step 5:** `cargo test` one final time; commit — `git add -A && git commit -m "docs: record render lab + benchmark sweep in planning docs"`.

---

## Self-review notes

- Spec coverage: settings (Task 4+5), GPU timing (Task 6), live stats (Task 7), sweep + table + persistence (Task 8), error handling embedded in Tasks 3/5/6/8, docs (Task 9). Gitignore for `benchmarks/` in Task 2.
- The `pct(0.95)`-style calls pass fractions; `from_samples_ms` documents `q` as a fraction (0.95), matching the closure signature.
- Type names consistent: `FrameStats`, `RunRecord`, `SweepResult`, `BenchRunner`, `BenchAction::{None, SwitchBackend, Finished}`, `RenderSettings`, `PresentModeChoice`, `DxCompilerChoice`, `GpuTimer`, `FrameGpuTiming`.
- Known risk: borrow-checker friction wiring `GpuTimer` into the existing `match &r.mode` blocks — mitigation written into Task 6 Step 3 (take/restore pattern).
- wgpu 29 API names verified against the local registry source (`Dx12Compiler::{Auto, Fxc, StaticDxc}`, `backend_options.dx12.shader_compiler`, `RenderPassTimestampWrites`, `QUERY_RESOLVE` usage, `get_timestamp_period`).
