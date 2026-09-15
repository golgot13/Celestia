use std::path::PathBuf;

use observatory_core::{
    compute_difference_image_2d, transient_result_to_json,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    width: usize,
    height: usize,
    threshold_sigma: f64,
    add_supernova: bool,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            width: 32,
            height: 32,
            threshold_sigma: 5.0,
            add_supernova: true,
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
            "--sigma" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --sigma");
                    std::process::exit(1);
                }
                options.threshold_sigma = args[index + 1].parse::<f64>().unwrap_or(5.0);
                index += 2;
            }
            "--no-transient" => {
                options.add_supernova = false;
                index += 1;
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
    let mut template = vec![20.0; total_pixels];
    let mut science = vec![20.0; total_pixels];

    // Reference field stars in both template & science
    let cx = options.width / 2;
    let cy = options.height / 2;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let r2 = (dx * dx + dy * dy) as f64;
            let val = 500.0 * (-r2 / 2.0).exp();
            let idx = (cy as isize + dy) as usize * options.width + (cx as isize + dx) as usize;
            template[idx] += val;
            science[idx] += val;
        }
    }

    // Add new transient (Supernova / Asteroid) in science frame only
    if options.add_supernova {
        let sn_x = (cx + 8).min(options.width - 2);
        let sn_y = (cy + 6).min(options.height - 2);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let r2 = (dx * dx + dy * dy) as f64;
                let val = 350.0 * (-r2 / 1.5).exp();
                let idx = (sn_y as isize + dy) as usize * options.width + (sn_x as isize + dx) as usize;
                science[idx] += val;
            }
        }
    }

    let result = compute_difference_image_2d(
        &template,
        &science,
        options.width,
        options.height,
        options.threshold_sigma,
    )
    .unwrap_or_else(|error| {
        eprintln!("image subtraction failed: {error}");
        std::process::exit(1);
    });

    let json_str = transient_result_to_json(&result);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let trans_file = dir.join("transient_candidates.json");
        if let Err(error) = std::fs::write(&trans_file, &json_str) {
            eprintln!("failed to write transient candidates report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("dimensions={}x{}", result.width, result.height);
        println!("background_rms={:.4}", result.background_rms);
        println!("total_candidate_count={}", result.total_candidate_count);
        for c in &result.detected_transients {
            println!(
                "  Candidate #{}: pos=({:.1}, {:.1}), flux={:.1}, SNR={:.1} sigma, type={:?}",
                c.candidate_id, c.x, c.y, c.total_flux, c.significance_sigmas, c.classification
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_transient_flags() {
        let args = vec![
            "--width".to_string(),
            "64".to_string(),
            "--height".to_string(),
            "64".to_string(),
            "--sigma".to_string(),
            "6.0".to_string(),
            "--no-transient".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.width, 64);
        assert_eq!(parsed.height, 64);
        assert!((parsed.threshold_sigma - 6.0).abs() < 1e-9);
        assert!(!parsed.add_supernova);
        assert!(parsed.json_stdout);
    }
}
