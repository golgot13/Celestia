use std::path::PathBuf;

use observatory_core::{parse_campaign_config, run_observation_cycle};

#[derive(Clone, Debug, Default, PartialEq)]
struct CliOptions {
    config_path: Option<String>,
    filter: String,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<String>,
    json_stdout: bool,
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions {
        filter: "R".to_string(),
        exposure_s: 60.0,
        repeats: 2,
        ..CliOptions::default()
    };

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
            "--exposure" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --exposure");
                    std::process::exit(1);
                }
                options.exposure_s = args[index + 1].parse::<f64>().unwrap_or_else(|_| {
                    eprintln!("invalid exposure value");
                    std::process::exit(1);
                });
                index += 2;
            }
            "--repeats" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --repeats");
                    std::process::exit(1);
                }
                options.repeats = args[index + 1].parse::<usize>().unwrap_or_else(|_| {
                    eprintln!("invalid repeat count");
                    std::process::exit(1);
                });
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
            _ if args[index].starts_with("--output-dir=") => {
                options.output_dir = Some(args[index].trim_start_matches("--output-dir=").to_string());
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
        eprintln!("Usage: controller_cli <config-file> [--filter R] [--exposure 60] [--repeats 2] [--output-dir out] [--json]");
        std::process::exit(1);
    });

    let config_text = std::fs::read_to_string(&config_path).unwrap_or_else(|error| {
        eprintln!("failed to read config '{config_path}': {error}");
        std::process::exit(1);
    });

    let _ = parse_campaign_config(&config_text).unwrap_or_else(|error| {
        eprintln!("invalid config: {error}");
        std::process::exit(1);
    });

    let result = run_observation_cycle(
        &config_text,
        &options.filter,
        options.exposure_s,
        options.repeats,
        options.output_dir.as_deref(),
    )
    .unwrap_or_else(|error| {
        eprintln!("controller cycle failed: {error}");
        std::process::exit(1);
    });

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        if let Some(report_path) = &result.report_path {
            let report_file = PathBuf::from(report_path);
            if let Err(error) = std::fs::read_to_string(&report_file).map(|_| ()) {
                eprintln!("failed to confirm report output '{report_path}': {error}");
                std::process::exit(1);
            }
        }
        if let Some(session_path) = &result.session_path {
            let session_file = PathBuf::from(session_path);
            if let Err(error) = std::fs::read_to_string(&session_file).map(|_| ()) {
                eprintln!("failed to confirm session output '{session_path}': {error}");
                std::process::exit(1);
            }
        }
    }

    if options.json_stdout {
        println!(
            "{{\n  \"campaign_ready\": {},\n  \"session_ready\": {},\n  \"acquisition_ready\": {},\n  \"astrometry_ready\": {},\n  \"report_ready\": {},\n  \"ready\": {}\n}}",
            result.campaign_ready,
            result.session_ready,
            result.acquisition_ready,
            result.astrometry_ready,
            result.report_ready,
            result.ready
        );
    } else {
        println!("campaign_ready={}", result.campaign_ready);
        println!("session_ready={}", result.session_ready);
        println!("acquisition_ready={}", result.acquisition_ready);
        println!("astrometry_ready={}", result.astrometry_ready);
        println!("report_ready={}", result.report_ready);
        println!("ready={}", result.ready);
        if let Some(path) = result.report_path {
            println!("report_path={path}");
        }
        if let Some(path) = result.session_path {
            println!("session_path={path}");
        }
    }

    if !result.ready {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_accepts_controller_inputs() {
        let args = vec![
            "sample.cfg".to_string(),
            "--filter".to_string(),
            "L".to_string(),
            "--exposure".to_string(),
            "120".to_string(),
            "--repeats".to_string(),
            "3".to_string(),
            "--output-dir".to_string(),
            "out".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.config_path, Some("sample.cfg".to_string()));
        assert_eq!(parsed.filter, "L");
        assert!((parsed.exposure_s - 120.0).abs() < 1e-9);
        assert_eq!(parsed.repeats, 3);
        assert_eq!(parsed.output_dir, Some("out".to_string()));
        assert!(parsed.json_stdout);
    }
}
