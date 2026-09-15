use std::collections::HashMap;
use std::path::PathBuf;

use observatory_core::{
    calibrate_target_magnitude, calibration_result_to_json, solve_zero_point_and_extinction,
    ObservedStarPhotometry, StandardStar,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    filter: String,
    flux: f64,
    exposure_s: f64,
    airmass: f64,
    catalog_mag: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            filter: "V".to_string(),
            flux: 150000.0,
            exposure_s: 60.0,
            airmass: 1.25,
            catalog_mag: 12.35,
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
            "--filter" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --filter");
                    std::process::exit(1);
                }
                options.filter = args[index + 1].clone();
                index += 2;
            }
            "--flux" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --flux");
                    std::process::exit(1);
                }
                options.flux = args[index + 1].parse::<f64>().unwrap_or(150000.0);
                index += 2;
            }
            "--exposure" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --exposure");
                    std::process::exit(1);
                }
                options.exposure_s = args[index + 1].parse::<f64>().unwrap_or(60.0);
                index += 2;
            }
            "--airmass" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --airmass");
                    std::process::exit(1);
                }
                options.airmass = args[index + 1].parse::<f64>().unwrap_or(1.25);
                index += 2;
            }
            "--cat-mag" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --cat-mag");
                    std::process::exit(1);
                }
                options.catalog_mag = args[index + 1].parse::<f64>().unwrap_or(12.35);
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

    let mut cat_mags = HashMap::new();
    let leaked_filter: &'static str = Box::leak(options.filter.clone().into_boxed_str());
    cat_mags.insert(leaked_filter, options.catalog_mag);

    let catalog = vec![
        StandardStar {
            name: "StandardRef1",
            ra_deg: 15.0,
            dec_deg: 45.0,
            catalog_magnitudes: cat_mags,
        },
    ];

    let obs = vec![
        ObservedStarPhotometry {
            star_name: "StandardRef1",
            filter: leaked_filter,
            instrumental_flux: options.flux,
            exposure_s: options.exposure_s,
            airmass: options.airmass,
        },
    ];

    let cal = solve_zero_point_and_extinction(&obs, &catalog, &options.filter)
        .unwrap_or_else(|error| {
            eprintln!("photometric calibration failed: {error}");
            std::process::exit(1);
        });

    let target_result = calibrate_target_magnitude(&obs[0], &cal)
        .unwrap_or_else(|error| {
            eprintln!("magnitude calibration failed: {error}");
            std::process::exit(1);
        });

    let json_str = calibration_result_to_json(&cal);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let cal_file = dir.join("photometric_calibration.json");
        if let Err(error) = std::fs::write(&cal_file, &json_str) {
            eprintln!("failed to write calibration result: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("filter={}", cal.filter);
        println!("zero_point_mag={:.4}", cal.zero_point_mag);
        println!("extinction_coefficient={:.4}", cal.extinction_coefficient);
        println!("residual_rms_mag={:.4}", cal.residual_rms_mag);
        println!("reference_star_count={}", cal.reference_star_count);
        println!("calibrated_mag={:.4}", target_result.calibrated_mag);
        println!("instrumental_mag={:.4}", target_result.instrumental_mag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_photometry_options() {
        let args = vec![
            "--filter".to_string(),
            "R".to_string(),
            "--flux".to_string(),
            "250000".to_string(),
            "--exposure".to_string(),
            "120".to_string(),
            "--airmass".to_string(),
            "1.15".to_string(),
            "--cat-mag".to_string(),
            "11.5".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.filter, "R");
        assert!((parsed.flux - 250000.0).abs() < 1e-9);
        assert!((parsed.exposure_s - 120.0).abs() < 1e-9);
        assert!((parsed.airmass - 1.15).abs() < 1e-9);
        assert!((parsed.catalog_mag - 11.5).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
