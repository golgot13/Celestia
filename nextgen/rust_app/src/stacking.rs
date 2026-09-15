#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackingMethod {
    Average,
    Median,
    SigmaClipping,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StackingParams {
    pub method: StackingMethod,
    pub sigma_clip_low: f64,
    pub sigma_clip_high: f64,
    pub max_iterations: usize,
}

impl Default for StackingParams {
    fn default() -> Self {
        Self {
            method: StackingMethod::SigmaClipping,
            sigma_clip_low: 3.0,
            sigma_clip_high: 3.0,
            max_iterations: 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StackedResult {
    pub width: usize,
    pub height: usize,
    pub frame_count: usize,
    pub stacked_image: Vec<f64>,
    pub noise_std_dev: f64,
    pub snr_improvement_factor: f64,
}

fn compute_median_inplace(slice: &mut [f64]) -> f64 {
    if slice.is_empty() {
        return 0.0;
    }
    slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = slice.len();
    if len % 2 == 1 {
        slice[len / 2]
    } else {
        0.5 * (slice[len / 2 - 1] + slice[len / 2])
    }
}

fn stack_pixel_sigma_clip(
    values: &[f64],
    low_sigma: f64,
    high_sigma: f64,
    max_iters: usize,
) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    if values.len() == 1 {
        return values[0];
    }

    let mut current = values.to_vec();

    for _ in 0..max_iters {
        if current.len() <= 2 {
            break;
        }

        let mean = current.iter().sum::<f64>() / current.len() as f64;
        let var = current
            .iter()
            .map(|&v| (v - mean).powi(2))
            .sum::<f64>()
            / (current.len() - 1) as f64;
        let std_dev = var.sqrt();

        if std_dev < 1e-12 {
            break;
        }

        let low_thresh = mean - low_sigma * std_dev;
        let high_thresh = mean + high_sigma * std_dev;

        let filtered: Vec<f64> = current
            .iter()
            .copied()
            .filter(|&v| v >= low_thresh && v <= high_thresh)
            .collect();

        if filtered.len() == current.len() {
            break;
        }
        current = filtered;
    }

    current.iter().sum::<f64>() / current.len() as f64
}

pub fn stack_frames_2d(
    frames: &[Vec<f64>],
    width: usize,
    height: usize,
    params: &StackingParams,
) -> Result<StackedResult, String> {
    if frames.is_empty() {
        return Err("cannot stack empty frame list".to_string());
    }

    let expected_len = width * height;
    for (i, frame) in frames.iter().enumerate() {
        if frame.len() != expected_len {
            return Err(format!(
                "frame {} size {} does not match expected {}",
                i,
                frame.len(),
                expected_len
            ));
        }
    }

    let frame_count = frames.len();
    let mut stacked_image = vec![0.0; expected_len];
    let mut pixel_stack = vec![0.0; frame_count];

    for idx in 0..expected_len {
        for f in 0..frame_count {
            pixel_stack[f] = frames[f][idx];
        }

        let val = match params.method {
            StackingMethod::Average => {
                pixel_stack.iter().sum::<f64>() / frame_count as f64
            }
            StackingMethod::Median => {
                let mut copy = pixel_stack.clone();
                compute_median_inplace(&mut copy)
            }
            StackingMethod::SigmaClipping => stack_pixel_sigma_clip(
                &pixel_stack,
                params.sigma_clip_low,
                params.sigma_clip_high,
                params.max_iterations,
            ),
        };

        stacked_image[idx] = val;
    }

    // Estimate noise standard deviation from background residuals
    let mean_val = stacked_image.iter().sum::<f64>() / expected_len as f64;
    let var = stacked_image
        .iter()
        .map(|&v| (v - mean_val).powi(2))
        .sum::<f64>()
        / expected_len.max(1) as f64;
    let noise_std_dev = var.sqrt();

    let snr_improvement_factor = (frame_count as f64).sqrt();

    Ok(StackedResult {
        width,
        height,
        frame_count,
        stacked_image,
        noise_std_dev,
        snr_improvement_factor,
    })
}

pub fn stacked_result_to_json(result: &StackedResult) -> String {
    format!(
        "{{\n  \"width\": {},\n  \"height\": {},\n  \"frame_count\": {},\n  \"noise_std_dev\": {:.6},\n  \"snr_improvement_factor\": {:.4}\n}}\n",
        result.width,
        result.height,
        result.frame_count,
        result.noise_std_dev,
        result.snr_improvement_factor
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_frames_with_sigma_clipping_rejecting_cosmic_ray() {
        let width = 4;
        let height = 4;
        let signal = 100.0;

        let mut f1 = vec![signal; 16];
        let mut f2 = vec![signal; 16];
        let mut f3 = vec![signal; 16];
        let mut f4 = vec![signal; 16];
        let mut f5 = vec![signal; 16];

        // Cosmic ray outlier in frame 3 at pixel (2,2) -> index 10
        f3[10] = 50000.0;

        let frames = vec![f1, f2, f3, f4, f5];
        let params = StackingParams::default();

        let result = stack_frames_2d(&frames, width, height, &params).unwrap();
        assert_eq!(result.frame_count, 5);
        assert!((result.stacked_image[10] - signal).abs() < 1e-6);
        assert!((result.snr_improvement_factor - (5.0f64).sqrt()).abs() < 1e-4);
    }

    #[test]
    fn stacks_frames_median_method() {
        let width = 2;
        let height = 2;

        let f1 = vec![10.0, 20.0, 30.0, 40.0];
        let f2 = vec![12.0, 22.0, 32.0, 42.0];
        let f3 = vec![1000.0, 21.0, 31.0, 41.0]; // Outlier at pixel 0

        let frames = vec![f1, f2, f3];
        let params = StackingParams {
            method: StackingMethod::Median,
            ..Default::default()
        };

        let result = stack_frames_2d(&frames, width, height, &params).unwrap();
        assert_eq!(result.stacked_image[0], 12.0); // Median of 10, 12, 1000 is 12
        assert_eq!(result.stacked_image[1], 21.0); // Median of 20, 21, 22 is 21
    }
}
