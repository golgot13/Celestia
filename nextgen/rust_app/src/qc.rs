#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityFlag {
    Nominal,
    HighBackground,
    PoorSeeing,
    Saturated,
    ElongatedPSF,
    LowSignalToNoise,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StarMeasurement {
    pub star_id: usize,
    pub x: f64,
    pub y: f64,
    pub flux: f64,
    pub fwhm_pixels: f64,
    pub roundness: f64, // 1.0 is perfectly circular, <0.7 is elongated/trailed
    pub peak_adu: f64,
    pub snr: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameQualitySummary {
    pub total_detected_stars: usize,
    pub median_fwhm_pixels: f64,
    pub median_roundness: f64,
    pub median_snr: f64,
    pub background_level: f64,
    pub quality_flag: QualityFlag,
    pub is_accepted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QcThresholds {
    pub max_fwhm_pixels: f64,
    pub min_roundness: f64,
    pub min_snr: f64,
    pub max_background: f64,
    pub saturation_limit_adu: f64,
    pub min_star_count: usize,
}

impl Default for QcThresholds {
    fn default() -> Self {
        Self {
            max_fwhm_pixels: 4.5,
            min_roundness: 0.75,
            min_snr: 5.0,
            max_background: 5000.0,
            saturation_limit_adu: 60000.0,
            min_star_count: 2,
        }
    }
}

fn compute_median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = sorted.len();
    if len % 2 == 1 {
        sorted[len / 2]
    } else {
        0.5 * (sorted[len / 2 - 1] + sorted[len / 2])
    }
}

pub fn evaluate_frame_quality(
    stars: &[StarMeasurement],
    background_level: f64,
    thresholds: &QcThresholds,
) -> FrameQualitySummary {
    if stars.len() < thresholds.min_star_count {
        return FrameQualitySummary {
            total_detected_stars: stars.len(),
            median_fwhm_pixels: 0.0,
            median_roundness: 0.0,
            median_snr: 0.0,
            background_level,
            quality_flag: QualityFlag::LowSignalToNoise,
            is_accepted: false,
        };
    }

    let fwhms: Vec<f64> = stars.iter().map(|s| s.fwhm_pixels).collect();
    let roundnesses: Vec<f64> = stars.iter().map(|s| s.roundness).collect();
    let snrs: Vec<f64> = stars.iter().map(|s| s.snr).collect();

    let med_fwhm = compute_median(&fwhms);
    let med_roundness = compute_median(&roundnesses);
    let med_snr = compute_median(&snrs);

    let has_saturation = stars.iter().any(|s| s.peak_adu >= thresholds.saturation_limit_adu);

    let (quality_flag, is_accepted) = if has_saturation {
        (QualityFlag::Saturated, false)
    } else if background_level > thresholds.max_background {
        (QualityFlag::HighBackground, false)
    } else if med_fwhm > thresholds.max_fwhm_pixels {
        (QualityFlag::PoorSeeing, false)
    } else if med_roundness < thresholds.min_roundness {
        (QualityFlag::ElongatedPSF, false)
    } else if med_snr < thresholds.min_snr {
        (QualityFlag::LowSignalToNoise, false)
    } else {
        (QualityFlag::Nominal, true)
    };

    FrameQualitySummary {
        total_detected_stars: stars.len(),
        median_fwhm_pixels: med_fwhm,
        median_roundness: med_roundness,
        median_snr: med_snr,
        background_level,
        quality_flag,
        is_accepted,
    }
}

pub fn frame_quality_to_json(summary: &FrameQualitySummary) -> String {
    format!(
        "{{\n  \"total_detected_stars\": {},\n  \"median_fwhm_pixels\": {:.4},\n  \"median_roundness\": {:.4},\n  \"median_snr\": {:.4},\n  \"background_level\": {:.2},\n  \"quality_flag\": \"{:?}\",\n  \"is_accepted\": {}\n}}\n",
        summary.total_detected_stars,
        summary.median_fwhm_pixels,
        summary.median_roundness,
        summary.median_snr,
        summary.background_level,
        summary.quality_flag,
        summary.is_accepted
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_nominal_frame_acceptance() {
        let stars = vec![
            StarMeasurement {
                star_id: 1,
                x: 100.0,
                y: 100.0,
                flux: 5000.0,
                fwhm_pixels: 2.3,
                roundness: 0.95,
                peak_adu: 8000.0,
                snr: 25.0,
            },
            StarMeasurement {
                star_id: 2,
                x: 300.0,
                y: 250.0,
                flux: 3500.0,
                fwhm_pixels: 2.4,
                roundness: 0.92,
                peak_adu: 6000.0,
                snr: 18.0,
            },
        ];

        let summary = evaluate_frame_quality(&stars, 120.0, &QcThresholds::default());
        assert!(summary.is_accepted);
        assert_eq!(summary.quality_flag, QualityFlag::Nominal);
        assert!((summary.median_fwhm_pixels - 2.35).abs() < 1e-4);
    }

    #[test]
    fn rejects_frame_with_elongated_trailed_stars() {
        let stars = vec![
            StarMeasurement {
                star_id: 1,
                x: 100.0,
                y: 100.0,
                flux: 5000.0,
                fwhm_pixels: 2.5,
                roundness: 0.55, // Guiding error trail
                peak_adu: 8000.0,
                snr: 25.0,
            },
            StarMeasurement {
                star_id: 2,
                x: 300.0,
                y: 250.0,
                flux: 3500.0,
                fwhm_pixels: 2.6,
                roundness: 0.58,
                peak_adu: 6000.0,
                snr: 18.0,
            },
        ];

        let summary = evaluate_frame_quality(&stars, 120.0, &QcThresholds::default());
        assert!(!summary.is_accepted);
        assert_eq!(summary.quality_flag, QualityFlag::ElongatedPSF);
    }
}
