use std::path::PathBuf;

use observatory_core::{
    analyze_frame, frame_quality_to_json, read_fits_file, CalibrationFrame, DetectionParams,
    FrameAnalysis, QcThresholds,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    frame_path: Option<String>,
    calibration: CalibrationFrame,
    detection: DetectionParams,
    thresholds: QcThresholds,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            frame_path: None,
            calibration: CalibrationFrame {
                bias: 0.0,
                dark_current: 0.0,
                flat_field: 1.0,
            },
            detection: DetectionParams::default(),
            thresholds: QcThresholds::default(),
            output_dir: None,
            json_stdout: false,
        }
    }
}

fn require_value<'a>(args: &'a [String], index: usize, flag: &str) -> &'a str {
    match args.get(index + 1) {
        Some(value) => value.as_str(),
        None => {
            eprintln!("missing value after {flag}");
            std::process::exit(1);
        }
    }
}

fn parse_number<T: std::str::FromStr>(raw: &str, flag: &str) -> T {
    raw.parse::<T>().unwrap_or_else(|_| {
        eprintln!("invalid value '{raw}' for {flag}");
        std::process::exit(1);
    })
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--frame" => {
                options.frame_path = Some(require_value(args, index, "--frame").to_string());
                index += 2;
            }
            "--bias" => {
                options.calibration.bias = parse_number(require_value(args, index, "--bias"), "--bias");
                index += 2;
            }
            "--dark" => {
                options.calibration.dark_current =
                    parse_number(require_value(args, index, "--dark"), "--dark");
                index += 2;
            }
            "--flat" => {
                options.calibration.flat_field =
                    parse_number(require_value(args, index, "--flat"), "--flat");
                index += 2;
            }
            "--detection-sigma" => {
                options.detection.detection_sigma = parse_number(
                    require_value(args, index, "--detection-sigma"),
                    "--detection-sigma",
                );
                index += 2;
            }
            "--aperture" => {
                options.detection.aperture_radius_px =
                    parse_number(require_value(args, index, "--aperture"), "--aperture");
                index += 2;
            }
            "--annulus-inner" => {
                options.detection.annulus_inner_px = parse_number(
                    require_value(args, index, "--annulus-inner"),
                    "--annulus-inner",
                );
                index += 2;
            }
            "--annulus-outer" => {
                options.detection.annulus_outer_px = parse_number(
                    require_value(args, index, "--annulus-outer"),
                    "--annulus-outer",
                );
                index += 2;
            }
            "--max-sources" => {
                options.detection.max_sources =
                    parse_number(require_value(args, index, "--max-sources"), "--max-sources");
                index += 2;
            }
            "--max-fwhm" => {
                options.thresholds.max_fwhm_pixels =
                    parse_number(require_value(args, index, "--max-fwhm"), "--max-fwhm");
                index += 2;
            }
            "--min-roundness" => {
                options.thresholds.min_roundness = parse_number(
                    require_value(args, index, "--min-roundness"),
                    "--min-roundness",
                );
                index += 2;
            }
            "--min-snr" => {
                options.thresholds.min_snr =
                    parse_number(require_value(args, index, "--min-snr"), "--min-snr");
                index += 2;
            }
            "--max-background" => {
                options.thresholds.max_background = parse_number(
                    require_value(args, index, "--max-background"),
                    "--max-background",
                );
                index += 2;
            }
            "--saturation" => {
                options.thresholds.saturation_limit_adu =
                    parse_number(require_value(args, index, "--saturation"), "--saturation");
                index += 2;
            }
            "--min-stars" => {
                options.thresholds.min_star_count =
                    parse_number(require_value(args, index, "--min-stars"), "--min-stars");
                index += 2;
            }
            "--output-dir" => {
                options.output_dir = Some(require_value(args, index, "--output-dir").to_string());
                index += 2;
            }
            "--json" => {
                options.json_stdout = true;
                index += 1;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("unexpected argument '{other}'");
                print_usage();
                std::process::exit(1);
            }
        }
    }

    options
}

fn print_usage() {
    println!("Usage:");
    println!("  qc_cli --frame <file.fits> [options]");
    println!();
    println!("The quality report is computed from the sources actually detected in the");
    println!("frame; no measurement is ever synthesised.");
    println!();
    println!("Calibration applied before detection:");
    println!("  --bias <adu>              bias level (default: 0)");
    println!("  --dark <adu>              dark current level (default: 0)");
    println!("  --flat <value>            flat field scale (default: 1)");
    println!();
    println!("Detection:");
    println!("  --detection-sigma <value> detection threshold above the noise (default: 5)");
    println!("  --aperture <px>           photometric aperture radius (default: 4)");
    println!("  --annulus-inner <px>      background annulus inner radius (default: 8)");
    println!("  --annulus-outer <px>      background annulus outer radius (default: 12)");
    println!("  --max-sources <count>     maximum number of sources (default: 64)");
    println!();
    println!("Acceptance thresholds:");
    println!("  --max-fwhm <px>           maximum median FWHM");
    println!("  --min-roundness <value>   minimum median roundness");
    println!("  --min-snr <value>         minimum median signal to noise ratio");
    println!("  --max-background <adu>    maximum background level");
    println!("  --saturation <adu>        saturation limit");
    println!("  --min-stars <count>       minimum number of detected stars");
    println!();
    println!("Output:");
    println!("  --output-dir <path>       write frame_quality.json into this directory");
    println!("  --json                    print the JSON report on stdout");
}

fn analyse(options: &CliOptions) -> Result<FrameAnalysis, String> {
    let path = options
        .frame_path
        .as_ref()
        .ok_or_else(|| "no frame supplied: use --frame <file.fits>".to_string())?;

    let image = read_fits_file(path)?;
    analyze_frame(
        &image.data,
        image.width,
        image.height,
        options.calibration,
        &options.detection,
        &options.thresholds,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let options = parse_cli_args(&args);
    let analysis = analyse(&options).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let summary = &analysis.quality;
    let json_str = frame_quality_to_json(summary);

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
        println!("frame={}", options.frame_path.unwrap_or_default());
        println!("width={}", analysis.width);
        println!("height={}", analysis.height);
        println!("background_median={:.4}", analysis.statistics.median);
        println!("background_sigma={:.4}", analysis.statistics.background_sigma);
        println!("detection_threshold={:.4}", analysis.detection_threshold);
        println!(
            "saturated_pixels={}",
            analysis.statistics.saturated_pixel_count
        );
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
    fn parse_cli_args_reads_frame_and_thresholds() {
        let args = vec![
            "--frame".to_string(),
            "light_001.fits".to_string(),
            "--bias".to_string(),
            "400".to_string(),
            "--detection-sigma".to_string(),
            "6.5".to_string(),
            "--min-snr".to_string(),
            "12".to_string(),
            "--min-stars".to_string(),
            "4".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.frame_path.as_deref(), Some("light_001.fits"));
        assert!((parsed.calibration.bias - 400.0).abs() < 1e-9);
        assert!((parsed.detection.detection_sigma - 6.5).abs() < 1e-9);
        assert!((parsed.thresholds.min_snr - 12.0).abs() < 1e-9);
        assert_eq!(parsed.thresholds.min_star_count, 4);
        assert!(parsed.json_stdout);
    }

    #[test]
    fn analysis_requires_a_frame() {
        let error = analyse(&CliOptions::default()).unwrap_err();
        assert!(error.contains("--frame"), "{error}");
    }

    #[test]
    fn a_missing_frame_is_reported() {
        let options = CliOptions {
            frame_path: Some("absent_frame.fits".to_string()),
            ..CliOptions::default()
        };
        let error = analyse(&options).unwrap_err();
        assert!(error.contains("absent_frame.fits"), "{error}");
    }
}
