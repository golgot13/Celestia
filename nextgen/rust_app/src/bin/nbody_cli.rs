use std::path::PathBuf;

use observatory_core::{
    nbody_report_to_json, propagate_nbody_system, CelestialBody, IntegratorMethod,
    ASTRONOMICAL_UNIT_M, GRAVITATIONAL_CONSTANT,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    days: f64,
    dt_hours: f64,
    relativity: bool,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            days: 365.25,
            dt_hours: 6.0,
            relativity: true,
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
            "--days" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --days");
                    std::process::exit(1);
                }
                options.days = args[index + 1].parse::<f64>().unwrap_or(365.25);
                index += 2;
            }
            "--dt-hours" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --dt-hours");
                    std::process::exit(1);
                }
                options.dt_hours = args[index + 1].parse::<f64>().unwrap_or(6.0);
                index += 2;
            }
            "--relativity" => {
                options.relativity = true;
                index += 1;
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

    let sun_mass = 1.98847e30;
    let sun = CelestialBody {
        name: "Sun",
        mass_kg: sun_mass,
        position_m: [0.0, 0.0, 0.0],
        velocity_m_s: [0.0, 0.0, 0.0],
    };

    // Inner Solar System setup (Mercury, Venus, Earth, Mars, Jupiter)
    // Mercury: 0.3871 AU, v ~ 47.36 km/s
    let r_merc = 0.387098 * ASTRONOMICAL_UNIT_M;
    let v_merc = (GRAVITATIONAL_CONSTANT * sun_mass / r_merc).sqrt();
    let mercury = CelestialBody {
        name: "Mercury",
        mass_kg: 3.3011e23,
        position_m: [r_merc, 0.0, 0.0],
        velocity_m_s: [0.0, v_merc, 0.0],
    };

    // Earth: 1.0 AU, v ~ 29.78 km/s
    let r_earth = 1.0 * ASTRONOMICAL_UNIT_M;
    let v_earth = (GRAVITATIONAL_CONSTANT * sun_mass / r_earth).sqrt();
    let earth = CelestialBody {
        name: "Earth",
        mass_kg: 5.9722e24,
        position_m: [r_earth, 0.0, 0.0],
        velocity_m_s: [0.0, v_earth, 0.0],
    };

    // Jupiter: 5.2044 AU, v ~ 13.07 km/s
    let r_jup = 5.2044 * ASTRONOMICAL_UNIT_M;
    let v_jup = (GRAVITATIONAL_CONSTANT * sun_mass / r_jup).sqrt();
    let jupiter = CelestialBody {
        name: "Jupiter",
        mass_kg: 1.89813e27,
        position_m: [0.0, r_jup, 0.0],
        velocity_m_s: [-v_jup, 0.0, 0.0],
    };

    let bodies = vec![sun, mercury, earth, jupiter];
    let duration_s = options.days * 86400.0;
    let dt_s = options.dt_hours * 3600.0;

    let report = propagate_nbody_system(
        bodies,
        duration_s,
        dt_s,
        IntegratorMethod::SymplecticYoshida4th,
        options.relativity,
    );

    let json_str = nbody_report_to_json(&report);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let nbody_file = dir.join("nbody_propagation.json");
        if let Err(error) = std::fs::write(&nbody_file, &json_str) {
            eprintln!("failed to write nbody report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("duration_days={:.2}", options.days);
        println!("step_count={}", report.step_count);
        println!("energy_relative_drift={:.6e}", report.energy_relative_drift);
        for body in &report.bodies {
            println!(
                "  Body '{}': r=({:.3}, {:.3}, {:.3}) AU, v=({:.2}, {:.2}, {:.2}) km/s",
                body.name,
                body.position_m[0] / ASTRONOMICAL_UNIT_M,
                body.position_m[1] / ASTRONOMICAL_UNIT_M,
                body.position_m[2] / ASTRONOMICAL_UNIT_M,
                body.velocity_m_s[0] / 1000.0,
                body.velocity_m_s[1] / 1000.0,
                body.velocity_m_s[2] / 1000.0
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_nbody_options() {
        let args = vec![
            "--days".to_string(),
            "730.5".to_string(),
            "--dt-hours".to_string(),
            "2.0".to_string(),
            "--relativity".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.days - 730.5).abs() < 1e-9);
        assert!((parsed.dt_hours - 2.0).abs() < 1e-9);
        assert!(parsed.relativity);
        assert!(parsed.json_stdout);
    }
}
