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
