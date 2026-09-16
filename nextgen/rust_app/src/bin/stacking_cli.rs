use std::path::{Path, PathBuf};

use observatory_core::{
    create_astronomical_fits_image, read_fits_file, scan_frame_directory, stack_frame_files,
    stacked_result_to_json, write_fits_binary, StackingMethod, StackingParams,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    frame_paths: Vec<String>,
    frame_dir: Option<String>,
    method: StackingMethod,
    sigma_clip_low: f64,
    sigma_clip_high: f64,
    max_iterations: usize,
    output_dir: Option<String>,
    output_fits: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            frame_paths: Vec::new(),
            frame_dir: None,
            method: StackingMethod::SigmaClipping,
            sigma_clip_low: 3.0,
            sigma_clip_high: 3.0,
            max_iterations: 3,
            output_dir: None,
            output_fits: None,
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
                options
                    .frame_paths
                    .push(require_value(args, index, "--frame").to_string());
                index += 2;
            }
            "--frame-dir" => {
                options.frame_dir = Some(require_value(args, index, "--frame-dir").to_string());
                index += 2;
            }
            "--method" => {
                let raw = require_value(args, index, "--method").to_lowercase();
                options.method = match raw.as_str() {
                    "average" | "mean" => StackingMethod::Average,
                    "median" => StackingMethod::Median,
                    "sigma" | "sigmaclipping" | "sigma-clipping" => StackingMethod::SigmaClipping,
                    _ => {
                        eprintln!("unknown --method '{raw}': use average, median or sigma");
                        std::process::exit(1);
                    }
                };
                index += 2;
            }
            "--sigma" => {
                let value = parse_number::<f64>(require_value(args, index, "--sigma"), "--sigma");
                options.sigma_clip_low = value;
                options.sigma_clip_high = value;
                index += 2;
            }
            "--sigma-low" => {
                options.sigma_clip_low =
                    parse_number(require_value(args, index, "--sigma-low"), "--sigma-low");
                index += 2;
            }
            "--sigma-high" => {
                options.sigma_clip_high =
                    parse_number(require_value(args, index, "--sigma-high"), "--sigma-high");
                index += 2;
            }
            "--iterations" => {
                options.max_iterations =
                    parse_number(require_value(args, index, "--iterations"), "--iterations");
                index += 2;
            }
            "--output-dir" => {
                options.output_dir = Some(require_value(args, index, "--output-dir").to_string());
                index += 2;
            }
            "--output-fits" => {
                options.output_fits = Some(require_value(args, index, "--output-fits").to_string());
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
    println!("  stacking_cli --frame <file.fits> --frame <file.fits> [...] [options]");
    println!("  stacking_cli --frame-dir <directory> [options]");
    println!();
    println!("Input (at least two frames are required, no frame is ever synthesised):");
    println!("  --frame <path>          FITS frame to stack, repeat the flag per frame");
    println!("  --frame-dir <path>      stack every FITS frame found in the directory");
    println!();
    println!("Stacking:");
    println!("  --method <name>         average, median or sigma (default: sigma)");
    println!("  --sigma <value>         symmetric clipping threshold (default: 3)");
    println!("  --sigma-low <value>     low clipping threshold");
    println!("  --sigma-high <value>    high clipping threshold");
    println!("  --iterations <count>    sigma clipping iterations (default: 3)");
    println!();
    println!("Output:");
    println!("  --output-dir <path>     write stacked_summary.json into this directory");
    println!("  --output-fits <path>    write the stacked image as a FITS file");
    println!("  --json                  print the JSON summary on stdout");
}

fn resolve_frame_paths(options: &CliOptions) -> Result<Vec<String>, String> {
    let mut paths = options.frame_paths.clone();

    if let Some(directory) = options.frame_dir.as_ref() {
        let entries = scan_frame_directory(directory)?;
        if entries.is_empty() {
            return Err(format!("no FITS frame found in directory '{directory}'"));
        }
        // Oldest first, so the stack follows the acquisition order.
        for entry in entries.into_iter().rev() {
            paths.push(entry.path);
        }
    }

    for path in &paths {
        if !Path::new(path).is_file() {
            return Err(format!("frame '{path}' does not exist"));
        }
    }

    if paths.len() < 2 {
        return Err(format!(
            "stacking requires at least two frames, {} supplied",
            paths.len()
        ));
    }

    Ok(paths)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let options = parse_cli_args(&args);

    let frame_paths = resolve_frame_paths(&options).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let params = StackingParams {
        method: options.method,
        sigma_clip_low: options.sigma_clip_low,
        sigma_clip_high: options.sigma_clip_high,
        max_iterations: options.max_iterations.max(1),
    };

    let result = stack_frame_files(&frame_paths, &params).unwrap_or_else(|error| {
        eprintln!("stacking failed: {error}");
        std::process::exit(1);
    });

    let json_str = stacked_result_to_json(&result);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let stack_file = dir.join("stacked_summary.json");
        if let Err(error) = std::fs::write(&stack_file, &json_str) {
            eprintln!("failed to write stacked summary: {error}");
            std::process::exit(1);
        }
    }

    if let Some(output_fits) = options.output_fits.as_ref() {
        // Metadata is inherited from the first input frame so the stack stays traceable.
        let reference = read_fits_file(&frame_paths[0]).unwrap_or_else(|error| {
            eprintln!("failed to re-read reference frame: {error}");
            std::process::exit(1);
        });
        let object = reference
            .header
            .get_str("OBJECT")
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let filter = reference
            .header
            .get_str("FILTER")
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let exposure_s =
            reference.header.get_float("EXPTIME").unwrap_or(0.0) * result.frame_count as f64;
        let julian_day = reference.header.get_float("JD").unwrap_or(0.0);

        let image = create_astronomical_fits_image(
            &result.stacked_image,
            result.width,
            result.height,
            &object,
            &filter,
            exposure_s,
            julian_day,
        );
        if let Err(error) = write_fits_binary(&image, output_fits) {
            eprintln!("failed to write stacked FITS: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        for path in &frame_paths {
            println!("input_frame={path}");
        }
        println!("width={}", result.width);
        println!("height={}", result.height);
        println!("frame_count={}", result.frame_count);
        println!("noise_std_dev={:.6}", result.noise_std_dev);
        println!("snr_improvement_factor={:.4}", result.snr_improvement_factor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_collects_repeated_frames_and_options() {
        let args = vec![
            "--frame".to_string(),
            "a.fits".to_string(),
            "--frame".to_string(),
            "b.fits".to_string(),
            "--method".to_string(),
            "median".to_string(),
            "--sigma-low".to_string(),
            "2.5".to_string(),
            "--sigma-high".to_string(),
            "4.0".to_string(),
            "--iterations".to_string(),
            "5".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.frame_paths, vec!["a.fits", "b.fits"]);
        assert_eq!(parsed.method, StackingMethod::Median);
        assert!((parsed.sigma_clip_low - 2.5).abs() < 1e-9);
        assert!((parsed.sigma_clip_high - 4.0).abs() < 1e-9);
        assert_eq!(parsed.max_iterations, 5);
        assert!(parsed.json_stdout);
    }

    #[test]
    fn a_single_frame_is_refused() {
        let options = CliOptions {
            frame_paths: vec!["only.fits".to_string()],
            ..CliOptions::default()
        };
        let error = resolve_frame_paths(&options).unwrap_err();
        assert!(error.contains("does not exist") || error.contains("at least two"));
    }

    #[test]
    fn missing_frames_are_reported_instead_of_being_generated() {
        let options = CliOptions {
            frame_paths: vec!["absent_a.fits".to_string(), "absent_b.fits".to_string()],
            ..CliOptions::default()
        };
        let error = resolve_frame_paths(&options).unwrap_err();
        assert!(error.contains("absent_a.fits"), "{error}");
    }

    #[test]
    fn an_unknown_directory_is_reported() {
        let options = CliOptions {
            frame_dir: Some("directory_that_does_not_exist".to_string()),
            ..CliOptions::default()
        };
        assert!(resolve_frame_paths(&options).is_err());
    }
}
