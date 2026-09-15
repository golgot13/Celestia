#[derive(Clone, Debug, PartialEq)]
pub struct FocuserMeasurement {
    pub focuser_step: i64,
    pub hfd_pixels: f64,
    pub star_flux: f64,
    pub snr: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FocusCurveFit {
    pub optimal_step: i64,
    pub min_hfd_pixels: f64,
    pub curvature_a: f64,
    pub cfz_steps: f64,
    pub r_squared: f64,
    pub converged: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutofocusPlan {
    pub start_step: i64,
    pub end_step: i64,
    pub step_increment: i64,
    pub target_steps: Vec<i64>,
    pub backlash_steps: i64,
}

pub fn compute_half_flux_diameter(radial_fluxes: &[f64]) -> f64 {
    if radial_fluxes.is_empty() {
        return 0.0;
    }

    let bg = radial_fluxes.iter().copied().fold(f64::INFINITY, f64::min);
    let net_fluxes: Vec<f64> = radial_fluxes.iter().map(|&f| (f - bg).max(0.0)).collect();
    let total_flux: f64 = net_fluxes.iter().sum();

    if total_flux <= 1e-12 {
        return 0.0;
    }

    // Cumulative sum to find radius where flux reaches 50%
    let half_flux = total_flux * 0.5;
    let mut cum_flux = 0.0;
    let mut hfd_radius = 0.0;

    for (i, &f) in net_fluxes.iter().enumerate() {
        let prev_cum = cum_flux;
        cum_flux += f;
        if cum_flux >= half_flux {
            let r0 = i as f64;
            let r1 = (i + 1) as f64;
            let frac = if f > 1e-12 {
                (half_flux - prev_cum) / f
            } else {
                0.0
            };
            hfd_radius = r0 + frac * (r1 - r0);
            break;
        }
    }

    // HFD is twice the half-flux radius
    2.0 * hfd_radius.max(0.1)
}

pub fn compute_critical_focus_zone_steps(
    focal_ratio: f64,
    wavelength_nm: f64,
    microns_per_step: f64,
) -> f64 {
    if focal_ratio <= 0.0 || microns_per_step <= 0.0 {
        return 0.0;
    }
    // Standard optical formula for diffraction-limited Critical Focus Zone: CFZ = 4.88 * lambda * N^2
    let lambda_microns = wavelength_nm * 1e-3;
    let cfz_microns = 4.88 * lambda_microns * focal_ratio * focal_ratio;
    cfz_microns / microns_per_step
}

pub fn fit_parabolic_v_curve(
    measurements: &[FocuserMeasurement],
    cfz_steps: f64,
) -> Result<FocusCurveFit, String> {
    let n = measurements.len();
    if n < 3 {
        return Err("at least 3 focuser measurements required for V-curve fit".to_string());
    }

    // Fit parabola: HFD(x) = a * x^2 + b * x + c using normal equations
    let mut sum_x = 0.0;
    let mut sum_x2 = 0.0;
    let mut sum_x3 = 0.0;
    let mut sum_x4 = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_x2y = 0.0;

    for m in measurements {
        let x = m.focuser_step as f64;
        let y = m.hfd_pixels;
        let x2 = x * x;
        sum_x += x;
        sum_x2 += x2;
        sum_x3 += x2 * x;
        sum_x4 += x2 * x2;
        sum_y += y;
        sum_xy += x * y;
        sum_x2y += x2 * y;
    }

    let count = n as f64;

    let det = count * (sum_x2 * sum_x4 - sum_x3 * sum_x3)
        - sum_x * (sum_x * sum_x4 - sum_x2 * sum_x3)
        + sum_x2 * (sum_x * sum_x3 - sum_x2 * sum_x2);

    if det.abs() < 1e-12 {
        return Err("singular matrix in focuser curve fit".to_string());
    }

    let det_c = sum_y * (sum_x2 * sum_x4 - sum_x3 * sum_x3)
        - sum_x * (sum_xy * sum_x4 - sum_x2y * sum_x3)
        + sum_x2 * (sum_xy * sum_x3 - sum_x2y * sum_x2);

    let det_b = count * (sum_xy * sum_x4 - sum_x2y * sum_x3)
        - sum_y * (sum_x * sum_x4 - sum_x2 * sum_x3)
        + sum_x2 * (sum_x * sum_x2y - sum_x2 * sum_xy);

    let det_a = count * (sum_x2 * sum_x2y - sum_x3 * sum_xy)
        - sum_x * (sum_x * sum_x2y - sum_x2 * sum_xy)
        + sum_y * (sum_x * sum_x3 - sum_x2 * sum_x2);

    let c = det_c / det;
    let b = det_b / det;
    let a = det_a / det;

    if a <= 1e-12 {
        return Err("fitted curve is not convex (a <= 0)".to_string());
    }

    // Parabola vertex: x0 = -b / (2a)
    let optimal_step = (-b / (2.0 * a)).round() as i64;
    let min_hfd = (c - (b * b) / (4.0 * a)).max(0.1);

    // Compute R^2 goodness of fit
    let mean_y = sum_y / count;
    let ss_tot: f64 = measurements.iter().map(|m| (m.hfd_pixels - mean_y).powi(2)).sum();
    let ss_res: f64 = measurements
        .iter()
        .map(|m| {
            let x = m.focuser_step as f64;
            let pred = a * x * x + b * x + c;
            (m.hfd_pixels - pred).powi(2)
        })
        .sum();

    let r_squared = if ss_tot > 1e-12 {
        (1.0 - ss_res / ss_tot).clamp(0.0, 1.0)
    } else {
        1.0
    };

    Ok(FocusCurveFit {
        optimal_step,
        min_hfd_pixels: min_hfd,
        curvature_a: a,
        cfz_steps,
        r_squared,
        converged: r_squared > 0.85,
    })
}

pub fn plan_autofocus_run(
    current_step: i64,
    step_span: i64,
    num_samples: usize,
    backlash_steps: i64,
) -> AutofocusPlan {
    let samples = num_samples.max(5);
    let half_span = step_span / 2;
    let start_step = current_step - half_span;
    let end_step = current_step + half_span;
    let step_increment = (step_span / (samples as i64 - 1)).max(1);

    let mut target_steps = Vec::with_capacity(samples);
    let mut step = start_step;
    for _ in 0..samples {
        target_steps.push(step);
        step += step_increment;
    }

    AutofocusPlan {
        start_step,
        end_step,
        step_increment,
        target_steps,
        backlash_steps,
    }
}

pub fn focus_curve_fit_to_json(fit: &FocusCurveFit) -> String {
    format!(
        "{{\n  \"optimal_step\": {},\n  \"min_hfd_pixels\": {:.4},\n  \"curvature_a\": {:.6e},\n  \"cfz_steps\": {:.2},\n  \"r_squared\": {:.4},\n  \"converged\": {}\n}}\n",
        fit.optimal_step,
        fit.min_hfd_pixels,
        fit.curvature_a,
        fit.cfz_steps,
        fit.r_squared,
        fit.converged
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_hfd_from_radial_profile() {
        let profile = vec![100.0, 80.0, 50.0, 20.0, 5.0, 1.0];
        let hfd = compute_half_flux_diameter(&profile);
        assert!(hfd > 1.0 && hfd < 5.0);
    }

    #[test]
    fn fits_exact_v_curve_and_finds_minimum() {
        // True focus at step 25000 with min HFD 2.0 pixels
        let true_center = 25000.0;
        let true_min_hfd = 2.0;
        let a = 0.000008; // curvature

        let steps = vec![23000, 24000, 24500, 25000, 25500, 26000, 27000];
        let measurements: Vec<FocuserMeasurement> = steps
            .iter()
            .map(|&s| {
                let dx = s as f64 - true_center;
                let hfd = true_min_hfd + a * dx * dx;
                FocuserMeasurement {
                    focuser_step: s,
                    hfd_pixels: hfd,
                    star_flux: 25000.0,
                    snr: 45.0,
                }
            })
            .collect();

        let cfz = compute_critical_focus_zone_steps(5.0, 550.0, 2.5); // f/5, 550nm, 2.5 um/step -> ~26.8 steps
        assert!(cfz > 20.0 && cfz < 35.0);

        let fit = fit_parabolic_v_curve(&measurements, cfz).unwrap();
        assert!(fit.converged);
        assert_eq!(fit.optimal_step, 25000);
        assert!((fit.min_hfd_pixels - true_min_hfd).abs() < 1e-4);
        assert!(fit.r_squared > 0.99);
    }

    #[test]
    fn generates_autofocus_plan_with_backlash() {
        let plan = plan_autofocus_run(25000, 2000, 5, 100);
        assert_eq!(plan.target_steps.len(), 5);
        assert_eq!(plan.target_steps[0], 24000);
        assert_eq!(plan.target_steps[4], 26000);
        assert_eq!(plan.step_increment, 500);
        assert_eq!(plan.backlash_steps, 100);
    }
}
