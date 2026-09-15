#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasurementSummary {
    pub mean: f64,
    pub rms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CentroidEstimate {
    pub x: f64,
    pub y: f64,
    pub peak: f64,
    pub background: f64,
    pub flux: f64,
    pub snr: f64,
}

pub fn summarize_samples(samples: &[f64]) -> MeasurementSummary {
    if samples.is_empty() {
        return MeasurementSummary { mean: 0.0, rms: 0.0 };
    }

    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let rms = (samples.iter().map(|value| {
        let delta = value - mean;
        delta * delta
    }).sum::<f64>() / samples.len() as f64).sqrt();

    MeasurementSummary { mean, rms }
}

pub fn estimate_star_centroid(frame: &[f64], width: usize, height: usize) -> CentroidEstimate {
    if frame.is_empty() || width == 0 || height == 0 || width * height != frame.len() {
        return CentroidEstimate {
            x: 0.0,
            y: 0.0,
            peak: 0.0,
            background: 0.0,
            flux: 0.0,
            snr: 0.0,
        };
    }

    let mut peak = 0.0_f64;
    let mut peak_x = 0usize;
    let mut peak_y = 0usize;
    let mut background_sum = 0.0_f64;

    for y in 0..height {
        for x in 0..width {
            let value = frame[y * width + x];
            if value > peak {
                peak = value;
                peak_x = x;
                peak_y = y;
            }
            background_sum += value;
        }
    }

    let background = background_sum / frame.len() as f64;
    let mut numerator_x = 0.0_f64;
    let mut numerator_y = 0.0_f64;
    let mut denominator = 0.0_f64;
    let mut flux = 0.0_f64;

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let value = frame[idx] - background;
            let weight = value.max(0.0);
            numerator_x += x as f64 * weight;
            numerator_y += y as f64 * weight;
            denominator += weight;
            flux += weight;
        }
    }

    let x = if denominator > 0.0 { numerator_x / denominator } else { peak_x as f64 };
    let y = if denominator > 0.0 { numerator_y / denominator } else { peak_y as f64 };

    let variance = (frame.iter().map(|value| (value - background).powi(2)).sum::<f64>() / frame.len() as f64).max(1e-12);
    let noise = variance.sqrt();
    let snr = if noise > 0.0 { (peak - background) / noise } else { 0.0 };

    CentroidEstimate {
        x,
        y,
        peak,
        background,
        flux,
        snr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_matches_reference_values() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0];
        let summary = summarize_samples(&data);

        assert!((summary.mean - 3.0).abs() < 1e-12);
        assert!((summary.rms - 1.4142135623730951).abs() < 1e-12);
    }

    #[test]
    fn centroid_estimates_known_gaussian_star() {
        let width = 9usize;
        let height = 9usize;
        let mut frame = vec![0.0; width * height];
        let cx = 4.2_f64;
        let cy = 3.8_f64;
        let sigma = 1.3_f64;

        for y in 0..height {
            for x in 0..width {
                let dx = x as f64 - cx;
                let dy = y as f64 - cy;
                let value = 100.0 * (-((dx * dx + dy * dy) / (2.0 * sigma * sigma))).exp() + 5.0;
                frame[y * width + x] = value;
            }
        }

        let centroid = estimate_star_centroid(&frame, width, height);
        assert!((centroid.x - cx).abs() < 0.6);
        assert!((centroid.y - cy).abs() < 0.6);
        assert!(centroid.flux > 0.0);
        assert!(centroid.snr > 0.0);
    }
}
