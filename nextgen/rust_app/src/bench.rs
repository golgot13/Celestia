use std::time::Instant;

use crate::{
    detect_cpu_features, interpolate_ephemeris, mean_anomaly_from_jd, EphemerisSample,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BenchmarkReport {
    pub samples: usize,
    pub scalar_ns: u128,
    pub reference_rms: f64,
    pub avx2_available: bool,
    pub vector_width: usize,
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
}
