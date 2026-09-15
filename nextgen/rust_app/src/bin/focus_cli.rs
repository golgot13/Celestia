use std::path::PathBuf;

use observatory_core::{
    compute_critical_focus_zone_steps, fit_parabolic_v_curve, focus_curve_fit_to_json,
    plan_autofocus_run, FocuserMeasurement,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    center_step: i64,
    span_steps: i64,
    samples: usize,
    focal_ratio: f64,
    microns_per_step: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            center_step: 25000,
            span_steps: 3000,
            samples: 7,
            focal_ratio: 5.0,
            microns_per_step: 2.5,
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
            "--center" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --center");
                    std::process::exit(1);
                }
                options.center_step = args[index + 1].parse::<i64>().unwrap_or(25000);
                index += 2;
            }
            "--span" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --span");
                    std::process::exit(1);
                }
                options.span_steps = args[index + 1].parse::<i64>().unwrap_or(3000);
                index += 2;
            }
            "--samples" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --samples");
                    std::process::exit(1);
                }
                options.samples = args[index + 1].parse::<usize>().unwrap_or(7);
                index += 2;
            }
            "--focal-ratio" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --focal-ratio");
                    std::process::exit(1);
                }
                options.focal_ratio = args[index + 1].parse::<f64>().unwrap_or(5.0);
                index += 2;
            }
            "--step-size" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --step-size");
                    std::process::exit(1);
                }
                options.microns_per_step = args[index + 1].parse::<f64>().unwrap_or(2.5);
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

    let plan = plan_autofocus_run(
        options.center_step,
        options.span_steps,
        options.samples,
        150,
    );

    // Optical simulation of parabolic V-curve: HFD = HFD_min + a * (x - x0)^2
    let true_optimal_step = options.center_step + 45; // slight focus drift
    let min_hfd = 1.85;
    let curvature_a = 0.000006;

    let measurements: Vec<FocuserMeasurement> = plan
        .target_steps
        .iter()
        .map(|&step| {
            let dx = (step - true_optimal_step) as f64;
            let hfd = min_hfd + curvature_a * dx * dx;
            FocuserMeasurement {
                focuser_step: step,
                hfd_pixels: hfd,
                star_flux: 30000.0,
                snr: 50.0,
            }
        })
        .collect();

    let cfz_steps = compute_critical_focus_zone_steps(
        options.focal_ratio,
        550.0,
        options.microns_per_step,
    );

    let fit = fit_parabolic_v_curve(&measurements, cfz_steps).unwrap_or_else(|error| {
        eprintln!("V-curve fit failed: {error}");
        std::process::exit(1);
    });

    let json_str = focus_curve_fit_to_json(&fit);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let focus_file = dir.join("autofocus_vcurve.json");
        if let Err(error) = std::fs::write(&focus_file, &json_str) {
            eprintln!("failed to write autofocus report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("optimal_step={}", fit.optimal_step);
        println!("min_hfd_pixels={:.4}", fit.min_hfd_pixels);
        println!("cfz_steps={:.2}", fit.cfz_steps);
        println!("r_squared={:.4}", fit.r_squared);
        println!("converged={}", fit.converged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_focus_flags() {
        let args = vec![
            "--center".to_string(),
            "30000".to_string(),
            "--span".to_string(),
            "4000".to_string(),
            "--samples".to_string(),
            "9".to_string(),
            "--focal-ratio".to_string(),
            "4.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.center_step, 30000);
        assert_eq!(parsed.span_steps, 4000);
        assert_eq!(parsed.samples, 9);
        assert!((parsed.focal_ratio - 4.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
