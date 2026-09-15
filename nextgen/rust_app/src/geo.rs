use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeographicCoord {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub elevation_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HorizontalCoordinates {
    pub azimuth_rad: f64,
    pub altitude_rad: f64,
}

pub fn deg_to_rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

pub fn rad_to_deg(rad: f64) -> f64 {
    rad * 180.0 / PI
}

pub fn normalize_angle(angle_rad: f64) -> f64 {
    angle_rad.rem_euclid(2.0 * PI)
}

pub fn equatorial_to_horizontal(hour_angle_rad: f64, declination_rad: f64, latitude_rad: f64) -> HorizontalCoordinates {
    let sin_lat = latitude_rad.sin();
    let cos_lat = latitude_rad.cos();
    let sin_dec = declination_rad.sin();
    let cos_dec = declination_rad.cos();
    let sin_ha = hour_angle_rad.sin();
    let cos_ha = hour_angle_rad.cos();

    let sin_alt = (sin_lat * sin_dec + cos_lat * cos_dec * cos_ha).clamp(-1.0, 1.0);
    let altitude_rad = sin_alt.asin();

    let x = cos_ha * sin_lat - declination_rad.tan() * cos_lat;
    let azimuth_rad = normalize_angle(sin_ha.atan2(x) + PI);

    HorizontalCoordinates {
        azimuth_rad,
        altitude_rad,
    }
}

pub fn compute_airmass(altitude_rad: f64) -> f64 {
    if altitude_rad >= PI / 2.0 - 1e-12 {
        return 1.0;
    }
    if altitude_rad <= 0.0 {
        return f64::INFINITY;
    }

    let sec_z = 1.0 / altitude_rad.cos();
    let delta = sec_z - 1.0;
    let airmass = sec_z
        - 0.0018167 * delta
        - 0.002875 * delta.powi(2)
        - 0.0008083 * delta.powi(3);

    airmass.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zenith_altitude_is_ninety_degrees() {
        let result = equatorial_to_horizontal(0.0, deg_to_rad(48.8566), deg_to_rad(48.8566));
        assert!((result.altitude_rad - PI / 2.0).abs() < 1e-7);
    }

    #[test]
    fn angle_normalization_wraps_around_full_turn() {
        let value = normalize_angle(-0.25);
        assert!((value - (2.0 * PI - 0.25)).abs() < 1e-12);
    }

    #[test]
    fn airmass_is_one_at_zenith() {
        let value = compute_airmass(PI / 2.0);
        assert!((value - 1.0).abs() < 1e-9);
    }
}
