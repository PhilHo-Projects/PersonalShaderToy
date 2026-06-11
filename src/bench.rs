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
