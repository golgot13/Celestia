use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensZone {
    Center,
    InnerRing,
    MidRing,
    OuterRing,
    Edge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WavefrontAberrations {
    pub piston_z0: f64,          // Z_0^0
    pub tilt_x_z1: f64,           // Z_1^1 (x-tilt)
    pub tilt_y_z2: f64,           // Z_1^-1 (y-tilt)
    pub defocus_z3: f64,          // Z_2^0 (defocus)
    pub astigmatism_primary_z4: f64, // Z_2^2 (0/90 deg astigmatism)
    pub astigmatism_oblique_z5: f64, // Z_2^-2 (45 deg astigmatism)
    pub coma_horizontal_z6: f64,  // Z_3^1 (horizontal coma)
    pub coma_vertical_z7: f64,    // Z_3^-1 (vertical coma)
    pub spherical_primary_z8: f64,// Z_4^0 (primary spherical aberration)
    pub trefoil_z9: f64,          // Z_3^3 (trefoil)
}

impl Default for WavefrontAberrations {
    fn default() -> Self {
        Self {
            piston_z0: 0.0,
            tilt_x_z1: 0.0,
            tilt_y_z2: 0.0,
            defocus_z3: 0.0,
            astigmatism_primary_z4: 0.0,
            astigmatism_oblique_z5: 0.0,
            coma_horizontal_z6: 0.0,
            coma_vertical_z7: 0.0,
            spherical_primary_z8: 0.0,
            trefoil_z9: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpticalWavefrontSummary {
    pub rms_wavefront_error_waves: f64,
    pub peak_to_valley_waves: f64,
    pub strehl_ratio: f64,
    pub maréchal_diffraction_limited: bool,
    pub dominant_aberration: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpotDiagramSample {
    pub ray_index: usize,
    pub pupil_u: f64, // Normalized pupil coordinate [-1.0, 1.0]
    pub pupil_v: f64,
    pub focal_x_um: f64, // Ray intercept at focal plane in microns
    pub focal_y_um: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpotDiagramMetrics {
    pub ray_count: usize,
    pub rms_spot_radius_um: f64,
    pub geometric_spot_radius_um: f64,
    pub airy_disk_radius_um: f64,
    pub is_diffraction_limited: bool,
}

pub fn evaluate_zernike_wavefront_opd(
    aberrations: &WavefrontAberrations,
    rho: f64, // Normalized pupil radius [0.0, 1.0]
    theta_rad: f64,
) -> f64 {
    let r = rho.clamp(0.0, 1.0);
    let r2 = r * r;
    let r3 = r2 * r;
    let r4 = r2 * r2;

    // Noll / ANSI Standard Zernike Circular Polynomials:
    // Z0 = 1
    // Z1 = 2 * r * cos(theta)
    // Z2 = 2 * r * sin(theta)
    // Z3 = sqrt(3) * (2*r^2 - 1)
    // Z4 = sqrt(6) * r^2 * cos(2*theta)
    // Z5 = sqrt(6) * r^2 * sin(2*theta)
    // Z6 = sqrt(8) * (3*r^3 - 2*r) * cos(theta)
    // Z7 = sqrt(8) * (3*r^3 - 2*r) * sin(theta)
    // Z8 = sqrt(5) * (6*r^4 - 6*r^2 + 1)
    // Z9 = sqrt(8) * r^3 * cos(3*theta)

    let z0 = 1.0;
    let z1 = 2.0 * r * theta_rad.cos();
    let z2 = 2.0 * r * theta_rad.sin();
    let z3 = 3.0_f64.sqrt() * (2.0 * r2 - 1.0);
    let z4 = 6.0_f64.sqrt() * r2 * (2.0 * theta_rad).cos();
    let z5 = 6.0_f64.sqrt() * r2 * (2.0 * theta_rad).sin();
    let z6 = 8.0_f64.sqrt() * (3.0 * r3 - 2.0 * r) * theta_rad.cos();
    let z7 = 8.0_f64.sqrt() * (3.0 * r3 - 2.0 * r) * theta_rad.sin();
    let z8 = 5.0_f64.sqrt() * (6.0 * r4 - 6.0 * r2 + 1.0);
    let z9 = 8.0_f64.sqrt() * r3 * (3.0 * theta_rad).cos();

    aberrations.piston_z0 * z0
        + aberrations.tilt_x_z1 * z1
        + aberrations.tilt_y_z2 * z2
        + aberrations.defocus_z3 * z3
        + aberrations.astigmatism_primary_z4 * z4
        + aberrations.astigmatism_oblique_z5 * z5
        + aberrations.coma_horizontal_z6 * z6
        + aberrations.coma_vertical_z7 * z7
        + aberrations.spherical_primary_z8 * z8
        + aberrations.trefoil_z9 * z9
}

pub fn analyze_optical_wavefront(
    aberrations: &WavefrontAberrations,
    grid_resolution: usize,
) -> OpticalWavefrontSummary {
    let n = grid_resolution.max(16);
    let mut opd_values = Vec::new();

    for i in 0..n {
        for j in 0..n {
            let u = (2.0 * (i as f64) / (n - 1) as f64) - 1.0;
            let v = (2.0 * (j as f64) / (n - 1) as f64) - 1.0;
            let rho = (u * u + v * v).sqrt();
            if rho <= 1.0 {
                let theta = v.atan2(u);
                let opd = evaluate_zernike_wavefront_opd(aberrations, rho, theta);
                opd_values.push(opd);
            }
        }
    }

    if opd_values.is_empty() {
        return OpticalWavefrontSummary {
            rms_wavefront_error_waves: 0.0,
            peak_to_valley_waves: 0.0,
            strehl_ratio: 1.0,
            maréchal_diffraction_limited: true,
            dominant_aberration: "None",
        };
    }

    let count = opd_values.len() as f64;
    let mean_opd = opd_values.iter().sum::<f64>() / count;
    let var = opd_values.iter().map(|&w| (w - mean_opd).powi(2)).sum::<f64>() / count;
    let rms_waves = var.sqrt();

    let min_opd = opd_values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_opd = opd_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let pt_valley = max_opd - min_opd;

    // Maréchal approximation for Strehl ratio: S ≈ exp(-(2*pi*sigma_w)^2)
    let phi_rms = 2.0 * PI * rms_waves;
    let strehl = (-phi_rms * phi_rms).exp().clamp(0.0, 1.0);
    let maréchal_diffraction_limited = rms_waves <= 0.0714; // lambda / 14 Maréchal criterion (Strehl >= 0.80)

    // Identify dominant aberration term
    let terms = [
        ("Defocus", aberrations.defocus_z3.abs()),
        ("Astigmatism", (aberrations.astigmatism_primary_z4.powi(2) + aberrations.astigmatism_oblique_z5.powi(2)).sqrt()),
        ("Coma", (aberrations.coma_horizontal_z6.powi(2) + aberrations.coma_vertical_z7.powi(2)).sqrt()),
        ("Spherical", aberrations.spherical_primary_z8.abs()),
        ("Trefoil", aberrations.trefoil_z9.abs()),
    ];

    let dominant = terms
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|t| t.0)
        .unwrap_or("None");

    OpticalWavefrontSummary {
        rms_wavefront_error_waves: rms_waves,
        peak_to_valley_waves: pt_valley,
        strehl_ratio: strehl,
        maréchal_diffraction_limited,
        dominant_aberration: if rms_waves < 0.01 { "None (Ideal)" } else { dominant },
    }
}

pub fn trace_pupil_spot_diagram(
    aberrations: &WavefrontAberrations,
    focal_ratio: f64,
    focal_length_mm: f64,
    wavelength_nm: f64,
    num_rings: usize,
) -> (Vec<SpotDiagramSample>, SpotDiagramMetrics) {
    let mut spots = Vec::new();
    let mut ray_idx = 0;

    let f_num = focal_ratio.max(1.0);
    // Ray transverse ray aberration (TRA) proportional to gradient of OPD: delta_x = -f_num * dW/du
    let lambda_um = wavelength_nm * 1e-3;
    let eps = 1e-4;

    for ring in 0..=num_rings {
        let rho = (ring as f64) / (num_rings as f64);
        let num_points = if ring == 0 { 1 } else { ring * 6 };

        for p in 0..num_points {
            let theta = (p as f64) * (2.0 * PI / num_points as f64);
            let u = rho * theta.cos();
            let v = rho * theta.sin();

            // Numerical derivatives of OPD at pupil position (u, v)
            let w_center = evaluate_zernike_wavefront_opd(aberrations, rho, theta);
            let u_plus = (u + eps).clamp(-1.0, 1.0);
            let v_plus = (v + eps).clamp(-1.0, 1.0);

            let rho_u = (u_plus * u_plus + v * v).sqrt();
            let theta_u = v.atan2(u_plus);
            let w_u = evaluate_zernike_wavefront_opd(aberrations, rho_u, theta_u);

            let rho_v = (u * u + v_plus * v_plus).sqrt();
            let theta_v = v_plus.atan2(u);
            let w_v = evaluate_zernike_wavefront_opd(aberrations, rho_v, theta_v);

            let dw_du = (w_u - w_center) / eps;
            let dw_dv = (w_v - w_center) / eps;

            // Transverse aberration at focal plane in microns: x = -R * f_num * dW/du
            let focal_x_um = -2.0 * f_num * dw_du * lambda_um;
            let focal_y_um = -2.0 * f_num * dw_dv * lambda_um;

            spots.push(SpotDiagramSample {
                ray_index: ray_idx,
                pupil_u: u,
                pupil_v: v,
                focal_x_um,
                focal_y_um,
            });
            ray_idx += 1;
        }
    }

    // Compute Spot Metrics
    let count = spots.len() as f64;
    let mean_x = spots.iter().map(|s| s.focal_x_um).sum::<f64>() / count;
    let mean_y = spots.iter().map(|s| s.focal_y_um).sum::<f64>() / count;

    let var_r = spots
        .iter()
        .map(|s| (s.focal_x_um - mean_x).powi(2) + (s.focal_y_um - mean_y).powi(2))
        .sum::<f64>()
        / count;
    let rms_radius_um = var_r.sqrt();

    let max_radius_um = spots
        .iter()
        .map(|s| ((s.focal_x_um - mean_x).powi(2) + (s.focal_y_um - mean_y).powi(2)).sqrt())
        .fold(0.0, f64::max);

    // Standard Airy Disk radius: r_airy = 1.22 * lambda * f_number (in microns)
    let airy_disk_radius_um = 1.22 * lambda_um * f_num;
    let is_diffraction_limited = rms_radius_um <= airy_disk_radius_um;

    let _ = focal_length_mm; // retained for optical scale context
    let ray_count = spots.len();

    (
        spots,
        SpotDiagramMetrics {
            ray_count,
            rms_spot_radius_um: rms_radius_um,
            geometric_spot_radius_um: max_radius_um,
            airy_disk_radius_um,
            is_diffraction_limited,
        },
    )
}

pub fn optical_wavefront_summary_to_json(
    wavefront: &OpticalWavefrontSummary,
    spot: &SpotDiagramMetrics,
) -> String {
    format!(
        "{{\n  \"rms_wavefront_error_waves\": {:.4},\n  \"peak_to_valley_waves\": {:.4},\n  \"strehl_ratio\": {:.4},\n  \"maréchal_diffraction_limited\": {},\n  \"dominant_aberration\": \"{}\",\n  \"rms_spot_radius_um\": {:.3},\n  \"airy_disk_radius_um\": {:.3},\n  \"is_diffraction_limited\": {}\n}}\n",
        wavefront.rms_wavefront_error_waves,
        wavefront.peak_to_valley_waves,
        wavefront.strehl_ratio,
        wavefront.maréchal_diffraction_limited,
        wavefront.dominant_aberration,
        spot.rms_spot_radius_um,
        spot.airy_disk_radius_um,
        spot.is_diffraction_limited
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_wavefront_achieves_unity_strehl() {
        let perfect = WavefrontAberrations::default();
        let summary = analyze_optical_wavefront(&perfect, 20);

        assert_eq!(summary.rms_wavefront_error_waves, 0.0);
        assert!((summary.strehl_ratio - 1.0).abs() < 1e-6);
        assert!(summary.maréchal_diffraction_limited);
        assert_eq!(summary.dominant_aberration, "None (Ideal)");
    }

    #[test]
    fn ray_tracing_identifies_diffraction_limited_system() {
        let aberrations = WavefrontAberrations {
            spherical_primary_z8: 0.02, // 1/50 wave spherical aberration (very small)
            ..Default::default()
        };

        let (_, metrics) = trace_pupil_spot_diagram(&aberrations, 5.0, 1000.0, 550.0, 8);
        assert!(metrics.is_diffraction_limited);
        assert!(metrics.rms_spot_radius_um < metrics.airy_disk_radius_um);
        assert!((metrics.airy_disk_radius_um - 3.355).abs() < 0.05); // 1.22 * 0.55 * 5 = ~3.355 um
    }
}
