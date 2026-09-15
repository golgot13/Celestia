#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeconvolutionMethod {
    RichardsonLucy,
    VanCittert,
    WienerFilter,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeconvolutionParams {
    pub method: DeconvolutionMethod,
    pub iterations: usize,
    pub regularization_factor: f64,
    pub positivity_constraint: bool,
}

impl Default for DeconvolutionParams {
    fn default() -> Self {
        Self {
            method: DeconvolutionMethod::RichardsonLucy,
            iterations: 15,
            regularization_factor: 0.001,
            positivity_constraint: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeconvolutionResult {
    pub width: usize,
    pub height: usize,
    pub iterations_completed: usize,
    pub restored_image: Vec<f64>,
    pub flux_conservation_ratio: f64,
    pub final_residual_rms: f64,
    pub contrast_improvement_ratio: f64,
}

pub fn convolve_2d_separable(
    image: &[f64],
    width: usize,
    height: usize,
    kernel: &[f64], // 1D symmetric kernel (odd length)
) -> Vec<f64> {
    if image.len() != width * height || kernel.is_empty() {
        return image.to_vec();
    }

    let k_radius = kernel.len() / 2;
    let mut temp = vec![0.0; width * height];
    let mut output = vec![0.0; width * height];

    // Horizontal 1D pass
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (k_idx, &k_val) in kernel.iter().enumerate() {
                let offset = k_idx as isize - k_radius as isize;
                let src_x = (x as isize + offset).clamp(0, width as isize - 1) as usize;
                sum += image[y * width + src_x] * k_val;
            }
            temp[y * width + x] = sum;
        }
    }

    // Vertical 1D pass
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (k_idx, &k_val) in kernel.iter().enumerate() {
                let offset = k_idx as isize - k_radius as isize;
                let src_y = (y as isize + offset).clamp(0, height as isize - 1) as usize;
                sum += temp[src_y * width + x] * k_val;
            }
            output[y * width + x] = sum;
        }
    }

    output
}

pub fn generate_gaussian_kernel_1d(radius: usize, sigma: f64) -> Vec<f64> {
    let size = 2 * radius + 1;
    let mut kernel = Vec::with_capacity(size);
    let s2 = (2.0 * sigma * sigma).max(1e-6);

    let mut sum = 0.0;
    for i in 0..size {
        let x = i as f64 - radius as f64;
        let val = (-x * x / s2).exp();
        kernel.push(val);
        sum += val;
    }

    // Normalize sum to 1.0 (flux conservation)
    for val in &mut kernel {
        *val /= sum;
    }

    kernel
}

pub fn richardson_lucy_deconvolve_2d(
    blurred: &[f64],
    width: usize,
    height: usize,
    psf_kernel_1d: &[f64],
    params: &DeconvolutionParams,
) -> Result<DeconvolutionResult, String> {
    if blurred.len() != width * height {
        return Err("blurred image dimensions mismatch".to_string());
    }
    if psf_kernel_1d.is_empty() {
        return Err("PSF kernel cannot be empty".to_string());
    }

    let initial_total_flux: f64 = blurred.iter().sum();
    if initial_total_flux <= 1e-12 {
        return Err("image total flux is zero".to_string());
    }

    // Initialize estimate with positive blurred image
    let mut estimate = blurred.to_vec();
    if params.positivity_constraint {
        for val in &mut estimate {
            *val = val.max(1e-6);
        }
    }

    let total_pixels = width * height;
    let mut final_residual = 0.0;

    for _iter in 0..params.iterations {
        // Step 1: Forward projection (re-blur current estimate: C_k = I_k * PSF)
        let reblurred = convolve_2d_separable(&estimate, width, height, psf_kernel_1d);

        // Step 2: Error ratio image (R_k = D / C_k)
        let mut ratio = vec![1.0; total_pixels];
        for i in 0..total_pixels {
            let reblur_val = reblurred[i].max(1e-6);
            ratio[i] = (blurred[i] / reblur_val).clamp(0.01, 100.0);
        }

        // Step 3: Back-projection (Correlation with adjoint PSF: G_k = R_k * PSF^T)
        // For symmetric 1D Gaussian PSF, PSF^T is identical to PSF
        let correction = convolve_2d_separable(&ratio, width, height, psf_kernel_1d);

        // Step 4: Multiplicative update (I_{k+1} = I_k * G_k) with Total Variation damping
        for i in 0..total_pixels {
            let mut new_val = estimate[i] * correction[i];
            if params.regularization_factor > 0.0 {
                new_val /= (1.0 + params.regularization_factor * (correction[i] - 1.0).abs());
            }
            if params.positivity_constraint {
                new_val = new_val.max(1e-6);
            }
            estimate[i] = new_val;
        }

        // Calculate residual RMS between blurred input and reblurred estimate
        let mut sq_err = 0.0;
        for i in 0..total_pixels {
            sq_err += (blurred[i] - reblurred[i]).powi(2);
        }
        final_residual = (sq_err / total_pixels as f64).sqrt();
    }

    let final_total_flux: f64 = estimate.iter().sum();
    let flux_ratio = final_total_flux / initial_total_flux;

    // Contrast improvement = max_flux / median_flux ratio enhancement
    let initial_max = blurred.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let final_max = estimate.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let contrast_improvement = (final_max / initial_max.max(1.0)).max(1.0);

    Ok(DeconvolutionResult {
        width,
        height,
        iterations_completed: params.iterations,
        restored_image: estimate,
        flux_conservation_ratio: flux_ratio,
        final_residual_rms: final_residual,
        contrast_improvement_ratio: contrast_improvement,
    })
}

pub fn deconvolution_result_to_json(result: &DeconvolutionResult) -> String {
    format!(
        "{{\n  \"width\": {},\n  \"height\": {},\n  \"iterations_completed\": {},\n  \"flux_conservation_ratio\": {:.6},\n  \"final_residual_rms\": {:.6},\n  \"contrast_improvement_ratio\": {:.4}\n}}\n",
        result.width,
        result.height,
        result.iterations_completed,
        result.flux_conservation_ratio,
        result.final_residual_rms,
        result.contrast_improvement_ratio
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_kernel_is_symmetric_and_conserves_flux() {
        let kernel = generate_gaussian_kernel_1d(3, 1.2);
        assert_eq!(kernel.len(), 7);
        let sum: f64 = kernel.iter().sum();
        assert!((sum - 1.0).abs() < 1e-12);
        assert!((kernel[0] - kernel[6]).abs() < 1e-12);
        assert!((kernel[1] - kernel[5]).abs() < 1e-12);
    }

    #[test]
    fn richardson_lucy_restores_sharp_point_source_and_conserves_flux() {
        let width = 16;
        let height = 16;
        let mut sharp = vec![5.0; 256];
        // Point source at center (8,8) -> index 8*16 + 8 = 136
        sharp[136] = 500.0;

        let psf = generate_gaussian_kernel_1d(3, 1.0);
        let blurred = convolve_2d_separable(&sharp, width, height, &psf);

        // Peak flux in blurred image should be dispersed
        assert!(blurred[136] < sharp[136] * 0.5);

        let params = DeconvolutionParams {
            method: DeconvolutionMethod::RichardsonLucy,
            iterations: 12,
            regularization_factor: 0.0,
            positivity_constraint: true,
        };

        let result = richardson_lucy_deconvolve_2d(&blurred, width, height, &psf, &params).unwrap();

        assert_eq!(result.iterations_completed, 12);
        // Flux must be conserved to within 2%
        assert!((result.flux_conservation_ratio - 1.0).abs() < 0.02);
        // Restored image peak should be significantly higher than blurred peak
        assert!(result.restored_image[136] > blurred[136] * 1.5);
        assert!(result.contrast_improvement_ratio > 1.2);
    }
}
