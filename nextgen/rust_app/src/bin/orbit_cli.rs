use std::path::PathBuf;

use observatory_core::{
    compute_apparent_ephemeris, target_ephemeris_to_json, KeplerianElements,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    target_name: String,
    semi_major_axis_au: f64,
    eccentricity: f64,
    inclination_deg: f64,
    node_deg: f64,
    peri_deg: f64,
    mean_anomaly_deg: f64,
    epoch_jd: f64,
    abs_mag_h: f64,
    slope_g: f64,
    target_jd: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            target_name: "Ceres".to_string(),
            semi_major_axis_au: 2.767,
            eccentricity: 0.0758,
            inclination_deg: 10.593,
            node_deg: 80.305,
            peri_deg: 73.597,
            mean_anomaly_deg: 77.372,
            epoch_jd: 2459000.5,
            abs_mag_h: 3.34,
            slope_g: 0.12,
            target_jd: 2459000.5,
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
                options.target_name = args[index + 1].clone();
                index += 2;
            }
            "--a" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --a");
                    std::process::exit(1);
                }
                options.semi_major_axis_au = args[index + 1].parse::<f64>().unwrap_or(2.767);
                index += 2;
            }
            "--e" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --e");
                    std::process::exit(1);
                }
                options.eccentricity = args[index + 1].parse::<f64>().unwrap_or(0.0758);
                index += 2;
            }
            "--inc" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --inc");
                    std::process::exit(1);
                }
                options.inclination_deg = args[index + 1].parse::<f64>().unwrap_or(10.593);
                index += 2;
            }
            "--node" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --node");
                    std::process::exit(1);
                }
                options.node_deg = args[index + 1].parse::<f64>().unwrap_or(80.305);
                index += 2;
            }
            "--peri" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --peri");
                    std::process::exit(1);
                }
                options.peri_deg = args[index + 1].parse::<f64>().unwrap_or(73.597);
                index += 2;
            }
            "--m" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --m");
                    std::process::exit(1);
                }
                options.mean_anomaly_deg = args[index + 1].parse::<f64>().unwrap_or(77.372);
                index += 2;
            }
            "--jd" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --jd");
                    std::process::exit(1);
                }
                options.target_jd = args[index + 1].parse::<f64>().unwrap_or(2459000.5);
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

    let leaked_name: &'static str = Box::leak(options.target_name.clone().into_boxed_str());

    let elements = KeplerianElements {
        name: leaked_name,
        semi_major_axis_au: options.semi_major_axis_au,
        eccentricity: options.eccentricity,
        inclination_deg: options.inclination_deg,
        longitude_ascending_node_deg: options.node_deg,
        argument_periapsis_deg: options.peri_deg,
        mean_anomaly_epoch_deg: options.mean_anomaly_deg,
        epoch_jd: options.epoch_jd,
        absolute_magnitude_h: options.abs_mag_h,
        slope_parameter_g: options.slope_g,
    };

    // Earth position in J2000 heliocentric equatorial coordinates at epoch
    let earth_pos = [1.0, 0.0, 0.0];
    let ephem = compute_apparent_ephemeris(&elements, earth_pos, options.target_jd);

    let json_str = target_ephemeris_to_json(&ephem);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let ephem_file = dir.join("orbit_ephemeris.json");
        if let Err(error) = std::fs::write(&ephem_file, &json_str) {
            eprintln!("failed to write ephemeris output: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("name={}", ephem.name);
        println!("target_jd={:.4}", ephem.target_jd);
        println!("ra_deg={:.6}", ephem.ra_deg);
        println!("dec_deg={:.6}", ephem.dec_deg);
        println!("heliocentric_distance_au={:.6}", ephem.heliocentric_distance_au);
        println!("geocentric_distance_au={:.6}", ephem.geocentric_distance_au);
        println!("apparent_magnitude_v={:.2}", ephem.apparent_magnitude_v);
        println!("phase_angle_deg={:.2}", ephem.phase_angle_deg);
        println!("proper_motion_ra={:.2} arcsec/h", ephem.proper_motion_ra_arcsec_hr);
        println!("proper_motion_dec={:.2} arcsec/h", ephem.proper_motion_dec_arcsec_hr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_orbit_elements_flags() {
        let args = vec![
            "--name".to_string(),
            "Vesta".to_string(),
            "--a".to_string(),
            "2.361".to_string(),
            "--e".to_string(),
            "0.0887".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.target_name, "Vesta");
        assert!((parsed.semi_major_axis_au - 2.361).abs() < 1e-9);
        assert!((parsed.eccentricity - 0.0887).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
