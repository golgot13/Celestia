use std::path::PathBuf;

use observatory_core::{
    compute_apparent_ephemeris, iod_result_to_json, solve_gauss_initial_orbit_determination,
    AstrometricObservation, KeplerianElements,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    object_name: String,
    true_a: f64,
    true_e: f64,
    true_inc: f64,
    dt_days: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            object_name: "2026-NEO-01".to_string(),
            true_a: 2.35,
            true_e: 0.12,
            true_inc: 8.5,
            dt_days: 5.0,
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
                options.object_name = args[index + 1].clone();
                index += 2;
            }
            "--a" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --a");
                    std::process::exit(1);
                }
                options.true_a = args[index + 1].parse::<f64>().unwrap_or(2.35);
                index += 2;
            }
            "--e" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --e");
                    std::process::exit(1);
                }
                options.true_e = args[index + 1].parse::<f64>().unwrap_or(0.12);
                index += 2;
            }
            "--inc" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --inc");
                    std::process::exit(1);
                }
                options.true_inc = args[index + 1].parse::<f64>().unwrap_or(8.5);
                index += 2;
            }
            "--dt" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --dt");
                    std::process::exit(1);
                }
                options.dt_days = args[index + 1].parse::<f64>().unwrap_or(5.0);
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

    let leaked_name: &'static str = Box::leak(options.object_name.clone().into_boxed_str());

    // True orbital ground truth
    let target_elements = KeplerianElements {
        name: leaked_name,
        semi_major_axis_au: options.true_a,
        eccentricity: options.true_e,
        inclination_deg: options.true_inc,
        longitude_ascending_node_deg: 45.0,
        argument_periapsis_deg: 30.0,
        mean_anomaly_epoch_deg: 10.0,
        epoch_jd: 2459000.0,
        absolute_magnitude_h: 18.0,
        slope_parameter_g: 0.15,
    };

    let t1 = 2459000.0;
    let t2 = t1 + options.dt_days;
    let t3 = t1 + 2.0 * options.dt_days;

    // Earth orbital motion approximation over 10 days
    let omega_earth = 2.0 * std::f64::consts::PI / 365.25;
    let pos_earth = |t: f64| -> [f64; 3] {
        let angle = omega_earth * (t - 2459000.0);
        [angle.cos(), angle.sin(), 0.0]
    };

    let earth1 = pos_earth(t1);
    let earth2 = pos_earth(t2);
    let earth3 = pos_earth(t3);

    let ephem1 = compute_apparent_ephemeris(&target_elements, earth1, t1);
    let ephem2 = compute_apparent_ephemeris(&target_elements, earth2, t2);
    let ephem3 = compute_apparent_ephemeris(&target_elements, earth3, t3);

    let obs1 = AstrometricObservation {
        epoch_jd: t1,
        ra_deg: ephem1.ra_deg,
        dec_deg: ephem1.dec_deg,
        observer_pos_au: earth1,
    };
    let obs2 = AstrometricObservation {
        epoch_jd: t2,
        ra_deg: ephem2.ra_deg,
        dec_deg: ephem2.dec_deg,
        observer_pos_au: earth2,
    };
    let obs3 = AstrometricObservation {
        epoch_jd: t3,
        ra_deg: ephem3.ra_deg,
        dec_deg: ephem3.dec_deg,
        observer_pos_au: earth3,
    };

    let iod_res = solve_gauss_initial_orbit_determination(&obs1, &obs2, &obs3, leaked_name)
        .unwrap_or_else(|error| {
            eprintln!("Initial orbit determination failed: {error}");
            std::process::exit(1);
        });

    let json_str = iod_result_to_json(&iod_res);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let iod_file = dir.join("initial_orbit_determination.json");
        if let Err(error) = std::fs::write(&iod_file, &json_str) {
            eprintln!("failed to write IOD report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("object_name={}", iod_res.elements.name);
        println!("epoch_jd={:.4}", iod_res.elements.epoch_jd);
        println!("semi_major_axis_au={:.6}", iod_res.semi_major_axis_au);
        println!("eccentricity={:.6}", iod_res.eccentricity);
        println!("inclination_deg={:.4}", iod_res.inclination_deg);
        println!("orbital_period_years={:.4}", iod_res.orbital_period_years);
        println!("converged={}", iod_res.converged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_iod_flags() {
        let args = vec![
            "--name".to_string(),
            "2026-NEO-X".to_string(),
            "--a".to_string(),
            "1.85".to_string(),
            "--e".to_string(),
            "0.22".to_string(),
            "--inc".to_string(),
            "12.0".to_string(),
            "--dt".to_string(),
            "3.5".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.object_name, "2026-NEO-X");
        assert!((parsed.true_a - 1.85).abs() < 1e-9);
        assert!((parsed.true_e - 0.22).abs() < 1e-9);
        assert!((parsed.true_inc - 12.0).abs() < 1e-9);
        assert!((parsed.dt_days - 3.5).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
