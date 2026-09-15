use std::f64::consts::PI;

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
}
