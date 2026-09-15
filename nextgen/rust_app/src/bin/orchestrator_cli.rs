use std::path::PathBuf;

use observatory_core::{
    build_campaign, build_campaign_report, build_sequence, build_session_summary,
    config_to_targets, execute_campaign, parse_campaign_config, reduce_sequence,
    write_campaign_report_json, write_session_summary_json, CampaignTarget, SequenceStep,
};

#[derive(Clone, Debug, Default, PartialEq)]
struct CliOptions {
    config_path: Option<String>,
    output_dir: Option<String>,
    json_stdout: bool,
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        match arg.as_str() {
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
            _ if arg.starts_with("--output-dir=") => {
                options.output_dir = Some(arg.trim_start_matches("--output-dir=").to_string());
                index += 1;
            }
            _ => {
                if options.config_path.is_none() {
                    options.config_path = Some(arg.clone());
                } else {
                    eprintln!("unexpected argument: '{arg}'");
                    std::process::exit(1);
                }
                index += 1;
            }
        }
    }

    options
}

fn synthesize_reduction(_target: &CampaignTarget, config: &observatory_core::CampaignConfig) -> observatory_core::SequenceReductionResult {
    let width = 16usize;
    let height = 16usize;
    let mut image = Vec::with_capacity(width * height);

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - 8.0;
            let dy = y as f64 - 8.0;
            let base = 10.0 + config.bias + config.dark_current;
            let star = 120.0 * (-((dx * dx + dy * dy) / (2.0 * 1.5 * 1.5))).exp();
            let value = base + star / config.flat_field.max(1e-12);
            image.push(value);
        }
    }

    reduce_sequence(observatory_core::ReductionRequest {
        width,
        height,
        image,
        bias: config.bias,
        dark_current: config.dark_current,
        flat_field: config.flat_field,
        threshold: config.threshold,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);

    let config_path = options.config_path.clone().unwrap_or_else(|| {
        eprintln!("Usage: orchestrator_cli <config-file> [--output-dir out] [--json]");
        std::process::exit(1);
    });

    let text = std::fs::read_to_string(&config_path).unwrap_or_else(|error| {
        eprintln!("failed to read config '{config_path}': {error}");
        std::process::exit(1);
    });

    let config = parse_campaign_config(&text).unwrap_or_else(|error| {
        eprintln!("invalid config: {error}");
        std::process::exit(1);
    });

    let targets = config_to_targets(&config);
    if targets.is_empty() {
        eprintln!("no targets found in config");
        std::process::exit(1);
    }

    let reductions = targets
        .iter()
        .map(|target| synthesize_reduction(target, &config))
        .collect::<Vec<_>>();

    let campaign = build_campaign(&targets);
    let steps = targets
        .iter()
        .map(|target| SequenceStep {
            target: target.name,
            filter: "R",
            exposure_s: 60.0,
            repeat_count: 2,
        })
        .collect::<Vec<_>>();
    let sequence = build_sequence(targets[0].name, "R", &steps);
    let session = build_session_summary(&campaign, &sequence);

    let execution = execute_campaign(&targets, &reductions);
    let report = build_campaign_report(&execution);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let report_path = dir.join("campaign_report.json");
        let session_path = dir.join("session_summary.json");

        if let Err(error) = write_campaign_report_json(report_path.to_str().unwrap(), &report) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        if let Err(error) = write_session_summary_json(session_path.to_str().unwrap(), &session) {
            eprintln!("{error}");
            std::process::exit(1);
        }

        println!("report_written={}", report_path.display());
        println!("session_written={}", session_path.display());
    }

    if options.json_stdout {
        println!("{{\n  \"campaign_target_count\": {},\n  \"total_priority\": {},\n  \"sequence_total_exposure_s\": {},\n  \"ready\": {},\n  \"sequence_valid\": {},\n  \"report_ready\": {}\n}}",
            session.campaign_target_count,
            session.total_priority,
            session.sequence_total_exposure_s,
            session.ready,
            session.sequence_valid,
            report.ready,
        );
    } else {
        println!("campaign_name={}", config.name);
        println!("campaign_ready={}", report.ready);
        println!("session_ready={}", session.ready);
        println!("target_count={}", report.target_count);
        println!("valid_targets={}", report.valid_targets);
        println!("total_priority={}", session.total_priority);
        println!("total_exposure_s={}", session.sequence_total_exposure_s);
    }

    if !session.ready || !report.ready {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_accepts_config_output_and_json() {
        let args = vec![
            "sample.cfg".to_string(),
            "--output-dir".to_string(),
            "out".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.config_path, Some("sample.cfg".to_string()));
        assert_eq!(parsed.output_dir, Some("out".to_string()));
        assert!(parsed.json_stdout);
    }
}
