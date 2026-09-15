use std::{arch::x86_64::{
    _mm256_add_pd, _mm256_div_pd, _mm256_loadu_pd, _mm256_max_pd, _mm256_min_pd,
    _mm256_mul_pd, _mm256_set1_pd, _mm256_storeu_pd, _mm256_sub_pd,
}, f64::consts::PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EphemerisSample {
    pub julian_day: f64,
    pub right_ascension_rad: f64,
    pub declination_rad: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EphemerisState {
    pub ra_rad: f64,
    pub dec_rad: f64,
    pub distance_au: f64,
}

pub fn mean_anomaly_from_jd(julian_day: f64) -> f64 {
    let days = julian_day - 2451545.0;
    let wrapped = (days / 365.25) * 2.0 * PI;
    wrapped.rem_euclid(2.0 * PI)
}

pub fn interpolate_ephemeris(sample_a: EphemerisSample, sample_b: EphemerisSample, target_jd: f64) -> EphemerisState {
    let span = sample_b.julian_day - sample_a.julian_day;
    if span == 0.0 {
        return EphemerisState {
            ra_rad: sample_a.right_ascension_rad,
            dec_rad: sample_a.declination_rad,
            distance_au: 1.0,
        };
    }

    let t = ((target_jd - sample_a.julian_day) / span).clamp(0.0, 1.0);
    let ra_rad = sample_a.right_ascension_rad + (sample_b.right_ascension_rad - sample_a.right_ascension_rad) * t;
    let dec_rad = sample_a.declination_rad + (sample_b.declination_rad - sample_a.declination_rad) * t;
    let distance_au = 1.0 + t * 0.25;

    EphemerisState {
        ra_rad,
        dec_rad,
        distance_au,
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn interpolate_ephemeris_batch_avx2_inner(
    target_jd: &[f64],
    sample_a: EphemerisSample,
    sample_b: EphemerisSample,
    out_ra: &mut [f64],
    out_dec: &mut [f64],
    out_distance: &mut [f64],
) -> Result<(), &'static str> {
    if target_jd.len() != out_ra.len() || target_jd.len() != out_dec.len() || target_jd.len() != out_distance.len() {
        return Err("batch lengths must match");
    }

    let span = sample_b.julian_day - sample_a.julian_day;
    if span == 0.0 {
        return Err("interpolation span must be non-zero");
    }

    let zero = _mm256_set1_pd(0.0);
    let one = _mm256_set1_pd(1.0);
    let sample_a_jd = _mm256_set1_pd(sample_a.julian_day);
    let sample_b_ra = _mm256_set1_pd(sample_b.right_ascension_rad);
    let sample_a_ra = _mm256_set1_pd(sample_a.right_ascension_rad);
    let sample_b_dec = _mm256_set1_pd(sample_b.declination_rad);
    let sample_a_dec = _mm256_set1_pd(sample_a.declination_rad);
    let span_v = _mm256_set1_pd(span);

    for (index, chunk) in target_jd.chunks_exact(4).enumerate() {
        let jd_v = _mm256_loadu_pd(chunk.as_ptr());
        let offset = _mm256_sub_pd(jd_v, sample_a_jd);
        let t = _mm256_div_pd(offset, span_v);
        let clamped_t = _mm256_max_pd(_mm256_min_pd(t, one), zero);

        let ra_delta = _mm256_sub_pd(sample_b_ra, sample_a_ra);
        let dec_delta = _mm256_sub_pd(sample_b_dec, sample_a_dec);
        let ra_v = _mm256_add_pd(sample_a_ra, _mm256_mul_pd(ra_delta, clamped_t));
        let dec_v = _mm256_add_pd(sample_a_dec, _mm256_mul_pd(dec_delta, clamped_t));
        let distance_v = _mm256_add_pd(_mm256_set1_pd(1.0), _mm256_mul_pd(clamped_t, _mm256_set1_pd(0.25)));

        let out_index = index * 4;
        _mm256_storeu_pd(out_ra.as_mut_ptr().add(out_index), ra_v);
        _mm256_storeu_pd(out_dec.as_mut_ptr().add(out_index), dec_v);
        _mm256_storeu_pd(out_distance.as_mut_ptr().add(out_index), distance_v);
    }

    let tail = target_jd.len() % 4;
    for index in (target_jd.len() - tail)..target_jd.len() {
        let state = interpolate_ephemeris(sample_a, sample_b, target_jd[index]);
        out_ra[index] = state.ra_rad;
        out_dec[index] = state.dec_rad;
        out_distance[index] = state.distance_au;
    }

    Ok(())
}

pub fn interpolate_ephemeris_batch_avx2(
    target_jd: &[f64],
    sample_a: EphemerisSample,
    sample_b: EphemerisSample,
    out_ra: &mut [f64],
    out_dec: &mut [f64],
    out_distance: &mut [f64],
) -> Result<(), &'static str> {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            return unsafe {
                interpolate_ephemeris_batch_avx2_inner(target_jd, sample_a, sample_b, out_ra, out_dec, out_distance)
            };
        }
    }

    if target_jd.len() != out_ra.len() || target_jd.len() != out_dec.len() || target_jd.len() != out_distance.len() {
        return Err("batch lengths must match");
    }

    for (index, jd) in target_jd.iter().enumerate() {
        let state = interpolate_ephemeris(sample_a, sample_b, *jd);
        out_ra[index] = state.ra_rad;
        out_dec[index] = state.dec_rad;
        out_distance[index] = state.distance_au;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_anomaly_wraps_correctly() {
        let value = mean_anomaly_from_jd(2451545.0 + 365.25 + 10.0);
        let expected = (10.0 / 365.25) * 2.0 * PI;
        assert!((value - expected).abs() < 1e-9);
        assert!(value >= 0.0 && value < 2.0 * PI);
    }

    #[test]
    fn interpolation_is_between_reference_samples() {
        let a = EphemerisSample {
            julian_day: 2451545.0,
            right_ascension_rad: 0.1,
            declination_rad: 0.2,
        };
        let b = EphemerisSample {
            julian_day: 2451545.0 + 10.0,
            right_ascension_rad: 0.4,
            declination_rad: 0.5,
        };

        let state = interpolate_ephemeris(a, b, 2451545.0 + 5.0);
        assert!((state.ra_rad - 0.25).abs() < 1e-9);
        assert!((state.dec_rad - 0.35).abs() < 1e-9);
        assert!((state.distance_au - 1.125).abs() < 1e-9);
    }

    #[test]
    fn avx2_batch_matches_scalar_reference() {
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

        let jd_values: Vec<f64> = (0..8).map(|i| 2451545.0 + i as f64).collect();
        let mut expected_ra = vec![0.0; 8];
        let mut expected_dec = vec![0.0; 8];
        let mut expected_distance = vec![0.0; 8];

        for (index, jd) in jd_values.iter().enumerate() {
            let state = interpolate_ephemeris(sample_a, sample_b, *jd);
            expected_ra[index] = state.ra_rad;
            expected_dec[index] = state.dec_rad;
            expected_distance[index] = state.distance_au;
        }

        let mut out_ra = vec![0.0; 8];
        let mut out_dec = vec![0.0; 8];
        let mut out_distance = vec![0.0; 8];

        let result = interpolate_ephemeris_batch_avx2(
            &jd_values,
            sample_a,
            sample_b,
            &mut out_ra,
            &mut out_dec,
            &mut out_distance,
        );

        assert!(result.is_ok());
        for i in 0..8 {
            assert!((out_ra[i] - expected_ra[i]).abs() < 1e-9);
            assert!((out_dec[i] - expected_dec[i]).abs() < 1e-9);
            assert!((out_distance[i] - expected_distance[i]).abs() < 1e-9);
        }
    }
}
