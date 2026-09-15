use std::path::PathBuf;

use observatory_core::{parse_campaign_config, run_application_runtime};

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
    let opts = parse_cli_args(&args);

    let config_path = opts.config_path.clone().unwrap_or_else(|| {
        eprintln!("Usage: runtime_cli <config-file> [--filter R] [--exposure 60] [--repeats 2] [--output-dir out] [--json]");
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

    if let Some(output_dir) = opts.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }
    }

    let result = run_application_runtime(
        &config_text,
        &opts.filter,
        opts.exposure_s,
        opts.repeats,
        opts.output_dir.as_deref(),
    )
    .unwrap_or_else(|error| {
        eprintln!("runtime failed: {error}");
        std::process::exit(1);
    });

    if opts.json_stdout {
        println!(
            "{{\n  \"state\": \"{:?}\",\n  \"config_name\": \"{}\",\n  \"campaign_ready\": {},\n  \"session_ready\": {},\n  \"acquisition_ready\": {},\n  \"astrometry_ready\": {},\n  \"report_ready\": {},\n  \"ready\": {}\n}}",
            result.state,
            result.config_name,
            result.controller.campaign_ready,
            result.controller.session_ready,
            result.controller.acquisition_ready,
            result.controller.astrometry_ready,
            result.controller.report_ready,
            result.controller.ready
        );
    } else {
        println!("state={:?}", result.state);
        println!("config_name={}", result.config_name);
        println!("campaign_ready={}", result.controller.campaign_ready);
        println!("session_ready={}", result.controller.session_ready);
        println!("acquisition_ready={}", result.controller.acquisition_ready);
        println!("astrometry_ready={}", result.controller.astrometry_ready);
        println!("report_ready={}", result.controller.report_ready);
        println!("ready={}", result.controller.ready);
    }

    if !result.controller.ready {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_accepts_runtime_configuration() {
        let args = vec![
            "sample.cfg".to_string(),
            "--filter".to_string(),
            "G".to_string(),
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
        assert_eq!(parsed.filter, "G");
        assert!((parsed.exposure_s - 120.0).abs() < 1e-9);
        assert_eq!(parsed.repeats, 3);
        assert_eq!(parsed.output_dir, Some("out".to_string()));
        assert!(parsed.json_stdout);
    }
}
