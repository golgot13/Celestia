use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct StandardStar {
    pub name: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub catalog_magnitudes: HashMap<&'static str, f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedStarPhotometry {
    pub star_name: &'static str,
    pub filter: &'static str,
    pub instrumental_flux: f64,
    pub exposure_s: f64,
    pub airmass: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ZeroPointCalibration {
    pub filter: String,
    pub zero_point_mag: f64,
    pub extinction_coefficient: f64,
    pub residual_rms_mag: f64,
    pub reference_star_count: usize,
    pub valid: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalibratedStarMagnitude {
    pub star_name: &'static str,
    pub filter: &'static str,
    pub instrumental_mag: f64,
    pub calibrated_mag: f64,
    pub error_estimate_mag: f64,
}

pub fn compute_instrumental_magnitude(flux: f64, exposure_s: f64) -> Result<f64, String> {
    if flux <= 1e-12 {
        return Err("flux must be strictly positive for magnitude calculation".to_string());
    }
    if exposure_s <= 1e-12 {
        return Err("exposure time must be strictly positive".to_string());
    }

    let flux_rate = flux / exposure_s;
    // Standard astronomical instrumental magnitude formula: -2.5 * log10(flux/exposure)
    let inst_mag = -2.5 * flux_rate.log10();
    Ok(inst_mag)
}

pub fn solve_zero_point_and_extinction(
    observations: &[ObservedStarPhotometry],
    catalog: &[StandardStar],
    filter: &str,
) -> Result<ZeroPointCalibration, String> {
    let mut catalog_map: HashMap<&str, f64> = HashMap::new();
    for star in catalog {
        if let Some(&mag) = star.catalog_magnitudes.get(filter) {
            catalog_map.insert(star.name, mag);
        }
    }

    let mut points: Vec<(f64, f64)> = Vec::new(); // (airmass X, delta_m = m_cat - m_inst)

    for obs in observations {
        if obs.filter != filter {
            continue;
        }
        if let Some(&cat_mag) = catalog_map.get(obs.star_name) {
            if let Ok(inst_mag) = compute_instrumental_magnitude(obs.instrumental_flux, obs.exposure_s) {
                let delta_m = cat_mag - inst_mag;
                points.push((obs.airmass, delta_m));
            }
        }
    }

    if points.is_empty() {
        return Err(format!("no standard stars found for filter '{filter}'"));
    }

    // Solve for m_cat - m_inst = ZP - k * X
    // Linear regression: Y = ZP + slope * X where slope = -k
    let n = points.len() as f64;
    let sum_x = points.iter().map(|(x, _)| *x).sum::<f64>();
    let sum_y = points.iter().map(|(_, y)| *y).sum::<f64>();
    let sum_xx = points.iter().map(|(x, _)| x * x).sum::<f64>();
    let sum_xy = points.iter().map(|(x, y)| x * y).sum::<f64>();

    let denom = n * sum_xx - sum_x * sum_x;

    let (zp, extinction) = if denom.abs() > 1e-9 && points.len() >= 2 {
        let slope = (n * sum_xy - sum_x * sum_y) / denom;
        let intercept = (sum_y - slope * sum_x) / n;
        (intercept, -slope)
    } else {
        // Fallback with fixed default atmospheric extinction k=0.15 mag/airmass if only 1 point
        let avg_y = sum_y / n;
        let avg_x = sum_x / n;
        let k = 0.15;
        let intercept = avg_y + k * avg_x;
        (intercept, k)
    };

    let mut sq_err = 0.0;
    for (x, y) in &points {
        let predicted_y = zp - extinction * x;
        sq_err += (y - predicted_y).powi(2);
    }
    let rms = (sq_err / n).sqrt();

    Ok(ZeroPointCalibration {
        filter: filter.to_string(),
        zero_point_mag: zp,
        extinction_coefficient: extinction,
        residual_rms_mag: rms,
        reference_star_count: points.len(),
        valid: rms < 0.25,
    })
}

pub fn calibrate_target_magnitude(
    obs: &ObservedStarPhotometry,
    cal: &ZeroPointCalibration,
) -> Result<CalibratedStarMagnitude, String> {
    let inst_mag = compute_instrumental_magnitude(obs.instrumental_flux, obs.exposure_s)?;
    // Calibrated magnitude: m_cal = m_inst + ZP - k * X
    let calibrated_mag = inst_mag + cal.zero_point_mag - cal.extinction_coefficient * obs.airmass;

    Ok(CalibratedStarMagnitude {
        star_name: obs.star_name,
        filter: obs.filter,
        instrumental_mag: inst_mag,
        calibrated_mag,
        error_estimate_mag: cal.residual_rms_mag,
    })
}

pub fn calibration_result_to_json(cal: &ZeroPointCalibration) -> String {
    format!(
        "{{\n  \"filter\": \"{}\",\n  \"zero_point_mag\": {:.4},\n  \"extinction_coefficient\": {:.4},\n  \"residual_rms_mag\": {:.4},\n  \"reference_star_count\": {},\n  \"valid\": {}\n}}\n",
        cal.filter,
        cal.zero_point_mag,
        cal.extinction_coefficient,
        cal.residual_rms_mag,
        cal.reference_star_count,
        cal.valid
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_zero_point_and_calibrates_magnitudes() {
        let mut cat_mags = HashMap::new();
        cat_mags.insert("V", 10.0);

        let catalog = vec![
            StandardStar {
                name: "StarA",
                ra_deg: 10.0,
                dec_deg: 20.0,
                catalog_magnitudes: cat_mags,
            },
        ];

        // Star with known flux producing inst_mag = -12.5 (flux 100000, exp 1s)
        // With ZP = 22.5, at airmass 1.0 with extinction 0.15, calibrated = -12.5 + 22.5 - 0.15 = 9.85
        let obs = vec![
            ObservedStarPhotometry {
                star_name: "StarA",
                filter: "V",
                instrumental_flux: 100000.0,
                exposure_s: 1.0,
                airmass: 1.0,
            },
        ];

        let cal = solve_zero_point_and_extinction(&obs, &catalog, "V").unwrap();
        assert_eq!(cal.filter, "V");
        assert!(cal.valid);

        let target_cal = calibrate_target_magnitude(&obs[0], &cal).unwrap();
        assert!((target_cal.calibrated_mag - 10.0).abs() < 1e-6);
    }
}
