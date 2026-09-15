use std::path::PathBuf;

use observatory_core::{
    pointing_model_to_json, solve_pointing_model_least_squares, AtmosphericConditions,
    PointingCalibrationStar,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    site_lat: f64,
    temperature_c: f64,
    pressure_hpa: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            site_lat: 43.9308,
            temperature_c: 12.0,
            pressure_hpa: 1015.0,
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
            "--lat" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --lat");
                    std::process::exit(1);
                }
                options.site_lat = args[index + 1].parse::<f64>().unwrap_or(43.9308);
                index += 2;
            }
            "--temp" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --temp");
                    std::process::exit(1);
                }
                options.temperature_c = args[index + 1].parse::<f64>().unwrap_or(12.0);
                index += 2;
            }
            "--pressure" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --pressure");
                    std::process::exit(1);
                }
                options.pressure_hpa = args[index + 1].parse::<f64>().unwrap_or(1015.0);
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

    let conditions = AtmosphericConditions {
        temperature_c: options.temperature_c,
        pressure_hpa: options.pressure_hpa,
        relative_humidity_pct: 45.0,
        wavelength_nm: 550.0,
    };

    // 6-star calibration set across different quadrants
    let stars = vec![
        PointingCalibrationStar {
            catalog_ra_deg: 15.0,
            catalog_dec_deg: 25.0,
            mount_hour_angle_deg: 0.003,
            mount_dec_deg: 25.002,
            lst_deg: 15.0,
            site_latitude_deg: options.site_lat,
        },
        PointingCalibrationStar {
            catalog_ra_deg: 75.0,
            catalog_dec_deg: 50.0,
            mount_hour_angle_deg: 0.004,
            mount_dec_deg: 50.003,
            lst_deg: 75.0,
            site_latitude_deg: options.site_lat,
        },
        PointingCalibrationStar {
            catalog_ra_deg: 165.0,
            catalog_dec_deg: 35.0,
            mount_hour_angle_deg: -0.002,
            mount_dec_deg: 34.999,
            lst_deg: 165.0,
            site_latitude_deg: options.site_lat,
        },
        PointingCalibrationStar {
            catalog_ra_deg: 245.0,
            catalog_dec_deg: 60.0,
            mount_hour_angle_deg: -0.004,
            mount_dec_deg: 59.998,
            lst_deg: 245.0,
            site_latitude_deg: options.site_lat,
        },
    ];

    let model = solve_pointing_model_least_squares(&stars, &conditions).unwrap_or_else(|error| {
        eprintln!("pointing model solve failed: {error}");
        std::process::exit(1);
    });

    let json_str = pointing_model_to_json(&model);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let model_file = dir.join("pointing_model.json");
        if let Err(error) = std::fs::write(&model_file, &json_str) {
            eprintln!("failed to write pointing model output: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("polar_elevation_error_arcsec={:.2}", model.polar_elevation_error_arcsec);
        println!("polar_azimuth_error_arcsec={:.2}", model.polar_azimuth_error_arcsec);
        println!("non_perpendicularity_arcsec={:.2}", model.non_perpendicularity_arcsec);
        println!("collimation_cone_error_arcsec={:.2}", model.collimation_cone_error_arcsec);
        println!("rms_residual_arcsec={:.2}", model.rms_residual_arcsec);
        println!("star_count={}", model.star_count);
        println!("converged={}", model.converged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_pointing_options() {
        let args = vec![
            "--lat".to_string(),
            "45.0".to_string(),
            "--temp".to_string(),
            "5.0".to_string(),
            "--pressure".to_string(),
            "1020.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.site_lat - 45.0).abs() < 1e-9);
        assert!((parsed.temperature_c - 5.0).abs() < 1e-9);
        assert!((parsed.pressure_hpa - 1020.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
