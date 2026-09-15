use observatory_core::{
    build_campaign_report, config_to_targets, execute_campaign, parse_campaign_config,
    reduce_sequence, run_campaign, write_campaign_report_json,
};

#[derive(Clone, Debug, Default, PartialEq)]
struct CliOptions {
    output_path: Option<String>,
    json_stdout: bool,
    positional: Vec<String>,
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        match arg.as_str() {
            "--output" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --output");
                    std::process::exit(1);
                }
                options.output_path = Some(args[index + 1].clone());
                index += 2;
            }
            "--json" => {
                options.json_stdout = true;
                index += 1;
            }
            _ if arg.starts_with("--output=") => {
                options.output_path = Some(arg.trim_start_matches("--output=").to_string());
                index += 1;
            }
            _ => {
                options.positional.push(arg.clone());
                index += 1;
            }
        }
    }

    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);
    let output_path = options.output_path;
    let json_stdout = options.json_stdout;
    let positional = options.positional;

    let path = positional.first().cloned().unwrap_or_else(|| {
        eprintln!("Usage: campaign_from_config <config-file> [--output report.json] [--json]");
        std::process::exit(1);
    });

    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("failed to read config '{path}': {error}");
        std::process::exit(1);
    });

    let config = parse_campaign_config(&text).unwrap_or_else(|error| {
        eprintln!("invalid config: {error}");
        std::process::exit(1);
    });

    let targets = config_to_targets(&config);
    let reductions = targets
        .iter()
        .map(|_target| reduce_sequence(observatory_core::ReductionRequest {
            width: 16,
            height: 16,
            image: (0..256)
                .map(|index| {
                    let x = index % 16;
                    let y = index / 16;
                    let dx = x as f64 - 8.0;
                    let dy = y as f64 - 8.0;
                    let base = 10.0 + config.bias + config.dark_current;
                    let star = 120.0 * (-((dx * dx + dy * dy) / (2.0 * 1.5 * 1.5))).exp();
                    base + star / config.flat_field.max(1e-12)
                })
                .collect(),
            bias: config.bias,
            dark_current: config.dark_current,
            flat_field: config.flat_field,
            threshold: config.threshold,
        }))
        .collect::<Vec<_>>();

    let outcome = run_campaign(&targets, &reductions);
    let execution = execute_campaign(&targets, &reductions);
    let report = build_campaign_report(&execution);

    if let Some(output) = output_path.as_ref() {
        if let Err(error) = write_campaign_report_json(output, &report) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("report_written={output}");
    }

    if json_stdout {
        println!("{}", observatory_core::campaign_report_to_json(&report));
        if !outcome.ready {
            std::process::exit(1);
        }
        return;
    }

    println!("campaign_name={}", config.name);
    println!("campaign_ready={}", report.ready);
    println!("target_count={}", report.target_count);
    println!("valid_targets={}", report.valid_targets);
    println!("total_sources={}", report.total_sources);
    println!("total_flux={}", report.total_flux);
    println!("average_flux={}", report.average_flux);
    println!("summary_status={}", if report.ready { "ready" } else { "degraded" });

    for (index, summary) in report.summaries.iter().enumerate() {
        println!(
            "summary[{index}]={}:valid={}:flux={}:successes={}",
            summary.target_name, summary.valid, summary.total_flux, summary.successful_reductions
        );
    }

    if !outcome.ready {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_keeps_output_path_and_json_flag() {
        let args = vec![
            "sample.cfg".to_string(),
            "--output".to_string(),
            "report.json".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.positional, vec!["sample.cfg".to_string()]);
        assert_eq!(parsed.output_path, Some("report.json".to_string()));
        assert!(parsed.json_stdout);
    }
}
