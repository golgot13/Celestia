#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarProfileModel {
    Gaussian,
    Moffat,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StarProfileFit {
    pub center_x: f64,
    pub center_y: f64,
    pub amplitude: f64,
    pub fwhm_pixels: f64,
    pub background: f64,
    pub residual_rms: f64,
    pub converged: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeeingAssessment {
    pub average_fwhm_pixels: f64,
    pub fwhm_arcsec: f64,
    pub plate_scale_arcsec_per_pixel: f64,
    pub star_count: usize,
    pub seeing_quality: &'static str,
}

pub fn fit_gaussian_profile_1d(data: &[f64], center_init: f64) -> StarProfileFit {
    if data.len() < 3 {
        return StarProfileFit {
            center_x: center_init,
            center_y: 0.0,
            amplitude: 0.0,
            fwhm_pixels: 0.0,
            background: 0.0,
            residual_rms: 0.0,
            converged: false,
        };
    }

    let min_val = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max_val = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let amplitude = (max_val - min_val).max(0.0);
    let background = min_val;

    if amplitude < 1e-6 {
        return StarProfileFit {
            center_x: center_init,
            center_y: 0.0,
            amplitude: 0.0,
            fwhm_pixels: 0.0,
            background,
            residual_rms: 0.0,
            converged: true,
        };
    }

    // Direct moment estimation for centroid and variance (sigma^2)
    let mut sum_w = 0.0;
    let mut sum_wx = 0.0;
    let mut sum_wxx = 0.0;

    for (i, &val) in data.iter().enumerate() {
        let w = (val - background).max(0.0);
        let x = i as f64;
        sum_w += w;
        sum_wx += w * x;
        sum_wxx += w * x * x;
    }

    let (centroid, sigma) = if sum_w > 1e-9 {
        let mean = sum_wx / sum_w;
        let var = (sum_wxx / sum_w - mean * mean).max(0.1);
        (mean, var.sqrt())
    } else {
        (center_init, 1.0)
    };

    // FWHM = 2 * sqrt(2 * ln(2)) * sigma approx 2.35482 * sigma
    let fwhm_pixels = 2.354820045 * sigma;

    // Compute residual RMS
    let mut sq_err = 0.0;
    for (i, &val) in data.iter().enumerate() {
        let x = i as f64;
        let model = background + amplitude * (-((x - centroid).powi(2)) / (2.0 * sigma * sigma)).exp();
        sq_err += (val - model).powi(2);
    }
    let residual_rms = (sq_err / data.len() as f64).sqrt();

    StarProfileFit {
        center_x: centroid,
        center_y: 0.0,
        amplitude,
        fwhm_pixels,
        background,
        residual_rms,
        converged: residual_rms < (amplitude * 0.2).max(1.0),
    }
}

pub fn fit_moffat_profile_1d(data: &[f64], center_init: f64, beta: f64) -> StarProfileFit {
    if data.len() < 3 {
        return StarProfileFit {
            center_x: center_init,
            center_y: 0.0,
            amplitude: 0.0,
            fwhm_pixels: 0.0,
            background: 0.0,
            residual_rms: 0.0,
            converged: false,
        };
    }

    let min_val = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max_val = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let amplitude = (max_val - min_val).max(0.0);
    let background = min_val;

    let mut sum_w = 0.0;
    let mut sum_wx = 0.0;
    let mut sum_wxx = 0.0;

    for (i, &val) in data.iter().enumerate() {
        let w = (val - background).max(0.0);
        let x = i as f64;
        sum_w += w;
        sum_wx += w * x;
        sum_wxx += w * x * x;
    }

    let (centroid, alpha) = if sum_w > 1e-9 {
        let mean = sum_wx / sum_w;
        let var = (sum_wxx / sum_w - mean * mean).max(0.1);
        (mean, var.sqrt())
    } else {
        (center_init, 1.0)
    };

    // For Moffat: FWHM = 2 * alpha * sqrt(2^(1/beta) - 1)
    let fwhm_factor = 2.0 * (2.0f64.powf(1.0 / beta) - 1.0).max(0.0).sqrt();
    let fwhm_pixels = alpha * fwhm_factor;

    let mut sq_err = 0.0;
    for (i, &val) in data.iter().enumerate() {
        let x = i as f64;
        let r2 = (x - centroid).powi(2);
        let model = background + amplitude / (1.0 + r2 / (alpha * alpha)).powf(beta);
        sq_err += (val - model).powi(2);
    }
    let residual_rms = (sq_err / data.len() as f64).sqrt();

    StarProfileFit {
        center_x: centroid,
        center_y: 0.0,
        amplitude,
        fwhm_pixels,
        background,
        residual_rms,
        converged: residual_rms < (amplitude * 0.25).max(1.0),
    }
}

pub fn assess_seeing_quality(
    fwhm_measurements: &[f64],
    plate_scale_arcsec_per_pixel: f64,
) -> SeeingAssessment {
    if fwhm_measurements.is_empty() {
        return SeeingAssessment {
            average_fwhm_pixels: 0.0,
            fwhm_arcsec: 0.0,
            plate_scale_arcsec_per_pixel,
            star_count: 0,
            seeing_quality: "Unknown (No stars detected)",
        };
    }

    let valid: Vec<f64> = fwhm_measurements
        .iter()
        .copied()
        .filter(|&f| f.is_finite() && f > 0.1)
        .collect();

    if valid.is_empty() {
        return SeeingAssessment {
            average_fwhm_pixels: 0.0,
            fwhm_arcsec: 0.0,
            plate_scale_arcsec_per_pixel,
            star_count: 0,
            seeing_quality: "Invalid",
        };
    }

    let avg_pixels = valid.iter().sum::<f64>() / valid.len() as f64;
    let fwhm_arcsec = avg_pixels * plate_scale_arcsec_per_pixel;

    let seeing_quality = if fwhm_arcsec < 1.0 {
        "Exceptional (<1.0\")"
    } else if fwhm_arcsec < 1.5 {
        "Excellent (1.0-1.5\")"
    } else if fwhm_arcsec < 2.5 {
        "Good (1.5-2.5\")"
    } else if fwhm_arcsec < 4.0 {
        "Fair (2.5-4.0\")"
    } else {
        "Poor (>4.0\")"
    };

    SeeingAssessment {
        average_fwhm_pixels: avg_pixels,
        fwhm_arcsec,
        plate_scale_arcsec_per_pixel,
        star_count: valid.len(),
        seeing_quality,
    }
}

pub fn seeing_assessment_to_json(assessment: &SeeingAssessment) -> String {
    format!(
        "{{\n  \"average_fwhm_pixels\": {:.4},\n  \"fwhm_arcsec\": {:.4},\n  \"plate_scale_arcsec_per_pixel\": {:.4},\n  \"star_count\": {},\n  \"seeing_quality\": \"{}\"\n}}\n",
        assessment.average_fwhm_pixels,
        assessment.fwhm_arcsec,
        assessment.plate_scale_arcsec_per_pixel,
        assessment.star_count,
        assessment.seeing_quality
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_exact_1d_gaussian_profile() {
        let sigma = 2.0;
        let center = 15.0;
        let amp = 500.0;
        let bg = 20.0;

        let profile: Vec<f64> = (0..31)
            .map(|i| {
                let x = i as f64;
                bg + amp * (-((x - center).powi(2)) / (2.0 * sigma * sigma)).exp()
            })
            .collect();

        let fit = fit_gaussian_profile_1d(&profile, 15.0);
        assert!(fit.converged);
        assert!((fit.center_x - center).abs() < 0.1);
        assert!((fit.fwhm_pixels - 2.35482 * sigma).abs() < 0.2);
        assert!((fit.amplitude - amp).abs() < 1.0);
    }

    #[test]
    fn assesses_seeing_quality_metric() {
        let fwhms = vec![2.1, 2.3, 2.0, 2.2];
        let plate_scale = 0.8; // 0.8 arcsec/pixel -> ~1.72 arcsec -> "Good"
        let assessment = assess_seeing_quality(&fwhms, plate_scale);

        assert_eq!(assessment.star_count, 4);
        assert!((assessment.average_fwhm_pixels - 2.15).abs() < 1e-4);
        assert!((assessment.fwhm_arcsec - 1.72).abs() < 1e-4);
        assert_eq!(assessment.seeing_quality, "Good (1.5-2.5\")");
    }
}
