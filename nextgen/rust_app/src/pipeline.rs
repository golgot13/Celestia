use crate::calibration::{CalibrationFrame, detect_sources};

#[derive(Clone, Debug, PartialEq)]
pub struct ReductionRequest {
    pub width: usize,
    pub height: usize,
    pub image: Vec<f64>,
    pub bias: f64,
    pub dark_current: f64,
    pub flat_field: f64,
    pub threshold: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SequenceReductionResult {
    pub median_signal: f64,
    pub source_count: usize,
    pub total_flux: f64,
    pub valid: bool,
}

pub fn reduce_sequence(request: ReductionRequest) -> SequenceReductionResult {
    if request.width == 0
        || request.height == 0
        || request.image.is_empty()
        || request.width * request.height != request.image.len()
    {
        return SequenceReductionResult {
            median_signal: 0.0,
            source_count: 0,
            total_flux: 0.0,
            valid: false,
        };
    }

    let calibration = CalibrationFrame {
        bias: request.bias,
        dark_current: request.dark_current,
        flat_field: request.flat_field,
    };

    let reduced = crate::calibration::calibrate_frame_2d(
        &request.image,
        request.width,
        request.height,
        calibration,
    );

    let mut sorted = reduced.pixels.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_signal = if sorted.is_empty() {
        0.0
    } else {
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    };

    let sources = detect_sources(&reduced.pixels, request.width, request.height, request.threshold, 16);
    let total_flux = sources.iter().map(|source| source.flux).sum::<f64>();

    SequenceReductionResult {
        median_signal,
        source_count: sources.len(),
        total_flux,
        valid: reduced.width > 0
            && reduced.height > 0
            && reduced.pixels.len() == request.width * request.height
            && (source_count_is_reasonable(&sources) || median_signal.is_finite()),
    }
}

fn source_count_is_reasonable(sources: &[crate::calibration::SourceDetection]) -> bool {
    !sources.is_empty() && sources.len() <= 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_sequence_returns_valid_summary_for_synthetic_data() {
        let width = 7usize;
        let height = 7usize;
        let mut image = vec![10.0; width * height];

        for y in 0..height {
            for x in 0..width {
                let dx = x as f64 - 3.0;
                let dy = y as f64 - 3.0;
                let star = 200.0 * (-((dx * dx + dy * dy) / (2.0 * 1.2 * 1.2))).exp();
                image[y * width + x] = 10.0 + star;
            }
        }

        let result = reduce_sequence(ReductionRequest {
            width,
            height,
            image,
            bias: 5.0,
            dark_current: 1.0,
            flat_field: 2.0,
            threshold: 25.0,
        });

        assert!(result.valid);
        assert!(result.source_count >= 1);
        assert!(result.total_flux > 0.0);
        assert!(result.median_signal > 0.0);
    }
}
