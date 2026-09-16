use crate::{
    AperturePhotometry, CpuFeatureFlags, ObservationControllerResult,
    PixelCoord, WcsTransform, WorldCoord, ASTRO_ABI_VERSION_MAJOR, ASTRO_ABI_VERSION_MINOR,
};

#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticMetrics {
    pub cpu_avx2_supported: bool,
    pub abi_version_match: bool,
    pub astrometry_rms_residual_deg: f64,
    pub calibration_mean_snr: f64,
    pub total_targets: usize,
    pub valid_targets: usize,
    pub system_healthy: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SystemDiagnosticReport {
    pub metrics: DiagnosticMetrics,
    pub diagnostic_log: Vec<String>,
}

pub fn evaluate_astrometry_residuals(
    wcs: &WcsTransform,
    reference_points: &[(PixelCoord, WorldCoord)],
) -> f64 {
    if reference_points.is_empty() {
        return 0.0;
    }

    let mut sum_sq = 0.0;
    for (pixel, expected_world) in reference_points {
        let predicted_world = crate::pixel_to_world(*wcs, *pixel);
        let dra = predicted_world.ra_deg - expected_world.ra_deg;
        let ddec = predicted_world.dec_deg - expected_world.dec_deg;
        sum_sq += dra * dra + ddec * ddec;
    }

    (sum_sq / reference_points.len() as f64).sqrt()
}

pub fn evaluate_calibration_snr(
    photometry: &[AperturePhotometry],
) -> f64 {
    if photometry.is_empty() {
        return 0.0;
    }

    let total_snr: f64 = photometry.iter().map(|p| p.signal_to_noise).sum();
    total_snr / photometry.len() as f64
}

pub fn run_system_diagnostics(
    cpu_features: &CpuFeatureFlags,
    controller_result: &ObservationControllerResult,
    astrometry_rms_deg: f64,
    calibration_snr: f64,
) -> SystemDiagnosticReport {
    let mut logs = Vec::new();

    let cpu_avx2_supported = cpu_features.avx2;
    if cpu_avx2_supported {
        logs.push("CPU AVX2 SIMD acceleration available".to_string());
    } else {
        logs.push("CPU AVX2 SIMD acceleration missing - running in fallback mode".to_string());
    }

    let abi_version_match = ASTRO_ABI_VERSION_MAJOR == 1 && ASTRO_ABI_VERSION_MINOR == 0;
    if abi_version_match {
        logs.push(format!("ABI version {}.{} matched", ASTRO_ABI_VERSION_MAJOR, ASTRO_ABI_VERSION_MINOR));
    } else {
        logs.push(format!("ABI version mismatch: {}.{}", ASTRO_ABI_VERSION_MAJOR, ASTRO_ABI_VERSION_MINOR));
    }

    let astrometry_ok = astrometry_rms_deg < 0.05;
    if astrometry_ok {
        logs.push(format!("Astrometric solution verified with RMS {:.6} deg", astrometry_rms_deg));
    } else {
        logs.push(format!("Astrometric solution warning: RMS {:.6} deg exceeds threshold", astrometry_rms_deg));
    }

    let snr_ok = calibration_snr >= 5.0 || calibration_snr == 0.0;
    if snr_ok {
        logs.push(format!("Photometric calibration SNR nominal: {:.2}", calibration_snr));
    } else {
        logs.push(format!("Photometric calibration SNR low: {:.2}", calibration_snr));
    }

    let system_healthy = controller_result.ready
        && abi_version_match
        && astrometry_ok
        && snr_ok;

    if system_healthy {
        logs.push("All system diagnostic checks PASSED".to_string());
    } else {
        logs.push("System diagnostic checks reported WARNINGS or ERRORS".to_string());
    }

    SystemDiagnosticReport {
        metrics: DiagnosticMetrics {
            cpu_avx2_supported,
            abi_version_match,
            astrometry_rms_residual_deg: astrometry_rms_deg,
            calibration_mean_snr: calibration_snr,
            total_targets: if controller_result.campaign_ready { 1 } else { 0 },
            valid_targets: if controller_result.ready { 1 } else { 0 },
            system_healthy,
        },
        diagnostic_log: logs,
    }
}

pub fn diagnostic_report_to_json(report: &SystemDiagnosticReport) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"cpu_avx2_supported\": ");
    json.push_str(if report.metrics.cpu_avx2_supported { "true" } else { "false" });
    json.push_str(",\n");
    json.push_str("  \"abi_version_match\": ");
    json.push_str(if report.metrics.abi_version_match { "true" } else { "false" });
    json.push_str(",\n");
    json.push_str("  \"astrometry_rms_residual_deg\": ");
    json.push_str(&format!("{:.8}", report.metrics.astrometry_rms_residual_deg));
    json.push_str(",\n");
    json.push_str("  \"calibration_mean_snr\": ");
    json.push_str(&format!("{:.4}", report.metrics.calibration_mean_snr));
    json.push_str(",\n");
    json.push_str("  \"system_healthy\": ");
    json.push_str(if report.metrics.system_healthy { "true" } else { "false" });
    json.push_str(",\n");
    json.push_str("  \"logs\": [\n");

    for (index, log) in report.diagnostic_log.iter().enumerate() {
        json.push_str("    \"");
        json.push_str(log);
        json.push('"');
        if index + 1 < report.diagnostic_log.len() {
            json.push(',');
        }
        json.push('\n');
    }

    json.push_str("  ]\n");
    json.push_str("}\n");
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{solve_wcs_from_reference_points, CpuFeatureFlags, ObservationControllerResult, PixelCoord, WorldCoord};

    #[test]
    fn evaluates_astrometry_residuals_for_exact_solution() {
        let points = vec![
            (PixelCoord { x: 100.0, y: 100.0 }, WorldCoord { ra_deg: 10.0, dec_deg: 20.0 }),
            (PixelCoord { x: 200.0, y: 100.0 }, WorldCoord { ra_deg: 10.01, dec_deg: 20.0 }),
            (PixelCoord { x: 100.0, y: 200.0 }, WorldCoord { ra_deg: 10.0, dec_deg: 20.01 }),
        ];

        let wcs = solve_wcs_from_reference_points(&points).expect("WCS solve failed");
        let rms = evaluate_astrometry_residuals(&wcs, &points);
        assert!(rms < 1e-6);
    }

    #[test]
    fn runs_system_diagnostics_and_produces_healthy_report() {
        let cpu = CpuFeatureFlags {
            x86_64: true,
            sse2: true,
            avx2: true,
        };

        let controller_result = ObservationControllerResult {
            campaign_ready: true,
            session_ready: true,
            acquisition_ready: true,
            astrometry_ready: true,
            report_ready: true,
            report_path: None,
            session_path: None,
            ready: true,
        };

        let report = run_system_diagnostics(&cpu, &controller_result, 0.0001, 15.4);
        assert!(report.metrics.system_healthy);
        assert!(report.metrics.cpu_avx2_supported);
        assert!(report.metrics.abi_version_match);
        assert!(report.diagnostic_log.len() >= 4);

        let json = diagnostic_report_to_json(&report);
        assert!(json.contains("\"system_healthy\": true"));
        assert!(json.contains("\"cpu_avx2_supported\": true"));
    }
}
