use std::path::PathBuf;

use observatory_core::{
    evaluate_frame_quality, frame_quality_to_json, QcThresholds, StarMeasurement,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    background: f64,
    star_fwhms: Vec<f64>,
    star_roundnesses: Vec<f64>,
    star_snrs: Vec<f64>,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            background: 150.0,
            star_fwhms: vec![2.2, 2.4, 2.3],
            star_roundnesses: vec![0.92, 0.94, 0.91],
            star_snrs: vec![22.0, 19.5, 25.0],
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
            "--background" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --background");
                    std::process::exit(1);
                }
                options.background = args[index + 1].parse::<f64>().unwrap_or(150.0);
                index += 2;
            }
            "--fwhms" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --fwhms");
                    std::process::exit(1);
                }
                options.star_fwhms = args[index + 1]
                    .split(',')
                    .filter_map(|s| s.trim().parse::<f64>().ok())
                    .collect();
                index += 2;
            }
            "--roundness" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --roundness");
                    std::process::exit(1);
                }
                options.star_roundnesses = args[index + 1]
                    .split(',')
                    .filter_map(|s| s.trim().parse::<f64>().ok())
                    .collect();
                index += 2;
            }
            "--snr" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --snr");
                    std::process::exit(1);
                }
                options.star_snrs = args[index + 1]
                    .split(',')
                    .filter_map(|s| s.trim().parse::<f64>().ok())
                    .collect();
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

    let count = options
        .star_fwhms
        .len()
        .min(options.star_roundnesses.len())
        .min(options.star_snrs.len());

    let stars: Vec<StarMeasurement> = (0..count)
        .map(|i| StarMeasurement {
            star_id: i + 1,
            x: 100.0 + (i as f64) * 50.0,
            y: 100.0 + (i as f64) * 30.0,
            flux: 10000.0,
            fwhm_pixels: options.star_fwhms[i],
            roundness: options.star_roundnesses[i],
            peak_adu: 15000.0,
            snr: options.star_snrs[i],
        })
        .collect();

    let thresholds = QcThresholds::default();
    let summary = evaluate_frame_quality(&stars, options.background, &thresholds);

    let json_str = frame_quality_to_json(&summary);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let qc_file = dir.join("frame_quality.json");
        if let Err(error) = std::fs::write(&qc_file, &json_str) {
            eprintln!("failed to write frame quality report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("total_detected_stars={}", summary.total_detected_stars);
        println!("median_fwhm_pixels={:.4}", summary.median_fwhm_pixels);
        println!("median_roundness={:.4}", summary.median_roundness);
        println!("median_snr={:.4}", summary.median_snr);
        println!("background_level={:.2}", summary.background_level);
        println!("quality_flag={:?}", summary.quality_flag);
        println!("is_accepted={}", summary.is_accepted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_qc_options() {
        let args = vec![
            "--background".to_string(),
            "200.0".to_string(),
            "--fwhms".to_string(),
            "2.1,2.3,2.2".to_string(),
            "--roundness".to_string(),
            "0.9,0.95,0.92".to_string(),
            "--snr".to_string(),
            "15.0,20.0,18.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.background - 200.0).abs() < 1e-9);
        assert_eq!(parsed.star_fwhms.len(), 3);
        assert_eq!(parsed.star_roundnesses.len(), 3);
        assert_eq!(parsed.star_snrs.len(), 3);
        assert!(parsed.json_stdout);
    }
}
