use observatory_core::{
    interpolate_ephemeris, interpolate_ephemeris_batch_avx2, EphemerisSample,
};
use std::time::Instant;

fn main() {
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

    let size = 1_000_000usize;
    let mut scalar_ra = vec![0.0; size];
    let mut scalar_dec = vec![0.0; size];
    let mut scalar_distance = vec![0.0; size];
    let target_jd: Vec<f64> = (0..size).map(|i| sample_a.julian_day + i as f64).collect();

    let scalar_start = Instant::now();
    for (index, jd) in target_jd.iter().enumerate() {
        let state = interpolate_ephemeris(sample_a, sample_b, *jd);
        scalar_ra[index] = state.ra_rad;
        scalar_dec[index] = state.dec_rad;
        scalar_distance[index] = state.distance_au;
    }
    let scalar_duration = scalar_start.elapsed();

    let mut avx_ra = vec![0.0; size];
    let mut avx_dec = vec![0.0; size];
    let mut avx_distance = vec![0.0; size];
    let avx_start = Instant::now();
    let _ = interpolate_ephemeris_batch_avx2(
        &target_jd,
        sample_a,
        sample_b,
        &mut avx_ra,
        &mut avx_dec,
        &mut avx_distance,
    );
    let avx_duration = avx_start.elapsed();

    let mut max_error = 0.0_f64;
    for i in 0..size {
        max_error = max_error.max((scalar_ra[i] - avx_ra[i]).abs());
        max_error = max_error.max((scalar_dec[i] - avx_dec[i]).abs());
        max_error = max_error.max((scalar_distance[i] - avx_distance[i]).abs());
    }

    println!("scalar_elapsed_ns={}", scalar_duration.as_nanos());
    println!("avx_elapsed_ns={}", avx_duration.as_nanos());
    println!("max_abs_error={}", max_error);
}
