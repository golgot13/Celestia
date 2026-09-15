use std::path::PathBuf;

use observatory_core::{
    assess_seeing_quality, fit_gaussian_profile_1d, seeing_assessment_to_json,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    plate_scale_arcsec_per_pixel: f64,
    samples: Vec<f64>,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            plate_scale_arcsec_per_pixel: 0.75,
            samples: Vec::new(),
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
            "--plate-scale" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --plate-scale");
                    std::process::exit(1);
                }
                options.plate_scale_arcsec_per_pixel = args[index + 1].parse::<f64>().unwrap_or(0.75);
                index += 2;
            }
            "--samples" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --samples");
                    std::process::exit(1);
                }
                options.samples = args[index + 1]
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

    if options.samples.is_empty() {
        // Default synthetic stellar profile sample set (FWHM ~2.3 pixels)
        let sigma = 1.0;
        let amp = 200.0;
        let bg = 15.0;
        let profile: Vec<f64> = (0..17)
            .map(|i| {
                let x = i as f64 - 8.0;
                bg + amp * (-(x * x) / (2.0 * sigma * sigma)).exp()
            })
            .collect();
        let fit = fit_gaussian_profile_1d(&profile, 8.0);
        options.samples.push(fit.fwhm_pixels);
    }

    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);

    let assessment = assess_seeing_quality(
        &options.samples,
        options.plate_scale_arcsec_per_pixel,
    );

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
        println!("average_fwhm_pixels={:.4}", assessment.average_fwhm_pixels);
        println!("fwhm_arcsec={:.4}", assessment.fwhm_arcsec);
        println!("plate_scale_arcsec_per_pixel={:.4}", assessment.plate_scale_arcsec_per_pixel);
        println!("star_count={}", assessment.star_count);
        println!("seeing_quality={}", assessment.seeing_quality);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_psf_options() {
        let args = vec![
            "--plate-scale".to_string(),
            "0.65".to_string(),
            "--samples".to_string(),
            "2.1,2.3,1.9".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.plate_scale_arcsec_per_pixel - 0.65).abs() < 1e-9);
        assert_eq!(parsed.samples.len(), 3);
        assert!(parsed.json_stdout);
    }
}
