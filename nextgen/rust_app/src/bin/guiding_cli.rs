use std::path::PathBuf;

use observatory_core::{
    compute_dither_offset, guider_summary_to_json, GuiderLoop,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    plate_scale: f64,
    lock_x: f64,
    lock_y: f64,
    simulated_steps: usize,
    dither_step_pixels: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            plate_scale: 0.85,
            lock_x: 512.0,
            lock_y: 512.0,
            simulated_steps: 10,
            dither_step_pixels: 4.0,
            output_dir: None,
            json_stdout: false,
        }
    }
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--plate-scale" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --plate-scale");
                    std::process::exit(1);
                }
                options.plate_scale = args[index + 1].parse::<f64>().unwrap_or(0.85);
                index += 2;
            }
            "--lock-x" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --lock-x");
                    std::process::exit(1);
                }
                options.lock_x = args[index + 1].parse::<f64>().unwrap_or(512.0);
                index += 2;
            }
            "--lock-y" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --lock-y");
                    std::process::exit(1);
                }
                options.lock_y = args[index + 1].parse::<f64>().unwrap_or(512.0);
                index += 2;
            }
            "--steps" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --steps");
                    std::process::exit(1);
                }
                options.simulated_steps = args[index + 1].parse::<usize>().unwrap_or(10);
                index += 2;
            }
            "--dither" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --dither");
                    std::process::exit(1);
                }
                options.dither_step_pixels = args[index + 1].parse::<f64>().unwrap_or(4.0);
                index += 2;
            }
            "--output-dir" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --output-dir");
                    std::process::exit(1);
                }
                options.output_dir = Some(args[index + 1].clone());
                index += 2;
            }
            "--json" => {
                options.json_stdout = true;
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }

    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);

    let mut guider = GuiderLoop::new(options.plate_scale, options.lock_x, options.lock_y);

    // Simulate closed loop tracking with periodic mount PE (periodic error) and PID damping
    let mut cur_x = options.lock_x + 0.35;
    let mut cur_y = options.lock_y - 0.25;

    for step in 0..options.simulated_steps {
        let dt = 1.0;
        let corr = guider.process_guide_frame(cur_x, cur_y, dt);

        // Mount responds to pulse corrections with physical feedback
        let ra_response = corr.pulse_ra_ms * 0.001 * 0.5;
        let dec_response = corr.pulse_dec_ms * 0.001 * 0.5;

        // Add simulated wind gust/periodic error drift
        let drift_x = 0.05 * ((step as f64) * 0.5).sin();
        let drift_y = 0.04 * ((step as f64) * 0.5).cos();

        cur_x = cur_x - (ra_response / options.plate_scale) + drift_x;
        cur_y = cur_y - (dec_response / options.plate_scale) + drift_y;
    }

    let summary = guider.get_summary();
    let (dither_dx, dither_dy) = compute_dither_offset(0, options.dither_step_pixels);

    let json_str = guider_summary_to_json(&summary);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let guide_file = dir.join("guiding_summary.json");
        if let Err(error) = std::fs::write(&guide_file, &json_str) {
            eprintln!("failed to write guiding summary: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("total_cycles={}", summary.total_cycles);
        println!("rms_ra_arcsec={:.4}", summary.rms_ra_arcsec);
        println!("rms_dec_arcsec={:.4}", summary.rms_dec_arcsec);
        println!("total_rms_arcsec={:.4}", summary.total_rms_arcsec);
        println!("tracking_stable={}", summary.tracking_stable);
        println!("dither_sample_offset=({:.2}, {:.2}) px", dither_dx, dither_dy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_guider_options() {
        let args = vec![
            "--plate-scale".to_string(),
            "0.75".to_string(),
            "--lock-x".to_string(),
            "256.0".to_string(),
            "--lock-y".to_string(),
            "256.0".to_string(),
            "--steps".to_string(),
            "20".to_string(),
            "--dither".to_string(),
            "5.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.plate_scale - 0.75).abs() < 1e-9);
        assert!((parsed.lock_x - 256.0).abs() < 1e-9);
        assert!((parsed.lock_y - 256.0).abs() < 1e-9);
        assert_eq!(parsed.simulated_steps, 20);
        assert!((parsed.dither_step_pixels - 5.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
