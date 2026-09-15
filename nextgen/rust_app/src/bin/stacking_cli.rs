use std::path::PathBuf;

use observatory_core::{
    stack_frames_2d, stacked_result_to_json, StackingMethod, StackingParams,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    width: usize,
    height: usize,
    frame_count: usize,
    method: StackingMethod,
    sigma_clip: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            width: 32,
            height: 32,
            frame_count: 5,
            method: StackingMethod::SigmaClipping,
            sigma_clip: 3.0,
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
            "--width" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --width");
                    std::process::exit(1);
                }
                options.width = args[index + 1].parse::<usize>().unwrap_or(32);
                index += 2;
            }
            "--height" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --height");
                    std::process::exit(1);
                }
                options.height = args[index + 1].parse::<usize>().unwrap_or(32);
                index += 2;
            }
            "--frames" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --frames");
                    std::process::exit(1);
                }
                options.frame_count = args[index + 1].parse::<usize>().unwrap_or(5);
                index += 2;
            }
            "--method" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --method");
                    std::process::exit(1);
                }
                options.method = match args[index + 1].to_lowercase().as_str() {
                    "average" | "mean" => StackingMethod::Average,
                    "median" => StackingMethod::Median,
                    "sigma" | "sigmaclipping" => StackingMethod::SigmaClipping,
                    _ => StackingMethod::SigmaClipping,
                };
                index += 2;
            }
            "--sigma" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --sigma");
                    std::process::exit(1);
                }
                options.sigma_clip = args[index + 1].parse::<f64>().unwrap_or(3.0);
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

    let total_pixels = options.width * options.height;
    let mut frames = Vec::with_capacity(options.frame_count);

    for f in 0..options.frame_count {
        let mut frame = Vec::with_capacity(total_pixels);
        for idx in 0..total_pixels {
            let x = (idx % options.width) as f64;
            let y = (idx / options.width) as f64;
            let dx = x - (options.width as f64 / 2.0);
            let dy = y - (options.height as f64 / 2.0);
            let star = 250.0 * (-((dx * dx + dy * dy) / (2.0 * 2.0 * 2.0))).exp();
            let base = 25.0 + ((idx + f) % 5) as f64 * 0.5;
            frame.push(base + star);
        }

        // Add a simulated cosmic ray hit on frame 2
        if f == 2 && total_pixels > 15 {
            frame[15] = 65000.0;
        }

        frames.push(frame);
    }

    let params = StackingParams {
        method: options.method,
        sigma_clip_low: options.sigma_clip,
        sigma_clip_high: options.sigma_clip,
        max_iterations: 3,
    };

    let result = stack_frames_2d(&frames, options.width, options.height, &params)
        .unwrap_or_else(|error| {
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

    if options.json_stdout {
        print!("{json_str}");
    } else {
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
    fn parse_cli_args_accepts_stacking_options() {
        let args = vec![
            "--width".to_string(),
            "64".to_string(),
            "--height".to_string(),
            "64".to_string(),
            "--frames".to_string(),
            "10".to_string(),
            "--method".to_string(),
            "sigma".to_string(),
            "--sigma".to_string(),
            "2.5".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.width, 64);
        assert_eq!(parsed.height, 64);
        assert_eq!(parsed.frame_count, 10);
        assert_eq!(parsed.method, StackingMethod::SigmaClipping);
        assert!((parsed.sigma_clip - 2.5).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
