#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasurementSummary {
    pub mean: f64,
    pub rms: f64,
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
}
