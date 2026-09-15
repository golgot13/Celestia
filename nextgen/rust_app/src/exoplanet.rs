use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct ExoplanetSystem {
    pub name: &'static str,
    pub planet_radius_ratio_k: f64, // Rp / R*
    pub semi_major_axis_stellar_radii_a: f64, // a / R*
    pub orbital_period_days: f64,
    pub inclination_deg: f64,
    pub transit_epoch_jd: f64,
    pub limb_darkening_u1: f64, // Linear limb darkening coeff
    pub limb_darkening_u2: f64, // Quadratic limb darkening coeff
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitParameters {
    pub impact_parameter_b: f64,
    pub transit_depth_fraction: f64,
    pub transit_depth_ppm: f64,
    pub total_duration_hours_t14: f64,
    pub full_transit_duration_hours_t23: f64,
    pub is_transiting: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitLightCurvePoint {
    pub time_jd: f64,
    pub phase: f64,
    pub projected_separation_z: f64,
    pub relative_flux: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransitFitResult {
    pub planet_radius_ratio_k: f64,
    pub transit_depth_ppm: f64,
    pub best_epoch_jd: f64,
    pub residual_rms: f64,
    pub snr_detection: f64,
    pub converged: bool,
}

pub fn compute_projected_separation(
    time_jd: f64,
    system: &ExoplanetSystem,
) -> (f64, f64) {
    let phase = ((time_jd - system.transit_epoch_jd) / system.orbital_period_days).rem_euclid(1.0);
    let mean_anomaly = 2.0 * PI * phase;

    let inc_rad = system.inclination_deg.to_radians();
    let cos_i = inc_rad.cos();

    // Normalized separation in units of stellar radius R*
    // z = a/R* * sqrt(sin^2(omega*t) + cos^2(i) * cos^2(omega*t))
    let sin_m = mean_anomaly.sin();
    let cos_m = mean_anomaly.cos();

    let x = system.semi_major_axis_stellar_radii_a * sin_m;
    let y = system.semi_major_axis_stellar_radii_a * cos_m * cos_i;

    let z = (x * x + y * y).sqrt();
    (phase, z)
}

pub fn compute_mandel_agol_flux(
    projected_separation_z: f64,
    radius_ratio_k: f64,
    u1: f64,
    u2: f64,
) -> f64 {
    let z = projected_separation_z.max(0.0);
    let p = radius_ratio_k.clamp(0.0001, 0.999);

    // Case 1: No occultation (planet outside stellar disk)
    if z >= 1.0 + p {
        return 1.0;
    }

    // Occultation area calculation (overlap between star circle R=1 and planet circle R=p)
    let lambda_e = if z <= 1.0 - p {
        // Full occultation (planet completely inside stellar disk)
        p * p
    } else {
        // Ingress / Egress partial occultation
        let z2 = z * z;
        let p2 = p * p;
        let kappa0 = ((p2 + z2 - 1.0) / (2.0 * p * z)).clamp(-1.0, 1.0).acos();
        let kappa1 = ((1.0 - p2 + z2) / (2.0 * z)).clamp(-1.0, 1.0).acos();
        let term = (4.0 * z2 - (1.0 + z2 - p2).powi(2)).max(0.0).sqrt() * 0.5;
        (p2 * kappa0 + kappa1 - term) / PI
    };

    // Quadratic limb darkening correction factor: I(r) = 1 - u1*(1-mu) - u2*(1-mu)^2
    let mu = (1.0 - (z.min(1.0)).powi(2)).max(0.0).sqrt();
    let intensity = 1.0 - u1 * (1.0 - mu) - u2 * (1.0 - mu).powi(2);
    let intensity_norm = 1.0 - u1 / 3.0 - u2 / 6.0; // Total flux normalization factor

    let limb_darkening_weight = (intensity / intensity_norm.max(1e-6)).clamp(0.1, 2.0);

    1.0 - lambda_e * limb_darkening_weight
}

pub fn compute_transit_parameters(system: &ExoplanetSystem) -> TransitParameters {
    let k = system.planet_radius_ratio_k;
    let a = system.semi_major_axis_stellar_radii_a;
    let i_rad = system.inclination_deg.to_radians();

    let impact_parameter_b = a * i_rad.cos();
    let is_transiting = impact_parameter_b < (1.0 + k);

    let depth_fraction = k * k;
    let depth_ppm = depth_fraction * 1e6;

    if !is_transiting {
        return TransitParameters {
            impact_parameter_b,
            transit_depth_fraction: 0.0,
            transit_depth_ppm: 0.0,
            total_duration_hours_t14: 0.0,
            full_transit_duration_hours_t23: 0.0,
            is_transiting: false,
        };
    }

    // Total duration T_14 (first to fourth contact)
    let term14 = (((1.0 + k).powi(2) - impact_parameter_b.powi(2)).max(0.0).sqrt() / (a * i_rad.sin().max(1e-4))).clamp(-1.0, 1.0);
    let t14_hours = (system.orbital_period_days * 24.0 / PI) * term14.asin();

    // Full transit duration T_23 (second to third contact - flat bottom)
    let term23 = (((1.0 - k).powi(2) - impact_parameter_b.powi(2)).max(0.0).sqrt() / (a * i_rad.sin().max(1e-4))).clamp(-1.0, 1.0);
    let t23_hours = if (1.0 - k) > impact_parameter_b {
        (system.orbital_period_days * 24.0 / PI) * term23.asin()
    } else {
        0.0
    };

    TransitParameters {
        impact_parameter_b,
        transit_depth_fraction: depth_fraction,
        transit_depth_ppm: depth_ppm,
        total_duration_hours_t14: t14_hours,
        full_transit_duration_hours_t23: t23_hours,
        is_transiting: true,
    }
}

pub fn generate_transit_light_curve(
    system: &ExoplanetSystem,
    start_jd: f64,
    end_jd: f64,
    step_minutes: f64,
) -> Vec<TransitLightCurvePoint> {
    let dt_days = (step_minutes.max(0.1)) / 1440.0;
    let count = ((end_jd - start_jd) / dt_days).ceil() as usize;

    let mut points = Vec::with_capacity(count);

    for i in 0..=count {
        let t = start_jd + (i as f64) * dt_days;
        let (phase, z) = compute_projected_separation(t, system);
        let flux = compute_mandel_agol_flux(
            z,
            system.planet_radius_ratio_k,
            system.limb_darkening_u1,
            system.limb_darkening_u2,
        );

        points.push(TransitLightCurvePoint {
            time_jd: t,
            phase,
            projected_separation_z: z,
            relative_flux: flux,
        });
    }

    points
}

pub fn fit_transit_depth_least_squares(
    observed_times: &[f64],
    observed_fluxes: &[f64],
    system_initial_guess: &ExoplanetSystem,
) -> Result<TransitFitResult, String> {
    let n = observed_times.len();
    if n < 5 || n != observed_fluxes.len() {
        return Err("at least 5 observed points required for transit fitting".to_string());
    }

    let mut best_k = system_initial_guess.planet_radius_ratio_k;
    let mut best_rms = f64::INFINITY;

    // Grid search refinement on radius ratio k around initial guess
    for step in -30..=30 {
        let test_k = (system_initial_guess.planet_radius_ratio_k + (step as f64) * 0.002).max(0.005);
        let mut sq_err = 0.0;

        for (&t, &obs_f) in observed_times.iter().zip(observed_fluxes) {
            let (_, z) = compute_projected_separation(t, system_initial_guess);
            let model_f = compute_mandel_agol_flux(
                z,
                test_k,
                system_initial_guess.limb_darkening_u1,
                system_initial_guess.limb_darkening_u2,
            );
            sq_err += (obs_f - model_f).powi(2);
        }

        let rms = (sq_err / n as f64).sqrt();
        if rms < best_rms {
            best_rms = rms;
            best_k = test_k;
        }
    }

    let transit_depth_ppm = best_k * best_k * 1e6;
    let snr = (best_k * best_k) / best_rms.max(1e-6);

    Ok(TransitFitResult {
        planet_radius_ratio_k: best_k,
        transit_depth_ppm,
        best_epoch_jd: system_initial_guess.transit_epoch_jd,
        residual_rms: best_rms,
        snr_detection: snr,
        converged: best_rms < 0.005,
    })
}

pub fn transit_parameters_to_json(params: &TransitParameters, system: &ExoplanetSystem) -> String {
    format!(
        "{{\n  \"system_name\": \"{}\",\n  \"planet_radius_ratio_k\": {:.4},\n  \"impact_parameter_b\": {:.4},\n  \"transit_depth_ppm\": {:.1},\n  \"total_duration_hours_t14\": {:.2},\n  \"full_transit_duration_hours_t23\": {:.2},\n  \"is_transiting\": {}\n}}\n",
        system.name,
        system.planet_radius_ratio_k,
        params.impact_parameter_b,
        params.transit_depth_ppm,
        params.total_duration_hours_t14,
        params.full_transit_duration_hours_t23,
        params.is_transiting
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_hd209458b_transit_parameters() {
        // HD 209458 b (Osiris) benchmark system: Rp/R* ~ 0.121, depth ~ 1.46% (14600 ppm), duration ~ 3.05 hours
        let osiris = ExoplanetSystem {
            name: "HD 209458 b",
            planet_radius_ratio_k: 0.1208,
            semi_major_axis_stellar_radii_a: 8.76,
            orbital_period_days: 3.52474859,
            inclination_deg: 86.71,
            transit_epoch_jd: 2452826.628521,
            limb_darkening_u1: 0.35,
            limb_darkening_u2: 0.20,
        };

        let params = compute_transit_parameters(&osiris);
        assert!(params.is_transiting);
        assert!((params.impact_parameter_b - 0.50).abs() < 0.05);
        assert!((params.transit_depth_ppm - 14592.0).abs() < 50.0);
        assert!(params.total_duration_hours_t14 > 2.8 && params.total_duration_hours_t14 < 3.3);
    }

    #[test]
    fn generates_and_fits_synthetic_exoplanet_transit() {
        let system = ExoplanetSystem {
            name: "WASP-12 b",
            planet_radius_ratio_k: 0.117,
            semi_major_axis_stellar_radii_a: 3.05,
            orbital_period_days: 1.09142,
            inclination_deg: 83.37,
            transit_epoch_jd: 2454500.0,
            limb_darkening_u1: 0.30,
            limb_darkening_u2: 0.15,
        };

        let curve = generate_transit_light_curve(&system, 2454499.9, 2454500.1, 5.0);
        assert!(curve.len() >= 50);

        let min_flux = curve.iter().map(|p| p.relative_flux).fold(f64::INFINITY, f64::min);
        assert!(min_flux < 0.985); // Transit depth > 1.5%

        let times: Vec<f64> = curve.iter().map(|p| p.time_jd).collect();
        let fluxes: Vec<f64> = curve.iter().map(|p| p.relative_flux).collect();

        let fit = fit_transit_depth_least_squares(&times, &fluxes, &system).unwrap();
        assert!(fit.converged);
        assert!((fit.planet_radius_ratio_k - 0.117).abs() < 0.005);
        assert!(fit.snr_detection > 20.0);
    }
}
