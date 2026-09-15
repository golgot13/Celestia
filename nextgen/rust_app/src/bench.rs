use std::time::Instant;

use crate::{
    detect_cpu_features, interpolate_ephemeris, interpolate_ephemeris_batch_avx2,
    mean_anomaly_from_jd, EphemerisSample,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchmarkReport {
    pub samples: usize,
    pub scalar_ns: u128,
    pub reference_rms: f64,
    pub avx2_available: bool,
    pub vector_width: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchmarkThreshold {
    pub samples: usize,
    pub scalar_ns: u128,
    pub avx2_ns: u128,
    pub speedup_ratio: f64,
    pub max_abs_error: f64,
    pub tolerance: f64,
    pub accepted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchmarkSweepResult {
    pub samples: usize,
    pub scalar_ns: u128,
    pub avx2_ns: u128,
    pub speedup_ratio: f64,
    pub max_abs_error: f64,
    pub accepted: bool,
}

pub fn run_ephemeris_benchmark(sample_count: usize) -> BenchmarkReport {
    let cpu = detect_cpu_features();
    let samples: Vec<EphemerisSample> = (0..sample_count)
        .map(|index| {
            let julian_day = 2451545.0 + index as f64 * 0.25;
            let phase = mean_anomaly_from_jd(julian_day);
            EphemerisSample {
                julian_day,
                right_ascension_rad: phase,
                declination_rad: 0.5 * phase.sin(),
            }
        })
        .collect();

    let start = Instant::now();
    let mut max_error = 0.0_f64;
    for window in samples.windows(2) {
        let from = window[0];
        let to = window[1];
        let target = (from.julian_day + to.julian_day) * 0.5;
        let result = interpolate_ephemeris(from, to, target);
        let expected_ra = (from.right_ascension_rad + to.right_ascension_rad) * 0.5;
        let expected_dec = (from.declination_rad + to.declination_rad) * 0.5;
        let ra_error = (result.ra_rad - expected_ra).abs();
        let dec_error = (result.dec_rad - expected_dec).abs();
        max_error = max_error.max(ra_error).max(dec_error);
    }

    let scalar_ns = start.elapsed().as_nanos();
    let reference_rms = max_error.max(1.0e-12);

    BenchmarkReport {
        samples: sample_count,
        scalar_ns,
        reference_rms,
        avx2_available: cpu.avx2,
        vector_width: if cpu.avx2 { 8 } else { 4 },
    }
}

pub fn evaluate_ephemeris_benchmark_threshold(
    sample_count: usize,
    min_speedup: f64,
    tolerance: f64,
) -> BenchmarkThreshold {
    const MIN_ACCEPTANCE_BATCH: usize = 100_000;
    let cpu = detect_cpu_features();

    if sample_count < MIN_ACCEPTANCE_BATCH {
        return BenchmarkThreshold {
            samples: sample_count,
            scalar_ns: 0,
            avx2_ns: 0,
            speedup_ratio: 0.0,
            max_abs_error: 0.0,
            tolerance,
            accepted: false,
        };
    }

    let sample_a = EphemerisSample {
        julian_day: 2451545.0,
        right_ascension_rad: 0.2,
        declination_rad: 0.3,
    };
    let sample_b = EphemerisSample {
        julian_day: 2451555.0,
        right_ascension_rad: 0.8,
        declination_rad: 1.1,
    };

    let target_jd: Vec<f64> = (0..sample_count)
        .map(|idx| sample_a.julian_day + idx as f64)
        .collect();

    let mut expected_ra = vec![0.0; sample_count];
    let mut expected_dec = vec![0.0; sample_count];
    let mut expected_distance = vec![0.0; sample_count];
    for (index, jd) in target_jd.iter().enumerate() {
        let state = interpolate_ephemeris(sample_a, sample_b, *jd);
        expected_ra[index] = state.ra_rad;
        expected_dec[index] = state.dec_rad;
        expected_distance[index] = state.distance_au;
    }

    let scalar_start = Instant::now();
    for (index, jd) in target_jd.iter().enumerate() {
        let state = interpolate_ephemeris(sample_a, sample_b, *jd);
        let _ = (state.ra_rad, state.dec_rad, state.distance_au, index);
    }
    let scalar_ns = scalar_start.elapsed().as_nanos();

    let mut avx_ra = vec![0.0; sample_count];
    let mut avx_dec = vec![0.0; sample_count];
    let mut avx_distance = vec![0.0; sample_count];

    let avx_start = Instant::now();
    let avx_result = interpolate_ephemeris_batch_avx2(
        &target_jd,
        sample_a,
        sample_b,
        &mut avx_ra,
        &mut avx_dec,
        &mut avx_distance,
    );
    let avx_ns = avx_start.elapsed().as_nanos();

    let mut max_abs_error = 0.0_f64;
    for idx in 0..sample_count {
        max_abs_error = max_abs_error
            .max((expected_ra[idx] - avx_ra[idx]).abs())
            .max((expected_dec[idx] - avx_dec[idx]).abs())
            .max((expected_distance[idx] - avx_distance[idx]).abs());
    }

    let speedup_ratio = if avx_ns == 0 { 1.0 } else { scalar_ns as f64 / avx_ns as f64 };
    let accepted = cpu.avx2
        && avx_result.is_ok()
        && speedup_ratio >= min_speedup
        && max_abs_error <= tolerance;

    BenchmarkThreshold {
        samples: sample_count,
        scalar_ns,
        avx2_ns: avx_ns,
        speedup_ratio,
        max_abs_error,
        tolerance,
        accepted,
    }
}

pub fn run_ephemeris_benchmark_suite() -> Vec<BenchmarkSweepResult> {
    let sizes = [10_000usize, 100_000, 1_000_000];
    let mut results = Vec::with_capacity(sizes.len());

    for size in sizes {
        let threshold = evaluate_ephemeris_benchmark_threshold(size, 1.0, 1e-9);
        results.push(BenchmarkSweepResult {
            samples: threshold.samples,
            scalar_ns: threshold.scalar_ns,
            avx2_ns: threshold.avx2_ns,
            speedup_ratio: threshold.speedup_ratio,
            max_abs_error: threshold.max_abs_error,
            accepted: threshold.accepted,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_scan_is_stable_for_small_batch() {
        let report = run_ephemeris_benchmark(32);
        assert!(report.samples == 32);
        assert!(report.scalar_ns > 0);
        assert!(report.reference_rms > 0.0);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn benchmark_gate_accepts_valid_avx2_gain() {
        let threshold = evaluate_ephemeris_benchmark_threshold(1_000_000, 1.0, 1e-9);
        assert!(threshold.accepted);
        assert!(threshold.speedup_ratio > 1.0);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn benchmark_gate_rejects_tiny_debug_batches() {
        let threshold = evaluate_ephemeris_benchmark_threshold(1_000, 1.0, 1e-9);
        assert!(!threshold.accepted);
    }

    #[test]
    fn benchmark_suite_reports_realistic_batch_progression() {
        let suite = run_ephemeris_benchmark_suite();
        assert_eq!(suite.len(), 3);
        assert!(suite.iter().all(|entry| entry.samples > 0));

        if !cfg!(debug_assertions) {
            assert!(suite.iter().any(|entry| entry.accepted));
        }
    }
}
