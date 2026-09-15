use std::path::PathBuf;

use observatory_core::{
    config_to_targets, parse_campaign_config, schedule_observation_queue,
    schedule_plan_to_json, GeographicCoord, SiteLimits,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    config_path: Option<String>,
    latitude_deg: f64,
    longitude_deg: f64,
    elevation_m: f64,
    min_altitude_deg: f64,
    max_airmass: f64,
    jd: f64,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            latitude_deg: 43.9308, // Observatoire de Haute-Provence
            longitude_deg: 5.7133,
            elevation_m: 650.0,
            min_altitude_deg: 20.0,
            max_airmass: 2.5,
            jd: 2451545.0, // J2000.0 epoch default
            exposure_s: 60.0,
            repeats: 2,
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
                options.latitude_deg = args[index + 1].parse::<f64>().unwrap_or(options.latitude_deg);
                index += 2;
            }
            "--lon" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --lon");
                    std::process::exit(1);
                }
                options.longitude_deg = args[index + 1].parse::<f64>().unwrap_or(options.longitude_deg);
                index += 2;
            }
            "--jd" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --jd");
                    std::process::exit(1);
                }
                options.jd = args[index + 1].parse::<f64>().unwrap_or(options.jd);
                index += 2;
            }
            "--min-alt" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --min-alt");
                    std::process::exit(1);
                }
                options.min_altitude_deg = args[index + 1].parse::<f64>().unwrap_or(options.min_altitude_deg);
                index += 2;
            }
            "--max-airmass" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --max-airmass");
                    std::process::exit(1);
                }
                options.max_airmass = args[index + 1].parse::<f64>().unwrap_or(options.max_airmass);
                index += 2;
            }
            "--exposure" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --exposure");
                    std::process::exit(1);
                }
                options.exposure_s = args[index + 1].parse::<f64>().unwrap_or(options.exposure_s);
                index += 2;
            }
            "--repeats" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --repeats");
                    std::process::exit(1);
                }
                options.repeats = args[index + 1].parse::<usize>().unwrap_or(options.repeats);
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
                if options.config_path.is_none() {
                    options.config_path = Some(args[index].clone());
                } else {
                    eprintln!("unexpected argument: '{}'", args[index]);
                    std::process::exit(1);
                }
                index += 1;
            }
        }
    }

    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);

    let config_path = options.config_path.clone().unwrap_or_else(|| {
        eprintln!("Usage: scheduler_cli <config-file> [--lat 43.9] [--lon 5.7] [--jd 2451545.0] [--min-alt 20] [--output-dir out] [--json]");
        std::process::exit(1);
    });

    let config_text = std::fs::read_to_string(&config_path).unwrap_or_else(|error| {
        eprintln!("failed to read config '{config_path}': {error}");
        std::process::exit(1);
    });

    let config = parse_campaign_config(&config_text).unwrap_or_else(|error| {
        eprintln!("invalid config: {error}");
        std::process::exit(1);
    });

    let targets = config_to_targets(&config);
    if targets.is_empty() {
        eprintln!("no targets found in campaign");
        std::process::exit(1);
    }

    let site = GeographicCoord {
        latitude_deg: options.latitude_deg,
        longitude_deg: options.longitude_deg,
        elevation_m: options.elevation_m,
    };

    let limits = SiteLimits {
        min_altitude_deg: options.min_altitude_deg,
        max_airmass: options.max_airmass,
    };

    let plan = schedule_observation_queue(
        &targets,
        &site,
        &limits,
        options.jd,
        options.exposure_s,
        options.repeats,
    );

    let plan_json = schedule_plan_to_json(&plan);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let plan_file = dir.join("schedule_plan.json");
        if let Err(error) = std::fs::write(&plan_file, &plan_json) {
            eprintln!("failed to write schedule plan: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        println!("{plan_json}");
    } else {
        println!("site_latitude_deg={:.4}", plan.site_latitude_deg);
        println!("site_longitude_deg={:.4}", plan.site_longitude_deg);
        println!("total_targets={}", plan.total_targets);
        println!("observable_targets={}", plan.observable_targets);
        println!("total_estimated_duration_s={:.2}", plan.total_estimated_duration_s);
        for obs in &plan.queue {
            println!(
                "  Rank #{}: {} (P{}) - Alt={:.1}deg, Airmass={:.2}, Score={:.1}, Duration={:.0}s",
                obs.rank, obs.target_name, obs.priority, obs.altitude_deg, obs.airmass, obs.merit_score, obs.estimated_duration_s
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_scheduler_options() {
        let args = vec![
            "sample.cfg".to_string(),
            "--lat".to_string(),
            "44.0".to_string(),
            "--lon".to_string(),
            "6.0".to_string(),
            "--jd".to_string(),
            "2451550.0".to_string(),
            "--min-alt".to_string(),
            "25.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.config_path, Some("sample.cfg".to_string()));
        assert!((parsed.latitude_deg - 44.0).abs() < 1e-9);
        assert!((parsed.longitude_deg - 6.0).abs() < 1e-9);
        assert!((parsed.jd - 2451550.0).abs() < 1e-9);
        assert!((parsed.min_altitude_deg - 25.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
