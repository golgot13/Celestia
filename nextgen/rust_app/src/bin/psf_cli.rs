use std::path::PathBuf;

use observatory_core::{
    analyze_frame, assess_seeing_quality, read_fits_file, seeing_assessment_to_json,
    CalibrationFrame, DetectedStar, DetectionParams, QcThresholds,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    frame_path: Option<String>,
    plate_scale_arcsec_per_pixel: f64,
    calibration: CalibrationFrame,
    detection: DetectionParams,
    min_snr: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            frame_path: None,
            plate_scale_arcsec_per_pixel: 1.0,
            calibration: CalibrationFrame {
                bias: 0.0,
                dark_current: 0.0,
                flat_field: 1.0,
            },
            detection: DetectionParams::default(),
            min_snr: 5.0,
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
            "--plate-scale" => {
                options.plate_scale_arcsec_per_pixel =
                    parse_number(require_value(args, index, "--plate-scale"), "--plate-scale");
                index += 2;
            }
            "--bias" => {
                options.calibration.bias =
                    parse_number(require_value(args, index, "--bias"), "--bias");
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
            "--max-sources" => {
                options.detection.max_sources =
                    parse_number(require_value(args, index, "--max-sources"), "--max-sources");
                index += 2;
            }
            "--min-snr" => {
                options.min_snr = parse_number(require_value(args, index, "--min-snr"), "--min-snr");
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
    println!("  psf_cli --frame <file.fits> --plate-scale <arcsec/px> [options]");
    println!();
    println!("The seeing is derived from the PSF of the stars actually detected in the");
    println!("frame; no stellar profile is ever synthesised.");
    println!();
    println!("Required:");
    println!("  --frame <path>            FITS frame to measure");
    println!("  --plate-scale <value>     image scale in arcsec per pixel");
    println!();
    println!("Calibration and detection:");
    println!("  --bias <adu>              bias level (default: 0)");
    println!("  --dark <adu>              dark current level (default: 0)");
    println!("  --flat <value>            flat field scale (default: 1)");
    println!("  --detection-sigma <value> detection threshold above the noise (default: 5)");
    println!("  --aperture <px>           photometric aperture radius (default: 4)");
    println!("  --max-sources <count>     maximum number of sources (default: 64)");
    println!("  --min-snr <value>         reject stars below this SNR (default: 5)");
    println!();
    println!("Output:");
    println!("  --output-dir <path>       write seeing_assessment.json into this directory");
    println!("  --json                    print the JSON report on stdout");
}

/// Keeps the stars whose profile is usable for a seeing estimate.
fn usable_stars(stars: &[DetectedStar], min_snr: f64) -> Vec<&DetectedStar> {
    stars
        .iter()
        .filter(|star| {
            star.measurement.snr >= min_snr
                && star.measurement.fwhm_pixels.is_finite()
                && star.measurement.fwhm_pixels > 0.0
        })
        .collect()
}

fn measure(options: &CliOptions) -> Result<(Vec<f64>, usize), String> {
    let path = options
        .frame_path
        .as_ref()
        .ok_or_else(|| "no frame supplied: use --frame <file.fits>".to_string())?;

    if options.plate_scale_arcsec_per_pixel <= 0.0 {
        return Err("--plate-scale must be strictly positive".to_string());
    }

    let image = read_fits_file(path)?;
    let analysis = analyze_frame(
        &image.data,
        image.width,
        image.height,
        options.calibration,
        &options.detection,
        &QcThresholds::default(),
    )?;

    let detected_count = analysis.stars.len();
    let retained = usable_stars(&analysis.stars, options.min_snr);
    if retained.is_empty() {
        return Err(format!(
            "no star above SNR {:.1} in '{path}': {detected_count} source(s) detected",
            options.min_snr
        ));
    }

    let fwhm_samples = retained
        .iter()
        .map(|star| star.measurement.fwhm_pixels)
        .collect();

    Ok((fwhm_samples, detected_count))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let options = parse_cli_args(&args);
    let (fwhm_samples, detected_count) = measure(&options).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let assessment = assess_seeing_quality(&fwhm_samples, options.plate_scale_arcsec_per_pixel);
    let json_str = seeing_assessment_to_json(&assessment);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let seeing_file = dir.join("seeing_assessment.json");
        if let Err(error) = std::fs::write(&seeing_file, &json_str) {
            eprintln!("failed to write seeing assessment: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("frame={}", options.frame_path.unwrap_or_default());
        println!("detected_sources={detected_count}");
        println!("average_fwhm_pixels={:.4}", assessment.average_fwhm_pixels);
        println!("fwhm_arcsec={:.4}", assessment.fwhm_arcsec);
        println!(
            "plate_scale_arcsec_per_pixel={:.4}",
            assessment.plate_scale_arcsec_per_pixel
        );
        println!("star_count={}", assessment.star_count);
        println!("seeing_quality={}", assessment.seeing_quality);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use observatory_core::{AperturePhotometry, StarMeasurement};

    fn star(snr: f64, fwhm: f64) -> DetectedStar {
        DetectedStar {
            measurement: StarMeasurement {
                star_id: 1,
                x: 10.0,
                y: 10.0,
                flux: 1_000.0,
                fwhm_pixels: fwhm,
                roundness: 0.95,
                peak_adu: 5_000.0,
                snr,
            },
            photometry: AperturePhotometry {
                aperture_flux: 1_200.0,
                background_level: 10.0,
                net_flux: 1_000.0,
                signal_to_noise: snr,
            },
            fwhm_x_pixels: fwhm,
            fwhm_y_pixels: fwhm,
            instrumental_magnitude: -7.5,
        }
    }

    #[test]
    fn parse_cli_args_reads_frame_and_plate_scale() {
        let args = vec![
            "--frame".to_string(),
            "light_001.fits".to_string(),
            "--plate-scale".to_string(),
            "0.65".to_string(),
            "--min-snr".to_string(),
            "8".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.frame_path.as_deref(), Some("light_001.fits"));
        assert!((parsed.plate_scale_arcsec_per_pixel - 0.65).abs() < 1e-9);
        assert!((parsed.min_snr - 8.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }

    #[test]
    fn low_signal_and_degenerate_stars_are_discarded() {
        let stars = vec![star(20.0, 2.4), star(2.0, 2.5), star(30.0, 0.0)];
        let retained = usable_stars(&stars, 5.0);
        assert_eq!(retained.len(), 1);
        assert!((retained[0].measurement.fwhm_pixels - 2.4).abs() < 1e-9);
    }

    #[test]
    fn measurement_requires_a_frame_and_a_positive_scale() {
        let error = measure(&CliOptions::default()).unwrap_err();
        assert!(error.contains("--frame"), "{error}");

        let options = CliOptions {
            frame_path: Some("absent.fits".to_string()),
            plate_scale_arcsec_per_pixel: 0.0,
            ..CliOptions::default()
        };
        let error = measure(&options).unwrap_err();
        assert!(error.contains("--plate-scale"), "{error}");
    }
}
