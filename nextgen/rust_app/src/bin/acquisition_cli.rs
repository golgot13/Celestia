use observatory_core::{
    build_campaign, build_sequence, build_session_summary, initialize_instrument,
    initialize_mount, plan_observation, start_capture, CampaignTarget, CaptureSession,
    InstrumentConfig, ObservationMeta, SequenceStep,
};

#[derive(Clone, Debug, Default, PartialEq)]
struct CliOptions {
    targets: Vec<String>,
    filter: String,
    exposure_s: f64,
    repeats: usize,
    json_stdout: bool,
}

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
            "--json" => {
                options.json_stdout = true;
                index += 1;
            }
            "--target" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --target");
                    std::process::exit(1);
                }
                options.targets.push(args[index + 1].clone());
                index += 2;
            }
            _ if args[index].starts_with("--target=") => {
                options.targets.push(args[index][9..].to_string());
                index += 1;
            }
            _ => {
                if args[index] == "--help" || args[index] == "-h" {
                    println!("Usage: acquisition_cli --target NAME:RA:DEC:PRIORITY [--target ...] [--filter R] [--exposure 60] [--repeats 2] [--json]");
                    std::process::exit(0);
                }
                options.targets.push(args[index].clone());
                index += 1;
            }
        }
    }

    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = parse_cli_args(&args);

    if opts.targets.is_empty() {
        eprintln!("no valid targets were provided");
        std::process::exit(1);
    }

    let targets = opts
        .targets
        .iter()
        .map(|target| parse_target(target).unwrap_or_else(|message| {
            eprintln!("{message}");
            std::process::exit(1);
        }))
        .collect::<Vec<_>>();

    let campaign = build_campaign(&targets);
    let steps = targets
        .iter()
        .map(|target| SequenceStep {
            target: target.name,
            filter: Box::leak(opts.filter.clone().into_boxed_str()),
            exposure_s: opts.exposure_s,
            repeat_count: opts.repeats,
        })
        .collect::<Vec<_>>();
    let sequence = build_sequence(targets[0].name, Box::leak(opts.filter.clone().into_boxed_str()), &steps);
    let session = build_session_summary(&campaign, &sequence);

    let mount = initialize_mount();
    let instrument = initialize_instrument(InstrumentConfig {
        name: "MainCam",
        pixel_width: 4096,
        pixel_height: 4096,
        gain_e_per_adu: 1.2,
        read_noise_e: 6.5,
        temperature_c: -10.0,
        enabled: true,
    });

    let observations = targets
        .iter()
        .map(|target| {
            let meta = ObservationMeta {
                target_name: target.name,
                ra_deg: target.ra_deg,
                dec_deg: target.dec_deg,
                exposure_s: opts.exposure_s,
                filter: Box::leak(opts.filter.clone().into_boxed_str()),
                gain_e_per_adu: 1.2,
                read_noise_e: 6.5,
                temperature_c: -10.0,
            };
            plan_observation(meta)
        })
        .collect::<Vec<_>>();

    let capture_ok = observations.iter().all(|obs| obs.usable)
        && mount.tracking
        && instrument.ready
        && instrument.exposure_ready;

    let capture = start_capture(CaptureSession {
        target: targets[0].name,
        exposure_s: opts.exposure_s,
        filter: Box::leak(opts.filter.clone().into_boxed_str()),
        count: opts.repeats * targets.len(),
        enabled: capture_ok,
    });

    let ready = session.ready && capture.sync_ok && mount.tracking && instrument.ready;

    if opts.json_stdout {
        println!(
            "{{\n  \"campaign_target_count\": {},\n  \"total_priority\": {},\n  \"sequence_total_exposure_s\": {},\n  \"mount_tracking\": {},\n  \"instrument_ready\": {},\n  \"exposure_ready\": {},\n  \"capture_ok\": {},\n  \"ready\": {}\n}}",
            session.campaign_target_count,
            session.total_priority,
            session.sequence_total_exposure_s,
            mount.tracking,
            instrument.ready,
            instrument.exposure_ready,
            capture.sync_ok,
            ready
        );
        if !ready {
            std::process::exit(1);
        }
        return;
    }

    println!("campaign_target_count={}", session.campaign_target_count);
    println!("total_priority={}", session.total_priority);
    println!("sequence_total_exposure_s={}", session.sequence_total_exposure_s);
    println!("mount_tracking={}", mount.tracking);
    println!("instrument_ready={}", instrument.ready);
    println!("exposure_ready={}", instrument.exposure_ready);
    println!("capture_ok={}", capture.sync_ok);
    println!("ready={}", ready);

    if !ready {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_reads_targets_and_options() {
        let args = vec![
            "--target".to_string(),
            "M31:10.6847:41.2687:5".to_string(),
            "--filter".to_string(),
            "R".to_string(),
            "--exposure".to_string(),
            "60".to_string(),
            "--repeats".to_string(),
            "2".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.targets.len(), 1);
        assert_eq!(parsed.filter, "R");
        assert_eq!(parsed.exposure_s, 60.0);
        assert_eq!(parsed.repeats, 2);
        assert!(parsed.json_stdout);
    }
}
