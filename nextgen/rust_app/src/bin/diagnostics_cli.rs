use std::path::PathBuf;

use observatory_core::{
    detect_cpu_features, diagnostic_report_to_json, parse_campaign_config,
    run_observation_cycle, run_system_diagnostics, LedgerEventType, SessionLedger,
};

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
        eprintln!("Usage: diagnostics_cli <config-file> [--filter R] [--exposure 60] [--repeats 2] [--output-dir out] [--json]");
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

    let mut ledger = SessionLedger::new("OBS-DIAG-001");
    ledger.record_event(LedgerEventType::SessionStart, None, "Diagnostic session initiated", 100);
    ledger.record_event(LedgerEventType::ConfigParsed, None, &format!("Loaded config from {config_path}"), 120);

    let cpu = detect_cpu_features();

    let controller_result = run_observation_cycle(
        &config_text,
        &options.filter,
        options.exposure_s,
        options.repeats,
        options.output_dir.as_deref(),
    )
    .unwrap_or_else(|error| {
        ledger.record_event(LedgerEventType::Error, None, &format!("Cycle error: {error}"), 200);
        eprintln!("observation cycle failed: {error}");
        std::process::exit(1);
    });

    ledger.record_event(
        LedgerEventType::AstrometrySolved,
        None,
        &format!("Astrometry status: ready={}", controller_result.astrometry_ready),
        250,
    );
    ledger.record_event(
        LedgerEventType::FrameCalibrated,
        None,
        &format!("Calibration status: ready={}", controller_result.acquisition_ready),
        300,
    );
    ledger.record_event(
        LedgerEventType::SessionComplete,
        None,
        &format!("Session outcome: ready={}", controller_result.ready),
        400,
    );

    let report = run_system_diagnostics(&cpu, &controller_result, 0.0002, 18.2);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let diag_path = dir.join("diagnostics_report.json");
        let diag_str = diagnostic_report_to_json(&report);
        if let Err(error) = std::fs::write(&diag_path, diag_str) {
            eprintln!("failed to write diagnostics report: {error}");
            std::process::exit(1);
        }

        let ledger_path = dir.join("session_ledger.json");
        if let Err(error) = ledger.write_to_file(ledger_path.to_str().unwrap()) {
            eprintln!("failed to write ledger: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        println!("{}", diagnostic_report_to_json(&report));
    } else {
        println!("system_healthy={}", report.metrics.system_healthy);
        println!("cpu_avx2_supported={}", report.metrics.cpu_avx2_supported);
        println!("abi_version_match={}", report.metrics.abi_version_match);
        println!("astrometry_rms_residual_deg={:.8}", report.metrics.astrometry_rms_residual_deg);
        println!("calibration_mean_snr={:.4}", report.metrics.calibration_mean_snr);
        println!("ledger_entries={}", ledger.entries.len());
        println!("ledger_integrity_verified={}", ledger.verify_integrity());
        for log in &report.diagnostic_log {
            println!("  [diag] {log}");
        }
    }

    if !report.metrics.system_healthy {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_accepts_diagnostics_flags() {
        let args = vec![
            "sample.cfg".to_string(),
            "--filter".to_string(),
            "V".to_string(),
            "--exposure".to_string(),
            "90".to_string(),
            "--repeats".to_string(),
            "4".to_string(),
            "--output-dir".to_string(),
            "diag_out".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.config_path, Some("sample.cfg".to_string()));
        assert_eq!(parsed.filter, "V");
        assert!((parsed.exposure_s - 90.0).abs() < 1e-9);
        assert_eq!(parsed.repeats, 4);
        assert_eq!(parsed.output_dir, Some("diag_out".to_string()));
        assert!(parsed.json_stdout);
    }
}
