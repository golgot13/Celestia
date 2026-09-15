use std::path::PathBuf;

use observatory_core::{
    compute_transit_parameters, generate_transit_light_curve, transit_parameters_to_json,
    ExoplanetSystem,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    name: String,
    radius_ratio_k: f64,
    semi_major_axis_a: f64,
    period_days: f64,
    inclination_deg: f64,
    epoch_jd: f64,
    u1: f64,
    u2: f64,
    step_minutes: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            name: "HD 209458 b".to_string(),
            radius_ratio_k: 0.1208,
            semi_major_axis_a: 8.76,
            period_days: 3.52474859,
            inclination_deg: 86.71,
            epoch_jd: 2452826.628521,
            u1: 0.35,
            u2: 0.20,
            step_minutes: 5.0,
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
            "--name" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --name");
                    std::process::exit(1);
                }
                options.name = args[index + 1].clone();
                index += 2;
            }
            "--k" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --k");
                    std::process::exit(1);
                }
                options.radius_ratio_k = args[index + 1].parse::<f64>().unwrap_or(0.1208);
                index += 2;
            }
            "--a" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --a");
                    std::process::exit(1);
                }
                options.semi_major_axis_a = args[index + 1].parse::<f64>().unwrap_or(8.76);
                index += 2;
            }
            "--period" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --period");
                    std::process::exit(1);
                }
                options.period_days = args[index + 1].parse::<f64>().unwrap_or(3.52474859);
                index += 2;
            }
            "--inc" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --inc");
                    std::process::exit(1);
                }
                options.inclination_deg = args[index + 1].parse::<f64>().unwrap_or(86.71);
                index += 2;
            }
            "--epoch" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --epoch");
                    std::process::exit(1);
                }
                options.epoch_jd = args[index + 1].parse::<f64>().unwrap_or(2452826.628521);
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

    let leaked_name: &'static str = Box::leak(options.name.clone().into_boxed_str());

    let system = ExoplanetSystem {
        name: leaked_name,
        planet_radius_ratio_k: options.radius_ratio_k,
        semi_major_axis_stellar_radii_a: options.semi_major_axis_a,
        orbital_period_days: options.period_days,
        inclination_deg: options.inclination_deg,
        transit_epoch_jd: options.epoch_jd,
        limb_darkening_u1: options.u1,
        limb_darkening_u2: options.u2,
    };

    let params = compute_transit_parameters(&system);

    // Generate 6 hours of light curve centered on transit epoch
    let half_window_days = 3.0 / 24.0;
    let start_jd = options.epoch_jd - half_window_days;
    let end_jd = options.epoch_jd + half_window_days;

    let points = generate_transit_light_curve(&system, start_jd, end_jd, options.step_minutes);

    let json_str = transit_parameters_to_json(&params, &system);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let transit_file = dir.join("exoplanet_transit.json");
        if let Err(error) = std::fs::write(&transit_file, &json_str) {
            eprintln!("failed to write transit parameters report: {error}");
            std::process::exit(1);
        }

        let lc_file = dir.join("exoplanet_lightcurve.csv");
        let mut csv = String::from("time_jd,phase,projected_z,relative_flux\n");
        for p in &points {
            csv.push_str(&format!(
                "{:.6},{:.6},{:.6},{:.6}\n",
                p.time_jd, p.phase, p.projected_separation_z, p.relative_flux
            ));
        }
        let _ = std::fs::write(&lc_file, csv);
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("system_name={}", system.name);
        println!("planet_radius_ratio_k={:.4}", system.planet_radius_ratio_k);
        println!("impact_parameter_b={:.4}", params.impact_parameter_b);
        println!("transit_depth_ppm={:.1} ppm", params.transit_depth_ppm);
        println!("total_duration_hours_t14={:.2} h", params.total_duration_hours_t14);
        println!("full_transit_duration_hours_t23={:.2} h", params.full_transit_duration_hours_t23);
        println!("is_transiting={}", params.is_transiting);
        println!("simulated_lightcurve_points={}", points.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_exoplanet_flags() {
        let args = vec![
            "--name".to_string(),
            "Kepler-186 f".to_string(),
            "--k".to_string(),
            "0.021".to_string(),
            "--a".to_string(),
            "110.0".to_string(),
            "--period".to_string(),
            "129.9".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.name, "Kepler-186 f");
        assert!((parsed.radius_ratio_k - 0.021).abs() < 1e-9);
        assert!((parsed.semi_major_axis_a - 110.0).abs() < 1e-9);
        assert!((parsed.period_days - 129.9).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
