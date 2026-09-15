use observatory_core::{
    build_campaign, build_sequence, build_session_summary, CampaignTarget, SequenceStep,
};

fn parse_target(arg: &str) -> Result<CampaignTarget, String> {
    let fields: Vec<&str> = arg.split(':').collect();
    if fields.len() != 4 {
        return Err(format!(
            "invalid target definition '{arg}'. Expected format: NAME:RA_DEG:DEC_DEG:PRIORITY"
        ));
    }

    let name = fields[0].trim();
    if name.is_empty() {
        return Err(format!("empty target name in '{arg}'"));
    }

    let ra_deg = fields[1].trim().parse::<f64>().map_err(|_| format!("invalid RA in '{arg}'"))?;
    let dec_deg = fields[2].trim().parse::<f64>().map_err(|_| format!("invalid DEC in '{arg}'"))?;
    let priority = fields[3].trim().parse::<u8>().map_err(|_| format!("invalid PRIORITY in '{arg}'"))?;

    Ok(CampaignTarget {
        name: Box::leak(name.to_owned().into_boxed_str()),
        ra_deg,
        dec_deg,
        priority,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: session_cli --target NAME:RA:DEC:PRIORITY [--target ...] [--filter R] [--exposure 60] [--repeats 2] [--output session.json] [--json]");
        return;
    }

    let mut targets = Vec::new();
    let mut filter = "R";
    let mut exposure_s = 60.0_f64;
    let mut repeats = 2usize;
    let mut output_path = None;
    let mut json_stdout = false;

    let mut index = 0usize;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--filter" => {
                if index + 1 < args.len() {
                    filter = Box::leak(args[index + 1].clone().into_boxed_str());
                    index += 2;
                    continue;
                }
                eprintln!("missing value after --filter");
                std::process::exit(1);
            }
            "--exposure" => {
                if index + 1 < args.len() {
                    exposure_s = args[index + 1].parse::<f64>().unwrap_or_else(|_| {
                        eprintln!("invalid exposure value");
                        std::process::exit(1);
                    });
                    index += 2;
                    continue;
                }
                eprintln!("missing value after --exposure");
                std::process::exit(1);
            }
            "--repeats" => {
                if index + 1 < args.len() {
                    repeats = args[index + 1].parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("invalid repeat count");
                        std::process::exit(1);
                    });
                    index += 2;
                    continue;
                }
                eprintln!("missing value after --repeats");
                std::process::exit(1);
            }
            "--output" => {
                if index + 1 < args.len() {
                    output_path = Some(args[index + 1].clone());
                    index += 2;
                    continue;
                }
                eprintln!("missing value after --output");
                std::process::exit(1);
            }
            "--json" => {
                json_stdout = true;
                index += 1;
            }
            _ => {
                if arg.starts_with("--target=") {
                    let cleaned = arg.trim_start_matches("--target=");
                    match parse_target(cleaned) {
                        Ok(target) => targets.push(target),
                        Err(message) => {
                            eprintln!("{message}");
                            std::process::exit(1);
                        }
                    }
                } else if arg == "--target" {
                    if index + 1 < args.len() {
                        match parse_target(&args[index + 1]) {
                            Ok(target) => targets.push(target),
                            Err(message) => {
                                eprintln!("{message}");
                                std::process::exit(1);
                            }
                        }
                        index += 2;
                        continue;
                    }
                    eprintln!("missing value after --target");
                    std::process::exit(1);
                } else {
                    match parse_target(&arg) {
                        Ok(target) => targets.push(target),
                        Err(message) => {
                            eprintln!("{message}");
                            std::process::exit(1);
                        }
                    }
                }
                index += 1;
            }
        }
    }

    if targets.is_empty() {
        eprintln!("no valid targets were provided");
        std::process::exit(1);
    }

    let campaign = build_campaign(&targets);
    let steps = targets
        .iter()
        .map(|target| SequenceStep {
            target: target.name,
            filter,
            exposure_s,
            repeat_count: repeats,
        })
        .collect::<Vec<_>>();
    let sequence = build_sequence(targets[0].name, filter, &steps);
    let summary = build_session_summary(&campaign, &sequence);

    if let Some(path) = output_path.as_ref() {
        if let Err(error) = observatory_core::write_session_summary_json(path, &summary) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("session_written={path}");
    }

    if json_stdout {
        println!("{}", observatory_core::session_summary_to_json(&summary));
    } else {
        println!("campaign_target_count={}", summary.campaign_target_count);
        println!("total_priority={}", summary.total_priority);
        println!("sequence_total_exposure_s={}", summary.sequence_total_exposure_s);
        println!("ready={}", summary.ready);
        println!("sequence_valid={}", summary.sequence_valid);
    }

    if !summary.ready {
        std::process::exit(1);
    }
}
