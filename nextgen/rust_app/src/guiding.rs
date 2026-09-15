#[derive(Clone, Debug, PartialEq)]
pub struct PidGains {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub min_pulse_ms: f64,
    pub max_pulse_ms: f64,
}

impl Default for PidGains {
    fn default() -> Self {
        Self {
            kp: 250.0,
            ki: 15.0,
            kd: 50.0,
            min_pulse_ms: 10.0,
            max_pulse_ms: 1500.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PidAxisState {
    pub integral_error: f64,
    pub last_error: f64,
    pub sample_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuiderCorrection {
    pub dx_pixels: f64,
    pub dy_pixels: f64,
    pub error_ra_arcsec: f64,
    pub error_dec_arcsec: f64,
    pub pulse_ra_ms: f64,
    pub pulse_dec_ms: f64,
    pub total_error_arcsec: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuiderSummary {
    pub total_cycles: usize,
    pub rms_ra_arcsec: f64,
    pub rms_dec_arcsec: f64,
    pub total_rms_arcsec: f64,
    pub tracking_stable: bool,
}

pub fn compute_subpixel_centroid_1d(fluxes: &[f64]) -> f64 {
    if fluxes.is_empty() {
        return 0.0;
    }
    let bg = fluxes.iter().copied().fold(f64::INFINITY, f64::min);
    let mut sum_w = 0.0;
    let mut sum_wx = 0.0;
    for (i, &f) in fluxes.iter().enumerate() {
        let w = (f - bg).max(0.0);
        sum_w += w;
        sum_wx += w * (i as f64);
    }
    if sum_w > 1e-12 {
        sum_wx / sum_w
    } else {
        (fluxes.len() as f64 - 1.0) / 2.0
    }
}

pub fn compute_pid_pulse(
    error: f64,
    state: &mut PidAxisState,
    gains: &PidGains,
    dt_s: f64,
) -> f64 {
    let dt = dt_s.max(0.01);
    state.integral_error += error * dt;
    // Anti-windup clamping on integral term
    state.integral_error = state.integral_error.clamp(-10.0, 10.0);

    let derivative = if state.sample_count > 0 {
        (error - state.last_error) / dt
    } else {
        0.0
    };

    state.last_error = error;
    state.sample_count += 1;

    let raw_output = gains.kp * error + gains.ki * state.integral_error + gains.kd * derivative;
    let sign = if raw_output >= 0.0 { 1.0 } else { -1.0 };
    let magnitude = raw_output.abs();

    if magnitude < gains.min_pulse_ms {
        0.0
    } else {
        sign * magnitude.min(gains.max_pulse_ms)
    }
}

pub fn compute_dither_offset(dither_index: usize, step_pixels: f64) -> (f64, f64) {
    // Spiral dithering pattern
    let angles = [0.0, 90.0, 180.0, 270.0, 45.0, 135.0, 225.0, 315.0];
    let ring = (dither_index / angles.len()) as f64 + 1.0;
    let angle_deg = angles[dither_index % angles.len()];
    let rad = angle_deg * std::f64::consts::PI / 180.0;
    (ring * step_pixels * rad.cos(), ring * step_pixels * rad.sin())
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuiderLoop {
    pub plate_scale_arcsec_per_pixel: f64,
    pub lock_x: f64,
    pub lock_y: f64,
    pub gains_ra: PidGains,
    pub gains_dec: PidGains,
    pub state_ra: PidAxisState,
    pub state_dec: PidAxisState,
    pub history_ra_sq: f64,
    pub history_dec_sq: f64,
    pub total_cycles: usize,
}

impl GuiderLoop {
    pub fn new(plate_scale: f64, lock_x: f64, lock_y: f64) -> Self {
        Self {
            plate_scale_arcsec_per_pixel: plate_scale,
            lock_x,
            lock_y,
            gains_ra: PidGains::default(),
            gains_dec: PidGains::default(),
            state_ra: PidAxisState::default(),
            state_dec: PidAxisState::default(),
            history_ra_sq: 0.0,
            history_dec_sq: 0.0,
            total_cycles: 0,
        }
    }

    pub fn process_guide_frame(
        &mut self,
        current_x: f64,
        current_y: f64,
        dt_s: f64,
    ) -> GuiderCorrection {
        let dx = current_x - self.lock_x;
        let dy = current_y - self.lock_y;

        let err_ra = dx * self.plate_scale_arcsec_per_pixel;
        let err_dec = dy * self.plate_scale_arcsec_per_pixel;

        let pulse_ra = compute_pid_pulse(err_ra, &mut self.state_ra, &self.gains_ra, dt_s);
        let pulse_dec = compute_pid_pulse(err_dec, &mut self.state_dec, &self.gains_dec, dt_s);

        self.history_ra_sq += err_ra * err_ra;
        self.history_dec_sq += err_dec * err_dec;
        self.total_cycles += 1;

        let total_err = (err_ra * err_ra + err_dec * err_dec).sqrt();

        GuiderCorrection {
            dx_pixels: dx,
            dy_pixels: dy,
            error_ra_arcsec: err_ra,
            error_dec_arcsec: err_dec,
            pulse_ra_ms: pulse_ra,
            pulse_dec_ms: pulse_dec,
            total_error_arcsec: total_err,
        }
    }

    pub fn get_summary(&self) -> GuiderSummary {
        if self.total_cycles == 0 {
            return GuiderSummary {
                total_cycles: 0,
                rms_ra_arcsec: 0.0,
                rms_dec_arcsec: 0.0,
                total_rms_arcsec: 0.0,
                tracking_stable: false,
            };
        }

        let rms_ra = (self.history_ra_sq / self.total_cycles as f64).sqrt();
        let rms_dec = (self.history_dec_sq / self.total_cycles as f64).sqrt();
        let total_rms = (rms_ra * rms_ra + rms_dec * rms_dec).sqrt();

        GuiderSummary {
            total_cycles: self.total_cycles,
            rms_ra_arcsec: rms_ra,
            rms_dec_arcsec: rms_dec,
            total_rms_arcsec: total_rms,
            tracking_stable: total_rms < 1.0, // Sub-arcsecond tracking requirement
        }
    }
}

pub fn guider_summary_to_json(summary: &GuiderSummary) -> String {
    format!(
        "{{\n  \"total_cycles\": {},\n  \"rms_ra_arcsec\": {:.4},\n  \"rms_dec_arcsec\": {:.4},\n  \"total_rms_arcsec\": {:.4},\n  \"tracking_stable\": {}\n}}\n",
        summary.total_cycles,
        summary.rms_ra_arcsec,
        summary.rms_dec_arcsec,
        summary.total_rms_arcsec,
        summary.tracking_stable
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_subpixel_centroid_accurately() {
        let fluxes = vec![10.0, 50.0, 200.0, 500.0, 200.0, 50.0, 10.0];
        let center = compute_subpixel_centroid_1d(&fluxes);
        assert!((center - 3.0).abs() < 1e-4);
    }

    #[test]
    fn guider_loop_drives_tracking_error_and_computes_rms() {
        let mut guider = GuiderLoop::new(1.0, 256.0, 256.0); // 1.0 arcsec/pixel

        // Step 1: Guide star drifting at (256.5, 256.3)
        let c1 = guider.process_guide_frame(256.5, 256.3, 1.0);
        assert!((c1.error_ra_arcsec - 0.5).abs() < 1e-6);
        assert!((c1.error_dec_arcsec - 0.3).abs() < 1e-6);
        assert!(c1.pulse_ra_ms > 0.0);
        assert!(c1.pulse_dec_ms > 0.0);

        // Step 2: Correction brings star closer to lock
        let c2 = guider.process_guide_frame(256.1, 256.05, 1.0);
        assert!(c2.total_error_arcsec < c1.total_error_arcsec);

        let summary = guider.get_summary();
        assert_eq!(summary.total_cycles, 2);
        assert!(summary.total_rms_arcsec < 1.0);
        assert!(summary.tracking_stable);
    }

    #[test]
    fn generates_spiral_dither_offsets() {
        let (dx0, dy0) = compute_dither_offset(0, 5.0); // 0 deg -> (+5.0, 0.0)
        assert!((dx0 - 5.0).abs() < 1e-4);
        assert!(dy0.abs() < 1e-4);

        let (dx1, dy1) = compute_dither_offset(1, 5.0); // 90 deg -> (0.0, +5.0)
        assert!(dx1.abs() < 1e-4);
        assert!((dy1 - 5.0).abs() < 1e-4);
    }
}
