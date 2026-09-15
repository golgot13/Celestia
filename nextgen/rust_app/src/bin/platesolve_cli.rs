use std::path::PathBuf;

use observatory_core::{
    blind_solver_result_to_json, solve_blind_astrometry, CatalogSource, DetectedSource,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    scale: f64,
    tolerance: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            tolerance: 0.015,
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
            "--scale" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --scale");
                    std::process::exit(1);
                }
                options.scale = args[index + 1].parse::<f64>().unwrap_or(1.0);
                index += 2;
            }
            "--tolerance" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --tolerance");
                    std::process::exit(1);
                }
                options.tolerance = args[index + 1].parse::<f64>().unwrap_or(0.015);
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

    let catalog = vec![
        CatalogSource { id: 1001, ra_deg: 10.6847, dec_deg: 41.2687, magnitude: 7.2 },
        CatalogSource { id: 1002, ra_deg: 10.8847, dec_deg: 41.2687, magnitude: 8.4 },
        CatalogSource { id: 1003, ra_deg: 10.6847, dec_deg: 41.5687, magnitude: 9.1 },
        CatalogSource { id: 1004, ra_deg: 10.9847, dec_deg: 41.6687, magnitude: 8.8 },
    ];

    let detected = vec![
        DetectedSource { x: 250.0, y: 250.0, flux: 50000.0, snr: 60.0 },
        DetectedSource { x: 450.0, y: 250.0, flux: 30000.0, snr: 45.0 },
        DetectedSource { x: 250.0, y: 550.0, flux: 18000.0, snr: 30.0 },
        DetectedSource { x: 550.0, y: 650.0, flux: 22000.0, snr: 35.0 },
    ];

    let result = solve_blind_astrometry(&detected, &catalog, options.scale, options.tolerance)
        .unwrap_or_else(|error| {
            eprintln!("plate solving failed: {error}");
            std::process::exit(1);
        });

    let json_str = blind_solver_result_to_json(&result);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let solve_file = dir.join("plate_solve_result.json");
        if let Err(error) = std::fs::write(&solve_file, &json_str) {
            eprintln!("failed to write plate solve result: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("solved={}", result.solved);
        println!("matched_stars_count={}", result.matched_stars_count);
        println!("center_ra_deg={:.6}", result.center_ra_deg);
        println!("center_dec_deg={:.6}", result.center_dec_deg);
        println!("pixel_scale_arcsec={:.4}", result.pixel_scale_arcsec);
        println!("rms_error_arcsec={:.4}", result.rms_error_arcsec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_platesolve_options() {
        let args = vec![
            "--scale".to_string(),
            "1.25".to_string(),
            "--tolerance".to_string(),
            "0.02".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.scale - 1.25).abs() < 1e-9);
        assert!((parsed.tolerance - 0.02).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
