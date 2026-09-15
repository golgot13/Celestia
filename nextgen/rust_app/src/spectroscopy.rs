use crate::SPEED_OF_LIGHT;

#[derive(Clone, Debug, PartialEq)]
pub struct SpectralLine {
    pub name: &'static str,
    pub rest_wavelength_angstrom: f64,
}

pub const H_ALPHA: SpectralLine = SpectralLine {
    name: "H-Alpha",
    rest_wavelength_angstrom: 6562.81,
};
pub const H_BETA: SpectralLine = SpectralLine {
    name: "H-Beta",
    rest_wavelength_angstrom: 4861.33,
};
pub const NA_D1: SpectralLine = SpectralLine {
    name: "Na-D1",
    rest_wavelength_angstrom: 5895.92,
};
pub const NA_D2: SpectralLine = SpectralLine {
    name: "Na-D2",
    rest_wavelength_angstrom: 5889.95,
};

#[derive(Clone, Debug, PartialEq)]
pub struct LampEmissionLine {
    pub pixel_position: f64,
    pub known_wavelength_angstrom: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DispersionSolution {
    pub polynomial_coeffs: Vec<f64>, // lambda(x) = c0 + c1*x + c2*x^2 + c3*x^3
    pub rms_residual_angstrom: f64,
    pub order: usize,
    pub valid: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedSpectrum {
    pub pixel_count: usize,
    pub wavelengths_angstrom: Vec<f64>,
    pub fluxes: Vec<f64>,
    pub continuum_normalized_fluxes: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadialVelocityMeasurement {
    pub line_name: &'static str,
    pub rest_wavelength_angstrom: f64,
    pub observed_wavelength_angstrom: f64,
    pub doppler_shift_angstrom: f64,
    pub radial_velocity_km_s: f64,
    pub redshift_z: f64,
}

pub fn solve_dispersion_polynomial(
    calibration_lines: &[LampEmissionLine],
    order: usize,
) -> Result<DispersionSolution, String> {
    let n = calibration_lines.len();
    if n <= order {
        return Err(format!(
            "insufficient lines ({n}) to fit polynomial of order {order}"
        ));
    }
    if order > 3 {
        return Err("polynomial order higher than 3 not supported".to_string());
    }

    // Linear regression for Order 1: lambda = c0 + c1 * x
    if order == 1 {
        let sum_x: f64 = calibration_lines.iter().map(|l| l.pixel_position).sum();
        let sum_y: f64 = calibration_lines.iter().map(|l| l.known_wavelength_angstrom).sum();
        let sum_xx: f64 = calibration_lines.iter().map(|l| l.pixel_position.powi(2)).sum();
        let sum_xy: f64 = calibration_lines.iter().map(|l| l.pixel_position * l.known_wavelength_angstrom).sum();

        let count = n as f64;
        let denom = count * sum_xx - sum_x * sum_x;
        if denom.abs() < 1e-12 {
            return Err("singular matrix in dispersion fit".to_string());
        }

        let c1 = (count * sum_xy - sum_x * sum_y) / denom;
        let c0 = (sum_y - c1 * sum_x) / count;

        let mut sq_err = 0.0;
        for line in calibration_lines {
            let pred = c0 + c1 * line.pixel_position;
            sq_err += (line.known_wavelength_angstrom - pred).powi(2);
        }
        let rms = (sq_err / count).sqrt();

        return Ok(DispersionSolution {
            polynomial_coeffs: vec![c0, c1],
            rms_residual_angstrom: rms,
            order: 1,
            valid: rms < 0.5,
        });
    }

    // Order 2 polynomial fit using normal equations: lambda = c0 + c1 * x + c2 * x^2
    let mut sum_x = 0.0;
    let mut sum_x2 = 0.0;
    let mut sum_x3 = 0.0;
    let mut sum_x4 = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_x2y = 0.0;

    for line in calibration_lines {
        let x = line.pixel_position;
        let y = line.known_wavelength_angstrom;
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

    // 3x3 linear system solver (Cramer's rule)
    // [ count  sum_x   sum_x2  ] [ c0 ]   [ sum_y   ]
    // [ sum_x  sum_x2  sum_x3  ] [ c1 ] = [ sum_xy  ]
    // [ sum_x2 sum_x3  sum_x4  ] [ c2 ]   [ sum_x2y ]

    let det = count * (sum_x2 * sum_x4 - sum_x3 * sum_x3)
        - sum_x * (sum_x * sum_x4 - sum_x2 * sum_x3)
        + sum_x2 * (sum_x * sum_x3 - sum_x2 * sum_x2);

    if det.abs() < 1e-12 {
        return Err("singular determinant in quadratic dispersion fit".to_string());
    }

    let det0 = sum_y * (sum_x2 * sum_x4 - sum_x3 * sum_x3)
        - sum_x * (sum_xy * sum_x4 - sum_x2y * sum_x3)
        + sum_x2 * (sum_xy * sum_x3 - sum_x2y * sum_x2);

    let det1 = count * (sum_xy * sum_x4 - sum_x2y * sum_x3)
        - sum_y * (sum_x * sum_x4 - sum_x2 * sum_x3)
        + sum_x2 * (sum_x * sum_x2y - sum_x2 * sum_xy);

    let det2 = count * (sum_x2 * sum_x2y - sum_x3 * sum_xy)
        - sum_x * (sum_x * sum_x2y - sum_x2 * sum_xy)
        + sum_y * (sum_x * sum_x3 - sum_x2 * sum_x2);

    let c0 = det0 / det;
    let c1 = det1 / det;
    let c2 = det2 / det;

    let mut sq_err = 0.0;
    for line in calibration_lines {
        let x = line.pixel_position;
        let pred = c0 + c1 * x + c2 * x * x;
        sq_err += (line.known_wavelength_angstrom - pred).powi(2);
    }
    let rms = (sq_err / count).sqrt();

    Ok(DispersionSolution {
        polynomial_coeffs: vec![c0, c1, c2],
        rms_residual_angstrom: rms,
        order: 2,
        valid: rms < 0.2,
    })
}

pub fn evaluate_wavelength_at_pixel(dispersion: &DispersionSolution, pixel: f64) -> f64 {
    let mut wavelength = 0.0;
    let mut x_pow = 1.0;
    for &coeff in &dispersion.polynomial_coeffs {
        wavelength += coeff * x_pow;
        x_pow *= pixel;
    }
    wavelength
}

pub fn extract_1d_spectrum_from_2d(
    image_2d: &[f64],
    width: usize,
    height: usize,
    trace_center_y: usize,
    aperture_half_width: usize,
    dispersion: &DispersionSolution,
) -> Result<ExtractedSpectrum, String> {
    if image_2d.len() != width * height {
        return Err("2D spectral image dimensions mismatch".to_string());
    }

    let y_min = trace_center_y.saturating_sub(aperture_half_width);
    let y_max = (trace_center_y + aperture_half_width).min(height - 1);

    let mut fluxes = Vec::with_capacity(width);
    let mut wavelengths = Vec::with_capacity(width);

    for x in 0..width {
        let mut column_flux = 0.0;
        for y in y_min..=y_max {
            column_flux += image_2d[y * width + x];
        }
        fluxes.push(column_flux);
        wavelengths.push(evaluate_wavelength_at_pixel(dispersion, x as f64));
    }

    // Continuum estimation with low-pass moving window
    let window = 15.min(width / 2);
    let mut continuum = Vec::with_capacity(width);
    for i in 0..width {
        let start = i.saturating_sub(window);
        let end = (i + window + 1).min(width);
        let mut local: Vec<f64> = fluxes[start..end].to_vec();
        local.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Take 80th percentile as local continuum
        let p80_idx = ((local.len() as f64) * 0.8).min(local.len() as f64 - 1.0) as usize;
        continuum.push(local[p80_idx].max(1.0));
    }

    let normalized: Vec<f64> = fluxes
        .iter()
        .zip(&continuum)
        .map(|(&f, &c)| f / c)
        .collect();

    Ok(ExtractedSpectrum {
        pixel_count: width,
        wavelengths_angstrom: wavelengths,
        fluxes,
        continuum_normalized_fluxes: normalized,
    })
}

pub fn compute_doppler_radial_velocity(
    line: &SpectralLine,
    observed_wavelength_angstrom: f64,
) -> RadialVelocityMeasurement {
    let delta_lambda = observed_wavelength_angstrom - line.rest_wavelength_angstrom;
    let z = delta_lambda / line.rest_wavelength_angstrom;
    // Radial velocity: v = c * z (in km/s, positive = receding/redshift, negative = approaching/blueshift)
    let v_km_s = (SPEED_OF_LIGHT / 1000.0) * z;

    RadialVelocityMeasurement {
        line_name: line.name,
        rest_wavelength_angstrom: line.rest_wavelength_angstrom,
        observed_wavelength_angstrom,
        doppler_shift_angstrom: delta_lambda,
        radial_velocity_km_s: v_km_s,
        redshift_z: z,
    }
}

pub fn radial_velocity_to_json(rv: &RadialVelocityMeasurement) -> String {
    format!(
        "{{\n  \"line_name\": \"{}\",\n  \"rest_wavelength_angstrom\": {:.4},\n  \"observed_wavelength_angstrom\": {:.4},\n  \"doppler_shift_angstrom\": {:.4},\n  \"radial_velocity_km_s\": {:.2},\n  \"redshift_z\": {:.6}\n}}\n",
        rv.line_name,
        rv.rest_wavelength_angstrom,
        rv.observed_wavelength_angstrom,
        rv.doppler_shift_angstrom,
        rv.radial_velocity_km_s,
        rv.redshift_z
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_quadratic_dispersion_solution() {
        let lines = vec![
            LampEmissionLine { pixel_position: 100.0, known_wavelength_angstrom: 4000.0 },
            LampEmissionLine { pixel_position: 300.0, known_wavelength_angstrom: 5000.0 },
            LampEmissionLine { pixel_position: 500.0, known_wavelength_angstrom: 6000.0 },
            LampEmissionLine { pixel_position: 700.0, known_wavelength_angstrom: 7000.0 },
        ];

        let sol = solve_dispersion_polynomial(&lines, 2).unwrap();
        assert!(sol.valid);
        assert!(sol.rms_residual_angstrom < 1e-4);

        let mid_lambda = evaluate_wavelength_at_pixel(&sol, 400.0);
        assert!((mid_lambda - 5500.0).abs() < 0.1);
    }

    #[test]
    fn computes_doppler_radial_velocity_shift() {
        // Star with radial velocity of ~ -300 km/s (Andromeda Galaxy M31 blueshift)
        // Delta lambda = lambda_0 * (-300 / 299792.458)
        let rest = H_ALPHA.rest_wavelength_angstrom;
        let expected_shift = rest * (-300.0 / 299792.458);
        let observed = rest + expected_shift;

        let rv = compute_doppler_radial_velocity(&H_ALPHA, observed);
        assert!((rv.radial_velocity_km_s - (-300.0)).abs() < 0.05);
        assert!(rv.doppler_shift_angstrom < 0.0);
    }
}
