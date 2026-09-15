use std::path::PathBuf;

use observatory_core::{
    compute_doppler_radial_velocity, evaluate_wavelength_at_pixel, extract_1d_spectrum_from_2d,
    radial_velocity_to_json, solve_dispersion_polynomial, LampEmissionLine, H_ALPHA,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    observed_line_pixel: f64,
    order: usize,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            observed_line_pixel: 512.0,
            order: 2,
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
            "--pixel" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --pixel");
                    std::process::exit(1);
                }
                options.observed_line_pixel = args[index + 1].parse::<f64>().unwrap_or(512.0);
                index += 2;
            }
            "--order" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --order");
                    std::process::exit(1);
                }
                options.order = args[index + 1].parse::<usize>().unwrap_or(2);
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

    // Calibration lamp lines (e.g. Neon-Argon / Thorium-Argon lines across dispersion axis)
    let lamp_lines = vec![
        LampEmissionLine { pixel_position: 128.0, known_wavelength_angstrom: 4200.0 },
        LampEmissionLine { pixel_position: 256.0, known_wavelength_angstrom: 4861.33 },
        LampEmissionLine { pixel_position: 512.0, known_wavelength_angstrom: 6562.81 },
        LampEmissionLine { pixel_position: 768.0, known_wavelength_angstrom: 7800.0 },
    ];

    let dispersion = solve_dispersion_polynomial(&lamp_lines, options.order)
        .unwrap_or_else(|error| {
            eprintln!("dispersion solution failed: {error}");
            std::process::exit(1);
        });

    // Create a synthetic 2D spectral image (width 1024, height 64) with stellar absorption line
    let width = 1024;
    let height = 64;
    let mut image_2d = vec![15.0; width * height]; // Sky background

    let trace_y = 32;
    for x in 0..width {
        let dx = (x as f64) - options.observed_line_pixel;
        // Stellar continuum with Gaussian absorption line
        let absorption = 1.0 - 0.6 * (-((dx * dx) / (2.0 * 3.0 * 3.0))).exp();
        for dy in -5..=5 {
            let y = (trace_y as isize + dy) as usize;
            let spatial_profile = (-((dy as f64).powi(2) / (2.0 * 2.0 * 2.0))).exp();
            image_2d[y * width + x] += 1200.0 * spatial_profile * absorption;
        }
    }

    let spectrum = extract_1d_spectrum_from_2d(
        &image_2d,
        width,
        height,
        trace_y,
        6,
        &dispersion,
    )
    .unwrap_or_else(|error| {
        eprintln!("spectral extraction failed: {error}");
        std::process::exit(1);
    });

    let observed_wavelength = evaluate_wavelength_at_pixel(&dispersion, options.observed_line_pixel);
    let rv = compute_doppler_radial_velocity(&H_ALPHA, observed_wavelength);

    let json_str = radial_velocity_to_json(&rv);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let rv_file = dir.join("radial_velocity.json");
        if let Err(error) = std::fs::write(&rv_file, &json_str) {
            eprintln!("failed to write radial velocity report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("line_name={}", rv.line_name);
        println!("rest_wavelength={:.4} A", rv.rest_wavelength_angstrom);
        println!("observed_wavelength={:.4} A", rv.observed_wavelength_angstrom);
        println!("doppler_shift={:.4} A", rv.doppler_shift_angstrom);
        println!("radial_velocity={:.2} km/s", rv.radial_velocity_km_s);
        println!("extracted_pixels={}", spectrum.pixel_count);
        println!("dispersion_rms={:.4} A", dispersion.rms_residual_angstrom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_spectroscopy_flags() {
        let args = vec![
            "--pixel".to_string(),
            "520.5".to_string(),
            "--order".to_string(),
            "2".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.observed_line_pixel - 520.5).abs() < 1e-9);
        assert_eq!(parsed.order, 2);
        assert!(parsed.json_stdout);
    }
}
