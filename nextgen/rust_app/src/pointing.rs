use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct AtmosphericConditions {
    pub temperature_c: f64,
    pub pressure_hpa: f64,
    pub relative_humidity_pct: f64,
    pub wavelength_nm: f64,
}

impl Default for AtmosphericConditions {
    fn default() -> Self {
        Self {
            temperature_c: 10.0,
            pressure_hpa: 1013.25,
            relative_humidity_pct: 50.0,
            wavelength_nm: 550.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointingModelTerms {
    pub polar_elevation_error_arcsec: f64,  // ME (misalignment in elevation)
    pub polar_azimuth_error_arcsec: f64,    // MA (misalignment in azimuth)
    pub non_perpendicularity_arcsec: f64,   // NP (non-perpendicularity of RA/DEC axes)
    pub collimation_cone_error_arcsec: f64, // CH (optical collimation error)
    pub tube_flexure_arcsec: f64,           // TF (gravitational tube flexure)
    pub rms_residual_arcsec: f64,
    pub star_count: usize,
    pub converged: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointingCalibrationStar {
    pub catalog_ra_deg: f64,
    pub catalog_dec_deg: f64,
    pub mount_hour_angle_deg: f64,
    pub mount_dec_deg: f64,
    pub lst_deg: f64,
    pub site_latitude_deg: f64,
}

pub fn compute_true_atmospheric_refraction(
    apparent_altitude_rad: f64,
    conditions: &AtmosphericConditions,
) -> f64 {
    let alt_deg = apparent_altitude_rad * 180.0 / PI;
    if alt_deg <= -0.5 {
        return 0.0;
    }

    // Standard Bennett formula for optical atmospheric refraction in arcminutes
    let r_std_arcmin = 1.0 / (alt_deg + 7.31 / (alt_deg + 4.4)).to_radians().tan();

    // Temperature, pressure, and wavelength scaling
    let p_factor = conditions.pressure_hpa / 1010.0;
    let t_factor = 283.0 / (273.15 + conditions.temperature_c);
    let lambda_um = conditions.wavelength_nm * 1e-3;
    let color_factor = 0.965 + 0.0164 / (lambda_um * lambda_um);

    let refraction_arcmin = r_std_arcmin * p_factor * t_factor * color_factor;
    (refraction_arcmin / 60.0).to_radians()
}

pub fn apply_pointing_correction(
    catalog_ra_rad: f64,
    catalog_dec_rad: f64,
    lst_rad: f64,
    lat_rad: f64,
    model: &PointingModelTerms,
    conditions: &AtmosphericConditions,
) -> (f64, f64) {
    let ha = (lst_rad - catalog_ra_rad).rem_euclid(2.0 * PI);
    let ha_signed = if ha > PI { ha - 2.0 * PI } else { ha };
    let dec = catalog_dec_rad;

    let arcsec_to_rad = PI / (180.0 * 3600.0);
    let me = model.polar_elevation_error_arcsec * arcsec_to_rad;
    let ma = model.polar_azimuth_error_arcsec * arcsec_to_rad;
    let np = model.non_perpendicularity_arcsec * arcsec_to_rad;
    let ch = model.collimation_cone_error_arcsec * arcsec_to_rad;
    let tf = model.tube_flexure_arcsec * arcsec_to_rad;

    // Geometric TPOINT analytical corrections for equatorial mount
    let sin_ha = ha_signed.sin();
    let cos_ha = ha_signed.cos();
    let tan_dec = dec.tan();
    let cos_dec = dec.cos();
    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();

    // Delta Hour Angle correction
    let delta_ha = -me * sin_ha * tan_dec
        - ma * cos_ha * tan_dec
        + np * tan_dec
        + ch / cos_dec.max(1e-6)
        - tf * (cos_lat * sin_ha) / cos_dec.max(1e-6);

    // Delta Declination correction
    let delta_dec = -me * cos_ha
        + ma * sin_ha
        - tf * (sin_lat * cos_dec - cos_lat * sin_dec * cos_ha);

    // Atmospheric refraction adjustment in zenith angle
    let sin_alt = (sin_lat * dec.sin() + cos_lat * cos_dec * cos_ha).clamp(-1.0, 1.0);
    let alt = sin_alt.asin();
    let refr = compute_true_atmospheric_refraction(alt, conditions);

    let cos_alt = alt.cos().max(1e-6);
    let sin_q = (cos_lat * sin_ha / cos_alt).clamp(-1.0, 1.0); // Parallactic angle
    let cos_q = ((sin_lat - sin_alt * dec.sin()) / (cos_alt * cos_dec.max(1e-6))).clamp(-1.0, 1.0);

    let refr_ha = refr * sin_q / cos_dec.max(1e-6);
    let refr_dec = refr * cos_q;

    let corrected_ha = ha_signed + delta_ha - refr_ha;
    let corrected_dec = dec + delta_dec + refr_dec;

    (corrected_ha.rem_euclid(2.0 * PI), corrected_dec)
}

pub fn solve_pointing_model_least_squares(
    stars: &[PointingCalibrationStar],
    conditions: &AtmosphericConditions,
) -> Result<PointingModelTerms, String> {
    let n = stars.len();
    if n < 4 {
        return Err("at least 4 calibration stars required to solve pointing model".to_string());
    }

    let rad_to_arcsec = (180.0 * 3600.0) / PI;

    let mut sum_dha_sq = 0.0;
    let mut sum_ddec_sq = 0.0;

    let mut total_me = 0.0;
    let mut total_ma = 0.0;
    let mut total_ch = 0.0;
    let mut total_np = 0.0;

    for star in stars {
        let cat_ra_rad = star.catalog_ra_deg.to_radians();
        let cat_dec_rad = star.catalog_dec_deg.to_radians();
        let lst_rad = star.lst_deg.to_radians();
        let lat_rad = star.site_latitude_deg.to_radians();

        let expected_ha_rad = (lst_rad - cat_ra_rad).rem_euclid(2.0 * PI);
        let measured_ha_rad = star.mount_hour_angle_deg.to_radians().rem_euclid(2.0 * PI);

        let dha_rad = (measured_ha_rad - expected_ha_rad + PI).rem_euclid(2.0 * PI) - PI;
        let ddec_rad = star.mount_dec_deg.to_radians() - cat_dec_rad;

        let ha_signed = if expected_ha_rad > PI { expected_ha_rad - 2.0 * PI } else { expected_ha_rad };
        let sin_ha = ha_signed.sin();
        let cos_ha = ha_signed.cos();

        // Project measured errors onto elevation (ME) and azimuth (MA) polar axis errors
        total_me += -ddec_rad * cos_ha * rad_to_arcsec;
        total_ma += ddec_rad * sin_ha * rad_to_arcsec;
        total_ch += dha_rad * cat_dec_rad.cos() * rad_to_arcsec;
        total_np += dha_rad * cat_dec_rad.tan().clamp(-5.0, 5.0) * rad_to_arcsec;

        sum_dha_sq += (dha_rad * rad_to_arcsec).powi(2);
        sum_ddec_sq += (ddec_rad * rad_to_arcsec).powi(2);
    }

    let count = n as f64;
    let me = total_me / count;
    let ma = total_ma / count;
    let ch = total_ch / count;
    let np = total_np / count;

    let total_rms = ((sum_dha_sq + sum_ddec_sq) / (2.0 * count)).sqrt();

    Ok(PointingModelTerms {
        polar_elevation_error_arcsec: me,
        polar_azimuth_error_arcsec: ma,
        non_perpendicularity_arcsec: np,
        collimation_cone_error_arcsec: ch,
        tube_flexure_arcsec: 0.0,
        rms_residual_arcsec: total_rms,
        star_count: n,
        converged: total_rms < 120.0,
    })
}

pub fn pointing_model_to_json(model: &PointingModelTerms) -> String {
    format!(
        "{{\n  \"polar_elevation_error_arcsec\": {:.2},\n  \"polar_azimuth_error_arcsec\": {:.2},\n  \"non_perpendicularity_arcsec\": {:.2},\n  \"collimation_cone_error_arcsec\": {:.2},\n  \"tube_flexure_arcsec\": {:.2},\n  \"rms_residual_arcsec\": {:.2},\n  \"star_count\": {},\n  \"converged\": {}\n}}\n",
        model.polar_elevation_error_arcsec,
        model.polar_azimuth_error_arcsec,
        model.non_perpendicularity_arcsec,
        model.collimation_cone_error_arcsec,
        model.tube_flexure_arcsec,
        model.rms_residual_arcsec,
        model.star_count,
        model.converged
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_standard_atmospheric_refraction_at_zenith_and_horizon() {
        let cond = AtmosphericConditions::default();

        let refr_zenith = compute_true_atmospheric_refraction(PI / 2.0, &cond);
        assert!(refr_zenith.to_degrees() * 3600.0 < 0.1); // ~0 at 90 deg alt

        let refr_45deg = compute_true_atmospheric_refraction(PI / 4.0, &cond);
        assert!((refr_45deg.to_degrees() * 3600.0 - 58.0).abs() < 5.0); // ~58 arcsec at 45 deg alt
    }

    #[test]
    fn solves_pointing_model_from_calibration_stars() {
        let cond = AtmosphericConditions::default();

        let stars = vec![
            PointingCalibrationStar {
                catalog_ra_deg: 0.0,
                catalog_dec_deg: 30.0,
                mount_hour_angle_deg: 0.005,
                mount_dec_deg: 30.002,
                lst_deg: 0.0,
                site_latitude_deg: 44.0,
            },
            PointingCalibrationStar {
                catalog_ra_deg: 90.0,
                catalog_dec_deg: 45.0,
                mount_hour_angle_deg: 0.004,
                mount_dec_deg: 45.001,
                lst_deg: 90.0,
                site_latitude_deg: 44.0,
            },
            PointingCalibrationStar {
                catalog_ra_deg: 180.0,
                catalog_dec_deg: 60.0,
                mount_hour_angle_deg: -0.003,
                mount_dec_deg: 59.998,
                lst_deg: 180.0,
                site_latitude_deg: 44.0,
            },
            PointingCalibrationStar {
                catalog_ra_deg: 270.0,
                catalog_dec_deg: 20.0,
                mount_hour_angle_deg: -0.005,
                mount_dec_deg: 19.997,
                lst_deg: 270.0,
                site_latitude_deg: 44.0,
            },
        ];

        let model = solve_pointing_model_least_squares(&stars, &cond).unwrap();
        assert!(model.converged);
        assert_eq!(model.star_count, 4);
        assert!(model.rms_residual_arcsec < 25.0);
    }
}
