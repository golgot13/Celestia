use std::f64::consts::PI;
use std::path::PathBuf;

use observatory_core::{
    analyze_light_curve, light_curve_analysis_to_json, PhotometricPoint,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    min_period: f64,
    max_period: f64,
    points: usize,
    simulated_period: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            min_period: 0.5,
            max_period: 10.0,
            points: 60,
            simulated_period: 2.5,
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
            "--min-period" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --min-period");
                    std::process::exit(1);
                }
                options.min_period = args[index + 1].parse::<f64>().unwrap_or(0.5);
                index += 2;
            }
            "--max-period" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --max-period");
                    std::process::exit(1);
                }
                options.max_period = args[index + 1].parse::<f64>().unwrap_or(10.0);
                index += 2;
            }
            "--points" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --points");
                    std::process::exit(1);
                }
                options.points = args[index + 1].parse::<usize>().unwrap_or(60);
                index += 2;
            }
            "--sim-period" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --sim-period");
                    std::process::exit(1);
                }
                options.simulated_period = args[index + 1].parse::<f64>().unwrap_or(2.5);
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

    let mut dataset = Vec::with_capacity(options.points);
    for i in 0..options.points {
        let t = 2459000.0 + (i as f64) * 0.15; // Every 3.6 hours
        let phase = 2.0 * PI * (t - 2459000.0) / options.simulated_period;
        let var = 0.25 * phase.sin();
        let target_flux = 10000.0 * (1.0 + var);
        let comp1 = 10000.0 + ((i % 5) as f64) * 2.0;
        let comp2 = 10000.0 - ((i % 5) as f64) * 2.0;

        dataset.push(PhotometricPoint {
            time_jd: t,
            target_flux,
            comparison_fluxes: vec![comp1, comp2],
        });
    }

    let analysis = analyze_light_curve(
        &dataset,
        options.min_period,
        options.max_period,
        250,
    )
    .unwrap_or_else(|error| {
        eprintln!("light curve analysis failed: {error}");
        std::process::exit(1);
    });

    let json_str = light_curve_analysis_to_json(&analysis);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let lc_file = dir.join("light_curve_analysis.json");
        if let Err(error) = std::fs::write(&lc_file, &json_str) {
            eprintln!("failed to write light curve analysis: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("point_count={}", analysis.point_count);
        println!("mean_magnitude={:.4}", analysis.mean_magnitude);
        println!("magnitude_amplitude={:.4}", analysis.magnitude_amplitude);
        println!("best_period_days={:.6}", analysis.best_period_days);
        println!("best_power={:.4}", analysis.best_power);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_lightcurve_options() {
        let args = vec![
            "--min-period".to_string(),
            "0.2".to_string(),
            "--max-period".to_string(),
            "8.0".to_string(),
            "--points".to_string(),
            "100".to_string(),
            "--sim-period".to_string(),
            "3.14".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.min_period - 0.2).abs() < 1e-9);
        assert!((parsed.max_period - 8.0).abs() < 1e-9);
        assert_eq!(parsed.points, 100);
        assert!((parsed.simulated_period - 3.14).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
