use std::path::PathBuf;

use observatory_core::{
    evaluate_meridian_safety, meridian_evaluation_to_json, plan_meridian_flip_sequence,
    MeridianSide, MountSafetyLimits,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    ra: f64,
    dec: f64,
    lst: f64,
    lat: f64,
    pier_side: MeridianSide,
    exposure_s: f64,
    settle_s: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            ra: 10.6847,
            dec: 41.2687,
            lst: 12.0,
            lat: 43.9308,
            pier_side: MeridianSide::West,
            exposure_s: 300.0,
            settle_s: 15.0,
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
            "--ra" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --ra");
                    std::process::exit(1);
                }
                options.ra = args[index + 1].parse::<f64>().unwrap_or(10.6847);
                index += 2;
            }
            "--dec" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --dec");
                    std::process::exit(1);
                }
                options.dec = args[index + 1].parse::<f64>().unwrap_or(41.2687);
                index += 2;
            }
            "--lst" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --lst");
                    std::process::exit(1);
                }
                options.lst = args[index + 1].parse::<f64>().unwrap_or(12.0);
                index += 2;
            }
            "--lat" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --lat");
                    std::process::exit(1);
                }
                options.lat = args[index + 1].parse::<f64>().unwrap_or(43.9308);
                index += 2;
            }
            "--pier" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --pier");
                    std::process::exit(1);
                }
                options.pier_side = match args[index + 1].to_lowercase().as_str() {
                    "east" => MeridianSide::East,
                    "west" => MeridianSide::West,
                    _ => MeridianSide::West,
                };
                index += 2;
            }
            "--exposure" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --exposure");
                    std::process::exit(1);
                }
                options.exposure_s = args[index + 1].parse::<f64>().unwrap_or(300.0);
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

    let limits = MountSafetyLimits::default();

    let eval = evaluate_meridian_safety(
        options.ra,
        options.dec,
        options.lst,
        options.lat,
        options.pier_side,
        options.exposure_s,
        &limits,
    );

    let flip_plan = if eval.flip_required {
        Some(plan_meridian_flip_sequence(
            options.ra,
            options.dec,
            options.pier_side,
            options.settle_s,
        ))
    } else {
        None
    };

    let json_str = meridian_evaluation_to_json(&eval);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let safety_file = dir.join("meridian_safety.json");
        if let Err(error) = std::fs::write(&safety_file, &json_str) {
            eprintln!("failed to write safety evaluation: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("current_ha_deg={:.4}", eval.current_ha_deg);
        println!("current_pier_side={:?}", eval.current_pier_side);
        println!("target_pier_side={:?}", eval.target_pier_side);
        println!("flip_required={}", eval.flip_required);
        println!("time_until_limit_seconds={:.1}", eval.time_until_limit_seconds);
        println!("safety_status={:?}", eval.safety_status);
        println!("safe_to_expose_duration_s={:.1}", eval.safe_to_expose_duration_s);
        if let Some(plan) = flip_plan {
            println!(
                "  [Flip Plan] Initial: {:?} -> Final: {:?}, Settle: {:.1}s",
                plan.initial_pier_side, plan.final_pier_side, plan.settle_time_seconds
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_safety_flags() {
        let args = vec![
            "--ra".to_string(),
            "180.0".to_string(),
            "--dec".to_string(),
            "20.0".to_string(),
            "--lst".to_string(),
            "185.0".to_string(),
            "--pier".to_string(),
            "west".to_string(),
            "--exposure".to_string(),
            "600.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.ra - 180.0).abs() < 1e-9);
        assert!((parsed.dec - 20.0).abs() < 1e-9);
        assert!((parsed.lst - 185.0).abs() < 1e-9);
        assert_eq!(parsed.pier_side, MeridianSide::West);
        assert!((parsed.exposure_s - 600.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
