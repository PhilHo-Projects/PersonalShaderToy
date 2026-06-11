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
        // q is a fraction (0.95 = p95); nearest-rank method.
        let pct = |q: f64| -> f64 {
            let idx = ((q * n as f64).ceil() as usize).clamp(1, n) - 1;
            sorted[idx]
        };
        let worst_count = (n / 100).max(1);
        let low_1pct = sorted[n - worst_count..].iter().sum::<f64>() / worst_count as f64;
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
        if self.low_1pct_ms > 0.0 {
            1000.0 / self.low_1pct_ms
        } else {
            0.0
        }
    }
}

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
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
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

pub fn save_sweep(
    result: &SweepResult,
    dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
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
    /// All backends processed; Finished not yet consumed via take_result.
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

/// Pure sweep state machine. The host (main.rs) polls `next_action()` once per
/// frame, executes renderer swaps, and reports back through `backend_ready` /
/// `backend_failed` and `record_frame`. The clock is abstract seconds so the
/// whole machine is testable without a GPU or real time.
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
        Self {
            config,
            phase,
            runs: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Done)
    }

    pub fn config(&self) -> &BenchConfig {
        &self.config
    }

    /// Poll once per frame. `SwitchBackend` is returned exactly once per
    /// backend; `Finished` repeats until `take_result` consumes it so the
    /// host cannot miss it.
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

    /// Call when the swapped backend is up and the shader compiled on it.
    /// No-op outside the WaitingReady phase, so unconditional calls are safe.
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
                meta: RunMeta {
                    backend,
                    adapter,
                    present_mode,
                    resolution,
                    pipeline_compile_ms,
                },
            };
        }
    }

    /// Call when the backend swap or shader compile failed. Records a failed
    /// run and moves on. No-op outside WaitingReady.
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

    /// Feed one completed frame. Warmup frames are dropped; the frame that
    /// crosses into the measure window is the first counted sample; the frame
    /// that crosses past the window finalizes the run without being counted.
    pub fn record_frame(&mut self, now: f64, cpu_ms: f64, gpu_ms: Option<f64>) {
        match &mut self.phase {
            Phase::Warmup {
                index,
                started,
                meta,
            } => {
                if now - *started >= self.config.warmup_secs {
                    self.phase = Phase::Measuring {
                        index: *index,
                        started: now,
                        meta: meta.clone(),
                        cpu_ms: vec![cpu_ms],
                        gpu_ms: gpu_ms.into_iter().collect(),
                        frames: 1,
                    };
                }
            }
            Phase::Measuring {
                index,
                started,
                meta,
                cpu_ms: cpu,
                gpu_ms: gpu,
                frames,
            } => {
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

    /// Consume the finished sweep. Returns None unless the runner reached the
    /// PendingFinish phase (i.e. next_action returned Finished).
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
                format!(
                    "Benchmark: switching to {}…",
                    self.config.backend_labels[*i]
                )
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

/// Saved sweeps in `dir`, newest first (file names end in a unix timestamp).
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
        let BenchAction::SwitchBackend { label } = r.next_action() else {
            panic!()
        };
        assert_eq!(label, "DirectX 12");
        assert!(matches!(r.next_action(), BenchAction::None));

        r.backend_ready(
            0.0,
            "Dx12".into(),
            "GPU A".into(),
            "Immediate".into(),
            [800, 600],
            Some(10.0),
        );
        // Warmup frames (t < 1.0) are not counted.
        r.record_frame(0.5, 4.0, Some(2.0));
        // Crossing into measure window.
        r.record_frame(1.1, 8.0, Some(4.0));
        r.record_frame(1.6, 8.0, None);
        r.record_frame(1.9, 8.0, Some(4.0));
        // Crossing past measure end finalizes the run.
        r.record_frame(3.2, 8.0, Some(4.0));

        let BenchAction::SwitchBackend { label } = r.next_action() else {
            panic!()
        };
        assert_eq!(label, "Vulkan");
        r.backend_ready(
            4.0,
            "Vulkan".into(),
            "GPU A".into(),
            "Mailbox".into(),
            [800, 600],
            None,
        );
        r.record_frame(5.1, 5.0, None);
        r.record_frame(5.5, 5.0, None);
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
        let BenchAction::SwitchBackend { .. } = r.next_action() else {
            panic!()
        };
        r.backend_failed("no adapter".into());

        let BenchAction::SwitchBackend { label } = r.next_action() else {
            panic!()
        };
        assert_eq!(label, "Vulkan");
        r.backend_ready(0.0, "Vulkan".into(), "GPU".into(), "Fifo".into(), [64, 64], None);
        r.record_frame(1.5, 5.0, None);
        r.record_frame(1.7, 5.0, None);
        r.record_frame(3.6, 5.0, None);

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
}
