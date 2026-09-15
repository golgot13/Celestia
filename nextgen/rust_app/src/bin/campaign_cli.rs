use observatory_core::{run_campaign, CampaignTarget, SequenceReductionResult};

fn parse_target(arg: &str) -> Result<CampaignTarget, String> {
    let fields: Vec<&str> = arg.split(':').collect();
    if fields.len() != 4 {
        return Err(format!(
            "invalid target definition '{arg}'. Expected format: NAME:RA_DEG:DEC_DEG:PRIORITY"
        ));
    }

    let name_raw = fields[0].trim();
    if name_raw.is_empty() {
        return Err(format!("empty target name in '{arg}'"));
    }

    let name = Box::leak(name_raw.to_owned().into_boxed_str());
    let ra_deg = fields[1].trim().parse::<f64>().map_err(|_| format!("invalid RA in '{arg}'"))?;
    let dec_deg = fields[2].trim().parse::<f64>().map_err(|_| format!("invalid DEC in '{arg}'"))?;
    let priority = fields[3].trim().parse::<u8>().map_err(|_| format!("invalid PRIORITY in '{arg}'"))?;

    Ok(CampaignTarget {
        name,
        ra_deg,
        dec_deg,
        priority,
    })
}

fn build_reductions(targets: &[CampaignTarget]) -> Vec<SequenceReductionResult> {
    targets
        .iter()
        .map(|target| SequenceReductionResult {
            median_signal: 10.0 + target.priority as f64,
            source_count: target.priority as usize,
            total_flux: 100.0 * target.priority as f64,
            valid: true,
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: campaign_cli --target NAME:RA:DEC:PRIORITY [--target NAME:RA:DEC:PRIORITY ...]");
        println!("Example: campaign_cli --target M31:10.6847:41.2687:5 --target M45:56.75:24.1167:3");
        return;
    }

    let mut targets = Vec::new();
    for arg in args {
        if arg == "--target" {
            continue;
        }
        if arg.starts_with("--target=") {
            let cleaned = arg.trim_start_matches("--target=");
            match parse_target(cleaned) {
                Ok(target) => targets.push(target),
                Err(message) => {
                    eprintln!("{message}");
                    std::process::exit(1);
                }
            }
            continue;
        }
        match parse_target(&arg) {
            Ok(target) => targets.push(target),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
    }

    if targets.is_empty() {
        eprintln!("no valid targets were provided");
        std::process::exit(1);
    }

    let reductions = build_reductions(&targets);
    let outcome = run_campaign(&targets, &reductions);

    println!("campaign_ready={}", outcome.ready);
    println!("target_count={}", outcome.target_count);
    println!("valid_targets={}", outcome.valid_targets);
    println!("total_sources={}", outcome.total_sources);
    println!("total_flux={}", outcome.total_flux);
    println!("average_flux={}", outcome.average_flux);
    for (index, target) in targets.iter().enumerate() {
        println!(
            "target[{index}]={}:ra_deg={}:dec_deg={}:priority={}:flux={}",
            target.name, target.ra_deg, target.dec_deg, target.priority, reductions[index].total_flux
        );
    }
}
