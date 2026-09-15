use std::path::PathBuf;

use observatory_core::{
    convolve_2d_separable, deconvolution_result_to_json, generate_gaussian_kernel_1d,
    richardson_lucy_deconvolve_2d, DeconvolutionMethod, DeconvolutionParams,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    width: usize,
    height: usize,
    iterations: usize,
    psf_sigma: f64,
    psf_radius: usize,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            width: 32,
            height: 32,
            iterations: 15,
            psf_sigma: 1.2,
            psf_radius: 3,
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
            "--iterations" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --iterations");
                    std::process::exit(1);
                }
                options.iterations = args[index + 1].parse::<usize>().unwrap_or(15);
                index += 2;
            }
            "--sigma" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --sigma");
                    std::process::exit(1);
                }
                options.psf_sigma = args[index + 1].parse::<f64>().unwrap_or(1.2);
                index += 2;
            }
            "--radius" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --radius");
                    std::process::exit(1);
                }
                options.psf_radius = args[index + 1].parse::<usize>().unwrap_or(3);
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
    let mut sharp_image = vec![10.0; total_pixels];

    // Create central point star and close companion (binary system test)
    let cx = options.width / 2;
    let cy = options.height / 2;
    sharp_image[cy * options.width + cx] = 1000.0;
    if cx + 3 < options.width {
        sharp_image[cy * options.width + (cx + 3)] = 350.0; // Close companion
    }

    let psf_kernel = generate_gaussian_kernel_1d(options.psf_radius, options.psf_sigma);
    let blurred = convolve_2d_separable(&sharp_image, options.width, options.height, &psf_kernel);

    let params = DeconvolutionParams {
        method: DeconvolutionMethod::RichardsonLucy,
        iterations: options.iterations,
        regularization_factor: 0.0005,
        positivity_constraint: true,
    };

    let result = richardson_lucy_deconvolve_2d(
        &blurred,
        options.width,
        options.height,
        &psf_kernel,
        &params,
    )
    .unwrap_or_else(|error| {
        eprintln!("deconvolution failed: {error}");
        std::process::exit(1);
    });

    let json_str = deconvolution_result_to_json(&result);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let deconv_file = dir.join("deconvolution_result.json");
        if let Err(error) = std::fs::write(&deconv_file, &json_str) {
            eprintln!("failed to write deconvolution report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("dimensions={}x{}", result.width, result.height);
        println!("iterations_completed={}", result.iterations_completed);
        println!("flux_conservation_ratio={:.6}", result.flux_conservation_ratio);
        println!("final_residual_rms={:.6}", result.final_residual_rms);
        println!("contrast_improvement_ratio={:.4}", result.contrast_improvement_ratio);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_deconvolution_flags() {
        let args = vec![
            "--width".to_string(),
            "48".to_string(),
            "--height".to_string(),
            "48".to_string(),
            "--iterations".to_string(),
            "20".to_string(),
            "--sigma".to_string(),
            "1.5".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.width, 48);
        assert_eq!(parsed.height, 48);
        assert_eq!(parsed.iterations, 20);
        assert!((parsed.psf_sigma - 1.5).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
