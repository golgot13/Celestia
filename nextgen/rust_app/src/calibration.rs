#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationFrame {
    pub bias: f64,
    pub dark_current: f64,
    pub flat_field: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReducedFrame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f64>,
    pub median_signal: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AperturePhotometry {
    pub aperture_flux: f64,
    pub background_level: f64,
    pub net_flux: f64,
    pub signal_to_noise: f64,
}

pub fn subtract_bias(frame: &[f64], bias: f64) -> Vec<f64> {
    frame.iter().map(|value| value - bias).collect()
}

pub fn apply_flat_field(frame: &[f64], flat_field: f64) -> Vec<f64> {
    if flat_field <= 0.0 {
        return frame.to_vec();
    }

    frame.iter().map(|value| value / flat_field).collect()
}

pub fn calibrate_frame(frame: &[f64], calibration: CalibrationFrame) -> ReducedFrame {
    calibrate_frame_2d(frame, 1, frame.len().max(1), calibration)
}

pub fn calibrate_frame_2d(
    frame: &[f64],
    width: usize,
    height: usize,
    calibration: CalibrationFrame,
) -> ReducedFrame {
    if frame.is_empty() || width == 0 || height == 0 || width * height != frame.len() {
        return ReducedFrame {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            median_signal: 0.0,
        };
    }

    let reduced = frame
        .iter()
        .map(|value| (value - calibration.bias - calibration.dark_current) / calibration.flat_field.max(1e-12))
        .collect::<Vec<_>>();

    let median_signal = if reduced.is_empty() {
        0.0
    } else {
        let mut v = reduced.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = v.len() / 2;
        if v.len() % 2 == 0 {
            (v[mid - 1] + v[mid]) / 2.0
        } else {
            v[mid]
        }
    };

    ReducedFrame {
        width,
        height,
        pixels: reduced,
        median_signal,
    }
}

pub fn aperture_photometry(
    frame: &[f64],
    width: usize,
    height: usize,
    center_x: f64,
    center_y: f64,
    aperture_radius: f64,
    annulus_inner: f64,
    annulus_outer: f64,
) -> AperturePhotometry {
    if frame.is_empty() || width == 0 || height == 0 || width * height != frame.len() {
        return AperturePhotometry {
            aperture_flux: 0.0,
            background_level: 0.0,
            net_flux: 0.0,
            signal_to_noise: 0.0,
        };
    }

    let mut aperture_flux = 0.0_f64;
    let mut aperture_pixels = 0usize;
    let mut annulus_sum = 0.0_f64;
    let mut annulus_pixels = 0usize;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - center_x;
            let dy = y as f64 - center_y;
            let distance = (dx * dx + dy * dy).sqrt();
            let value = frame[y * width + x];

            if distance <= aperture_radius {
                aperture_flux += value;
                aperture_pixels += 1;
            }

            if distance >= annulus_inner && distance <= annulus_outer {
                annulus_sum += value;
                annulus_pixels += 1;
            }
        }
    }

    let background_level = if annulus_pixels > 0 {
        annulus_sum / annulus_pixels as f64
    } else {
        0.0
    };

    let background_in_aperture = background_level * aperture_pixels as f64;
    let net_flux = aperture_flux - background_in_aperture;
    let variance = aperture_flux.max(1.0) + background_in_aperture.max(1.0);
    let signal_to_noise = if variance > 0.0 {
        net_flux / variance.sqrt()
    } else {
        0.0
    };

    AperturePhotometry {
        aperture_flux,
        background_level,
        net_flux,
        signal_to_noise,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtract_bias_removes_offset() {
        let input = [10.0, 20.0, 30.0];
        let output = subtract_bias(&input, 5.0);
        assert_eq!(output, vec![5.0, 15.0, 25.0]);
    }

    #[test]
    fn apply_flat_field_scales_signal() {
        let input = [100.0, 200.0];
        let output = apply_flat_field(&input, 2.0);
        assert_eq!(output, vec![50.0, 100.0]);
    }

    #[test]
    fn calibrate_frame_removes_bias_dark_and_flat() {
        let frame = [100.0, 120.0, 110.0, 130.0];
        let reduced = calibrate_frame(&frame, CalibrationFrame {
            bias: 10.0,
            dark_current: 2.0,
            flat_field: 2.0,
        });

        assert_eq!(reduced.pixels, vec![44.0, 54.0, 49.0, 59.0]);
        assert!((reduced.median_signal - 51.5).abs() < 1e-9);
    }

    #[test]
    fn aperture_photometry_reports_positive_net_flux() {
        let width = 5usize;
        let height = 5usize;
        let mut frame = vec![10.0; width * height];

        frame[12] = 100.0;
        frame[13] = 95.0;
        frame[17] = 92.0;
        frame[18] = 90.0;

        let result = aperture_photometry(&frame, width, height, 2.0, 2.0, 1.5, 2.0, 3.0);
        assert!(result.background_level >= 10.0);
        assert!(result.net_flux > 0.0);
        assert!(result.signal_to_noise > 0.0);
    }
}
