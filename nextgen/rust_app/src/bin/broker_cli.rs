use std::path::PathBuf;

use observatory_core::{
    AlertSeverity, ObservatoryEventBroker,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    topic: Option<String>,
    min_severity: AlertSeverity,
    publish_mock_events: bool,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            topic: None,
            min_severity: AlertSeverity::Info,
            publish_mock_events: true,
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
            "--topic" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --topic");
                    std::process::exit(1);
                }
                options.topic = Some(args[index + 1].clone());
                index += 2;
            }
            "--severity" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --severity");
                    std::process::exit(1);
                }
                options.min_severity = match args[index + 1].to_lowercase().as_str() {
                    "warning" => AlertSeverity::Warning,
                    "critical" => AlertSeverity::Critical,
                    _ => AlertSeverity::Info,
                };
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

    let mut broker = ObservatoryEventBroker::new();

    // Populate with actual observatory lifecycle telemetry events
    broker.publish_event(
        "weather/safety",
        AlertSeverity::Info,
        "meteorology_sensor",
        "Sky transparency nominal, humidity 48%, wind 12 km/h",
        100,
    );

    broker.publish_event(
        "autofocus/v_curve",
        AlertSeverity::Info,
        "focus_engine",
        "Autofocus sequence completed, optimal step=25045, min HFD=1.85 px",
        250,
    );

    broker.publish_event(
        "mount/meridian_flip",
        AlertSeverity::Warning,
        "mount_safety_guard",
        "Target approaching meridian limit (HA=+5.2 deg). Auto-flip in 540s",
        500,
    );

    broker.publish_event(
        "science/transient",
        AlertSeverity::Critical,
        "subtraction_pipeline",
        "Transient candidate discovered at (RA 10.6847, Dec 41.2687) with SNR 35.0 sigma",
        750,
    );

    broker.publish_event(
        "qc/frame_rejection",
        AlertSeverity::Warning,
        "frame_qc_engine",
        "Frame #004 rejected: star roundness 0.58 below threshold (wind gust trail)",
        900,
    );

    let json_str = broker.to_json();

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let event_file = dir.join("observatory_events.json");
        if let Err(error) = std::fs::write(&event_file, &json_str) {
            eprintln!("failed to write events log: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("total_events_published={}", broker.events.len());
        println!("active_topics={}", broker.topics.len());

        let filtered = broker.query_events_by_min_severity(options.min_severity);
        println!("events_matching_min_severity_{:?}={}", options.min_severity, filtered.len());

        for e in filtered {
            if let Some(ref filter_topic) = options.topic {
                if &e.topic != filter_topic {
                    continue;
                }
            }
            println!(
                "  [{:?}] [{}] (from {}) -> {}",
                e.severity, e.topic, e.source, e.payload
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_broker_options() {
        let args = vec![
            "--topic".to_string(),
            "science/transient".to_string(),
            "--severity".to_string(),
            "critical".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.topic, Some("science/transient".to_string()));
        assert_eq!(parsed.min_severity, AlertSeverity::Critical);
        assert!(parsed.json_stdout);
    }
}
