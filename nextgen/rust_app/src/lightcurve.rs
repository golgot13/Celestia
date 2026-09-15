use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct PhotometricPoint {
    pub time_jd: f64,
    pub target_flux: f64,
    pub comparison_fluxes: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DifferentialMeasurement {
    pub time_jd: f64,
    pub differential_mag: f64,
    pub phase: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeriodogramPeak {
    pub period_days: f64,
    pub frequency_cycles_per_day: f64,
    pub power: f64,
    pub false_alarm_probability: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LightCurveAnalysis {
    pub point_count: usize,
    pub mean_magnitude: f64,
    pub magnitude_amplitude: f64,
    pub best_period_days: f64,
    pub best_power: f64,
    pub light_curve: Vec<DifferentialMeasurement>,
}

pub fn compute_differential_photometry(
    points: &[PhotometricPoint],
    period_days: Option<f64>,
    epoch_jd: Option<f64>,
) -> Result<Vec<DifferentialMeasurement>, String> {
    if points.is_empty() {
        return Err("points slice cannot be empty".to_string());
    }

    let t0 = epoch_jd.unwrap_or(points[0].time_jd);
    let p = period_days.unwrap_or(1.0);
    if p <= 1e-12 {
        return Err("period must be positive".to_string());
    }

    let mut result = Vec::with_capacity(points.len());

    for pt in points {
        if pt.target_flux <= 1e-12 {
            continue;
        }

        let comp_sum: f64 = pt.comparison_fluxes.iter().copied().filter(|&f| f > 1e-12).sum();
        let comp_count = pt.comparison_fluxes.iter().filter(|&&f| f > 1e-12).count();

        let ensemble_flux = if comp_count > 0 {
            comp_sum / comp_count as f64
        } else {
            1.0
        };

        if ensemble_flux <= 1e-12 {
            continue;
        }

        // Differential magnitude: -2.5 * log10(target / ensemble)
        let diff_mag = -2.5 * (pt.target_flux / ensemble_flux).log10();
        let phase = ((pt.time_jd - t0) / p).rem_euclid(1.0);

        result.push(DifferentialMeasurement {
            time_jd: pt.time_jd,
            differential_mag: diff_mag,
            phase,
        });
    }

    Ok(result)
}

pub fn compute_lomb_scargle_periodogram(
    times: &[f64],
    values: &[f64],
    min_period_days: f64,
    max_period_days: f64,
    num_frequencies: usize,
) -> Result<Vec<PeriodogramPeak>, String> {
    let n = times.len();
    if n < 3 {
        return Err("at least 3 data points required for Lomb-Scargle".to_string());
    }
    if min_period_days <= 0.0 || max_period_days <= min_period_days {
        return Err("invalid period range".to_string());
    }
    if num_frequencies < 2 {
        return Err("num_frequencies must be >= 2".to_string());
    }

    let mean_y = values.iter().sum::<f64>() / n as f64;
    let var_y = values.iter().map(|&y| (y - mean_y).powi(2)).sum::<f64>() / (n - 1) as f64;
    if var_y <= 1e-15 {
        return Err("constant signal variance is zero".to_string());
    }

    let y_centered: Vec<f64> = values.iter().map(|&y| y - mean_y).collect();

    let f_min = 1.0 / max_period_days;
    let f_max = 1.0 / min_period_days;
    let df = (f_max - f_min) / (num_frequencies - 1) as f64;

    let mut peaks = Vec::with_capacity(num_frequencies);

    for i in 0..num_frequencies {
        let f = f_min + i as f64 * df;
        let omega = 2.0 * PI * f;

        // Compute tau time offset: tan(2 * omega * tau) = sum(sin(2*omega*t)) / sum(cos(2*omega*t))
        let mut sum_s2 = 0.0;
        let mut sum_c2 = 0.0;
        for &t in times {
            let angle = 2.0 * omega * t;
            sum_s2 += angle.sin();
            sum_c2 += angle.cos();
        }
        let tau = (sum_s2.atan2(sum_c2)) / (2.0 * omega);

        let mut sum_yc = 0.0;
        let mut sum_ys = 0.0;
        let mut sum_cc = 0.0;
        let mut sum_ss = 0.0;

        for (&t, &yc) in times.iter().zip(&y_centered) {
            let phase_arg = omega * (t - tau);
            let cos_val = phase_arg.cos();
            let sin_val = phase_arg.sin();

            sum_yc += yc * cos_val;
            sum_ys += yc * sin_val;
            sum_cc += cos_val * cos_val;
            sum_ss += sin_val * sin_val;
        }

        let power = if sum_cc > 1e-12 && sum_ss > 1e-12 {
            0.5 * ((sum_yc * sum_yc) / sum_cc + (sum_ys * sum_ys) / sum_ss) / var_y
        } else {
            0.0
        };

        // False Alarm Probability (FAP) approximation: FAP = 1 - (1 - exp(-power))^N_eff
        let exp_p = (-power).exp();
        let fap = (1.0 - (1.0 - exp_p).powi(num_frequencies as i32)).clamp(0.0, 1.0);

        peaks.push(PeriodogramPeak {
            period_days: 1.0 / f,
            frequency_cycles_per_day: f,
            power,
            false_alarm_probability: fap,
        });
    }

    Ok(peaks)
}

pub fn analyze_light_curve(
    points: &[PhotometricPoint],
    min_period_days: f64,
    max_period_days: f64,
    frequency_resolution: usize,
) -> Result<LightCurveAnalysis, String> {
    if points.is_empty() {
        return Err("cannot analyze empty photometric dataset".to_string());
    }

    let preliminary = compute_differential_photometry(points, None, None)?;
    let times: Vec<f64> = preliminary.iter().map(|p| p.time_jd).collect();
    let mags: Vec<f64> = preliminary.iter().map(|p| p.differential_mag).collect();

    let peaks = compute_lomb_scargle_periodogram(
        &times,
        &mags,
        min_period_days,
        max_period_days,
        frequency_resolution,
    )?;

    let best_peak = peaks
        .iter()
        .max_by(|a, b| a.power.partial_cmp(&b.power).unwrap_or(std::cmp::Ordering::Equal))
        .ok_or_else(|| "no periodogram peak found".to_string())?;

    let folded_curve = compute_differential_photometry(points, Some(best_peak.period_days), Some(times[0]))?;

    let mean_mag = mags.iter().sum::<f64>() / mags.len() as f64;
    let min_mag = mags.iter().copied().fold(f64::INFINITY, f64::min);
    let max_mag = mags.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let amplitude = max_mag - min_mag;

    Ok(LightCurveAnalysis {
        point_count: points.len(),
        mean_magnitude: mean_mag,
        magnitude_amplitude: amplitude,
        best_period_days: best_peak.period_days,
        best_power: best_peak.power,
        light_curve: folded_curve,
    })
}

pub fn light_curve_analysis_to_json(analysis: &LightCurveAnalysis) -> String {
    format!(
        "{{\n  \"point_count\": {},\n  \"mean_magnitude\": {:.4},\n  \"magnitude_amplitude\": {:.4},\n  \"best_period_days\": {:.6},\n  \"best_power\": {:.4}\n}}\n",
        analysis.point_count,
        analysis.mean_magnitude,
        analysis.magnitude_amplitude,
        analysis.best_period_days,
        analysis.best_power
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_exact_periodic_variation_with_lomb_scargle() {
        let true_period = 2.5; // 2.5 days period
        let n = 50;

        let mut points = Vec::with_capacity(n);
        for i in 0..n {
            let t = 2459000.0 + (i as f64) * 0.2; // Sampled every 4.8 hours
            let flux_val = 10000.0 * (1.0 + 0.2 * (2.0 * PI * (t - 2459000.0) / true_period).sin());
            points.push(PhotometricPoint {
                time_jd: t,
                target_flux: flux_val,
                comparison_fluxes: vec![10000.0, 10000.0],
            });
        }

        let analysis = analyze_light_curve(&points, 1.0, 5.0, 200).unwrap();
        assert_eq!(analysis.point_count, n);
        assert!((analysis.best_period_days - true_period).abs() < 0.1);
        assert!(analysis.best_power > 10.0);
        assert!(analysis.magnitude_amplitude > 0.3);
    }
}
