use observatory_core::{
    build_campaign_report, config_to_targets, execute_campaign, parse_campaign_config,
    reduce_sequence, run_campaign,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().cloned().unwrap_or_else(|| {
        eprintln!("Usage: campaign_from_config <config-file>");
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
