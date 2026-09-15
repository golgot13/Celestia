use std::path::PathBuf;

use observatory_core::{
    classify_star_by_color_index, compute_distance_modulus_and_pc, stellar_classification_to_json,
    StellarClassification,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    color_bv: f64,
    apparent_mag: f64,
    absolute_mag: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            color_bv: 0.65,
            apparent_mag: 8.0,
            absolute_mag: 4.8,
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
            "--bv" => {
                if index + 1 >= args.len() { eprintln!("missing value after --bv"); std::process::exit(1); }
                options.color_bv = args[index + 1].parse::<f64>().unwrap_or(0.65);
                index += 2;
            }
            "--apparent" => {
                if index + 1 >= args.len() { eprintln!("missing value after --apparent"); std::process::exit(1); }
                options.apparent_mag = args[index + 1].parse::<f64>().unwrap_or(8.0);
                index += 2;
            }
            "--absolute" => {
                if index + 1 >= args.len() { eprintln!("missing value after --absolute"); std::process::exit(1); }
                options.absolute_mag = args[index + 1].parse::<f64>().unwrap_or(4.8);
                index += 2;
            }
            "--output-dir" => {
                if index + 1 >= args.len() { eprintln!("missing value after --output-dir"); std::process::exit(1); }
                options.output_dir = Some(args[index + 1].clone());
                index += 2;
            }
            "--json" => {
                options.json_stdout = true;
                index += 1;
            }
            _ => { index += 1; }
        }
    }
    options
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = parse_cli_args(&args);

    let classification: StellarClassification = classify_star_by_color_index(options.color_bv);
    let photometric = compute_distance_modulus_and_pc(options.apparent_mag, options.absolute_mag);
    let json = stellar_classification_to_json(&classification);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }
        let path = dir.join("stellar_classification.json");
        if let Err(error) = std::fs::write(&path, &json) {
            eprintln!("failed to write stellar classification: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json}");
    } else {
        println!("spectral_type={:?}", classification.spectral_type);
        println!("luminosity_class={}", classification.luminosity_class);
        println!("color_index_b_v={:.3}", classification.color_index_b_v);
        println!("estimated_temperature_k={:.0}", classification.estimated_temperature_k);
        println!("distance_modulus={:.3}", photometric.distance_modulus);
        println!("distance_pc={:.3}", photometric.distance_pc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_stellar_flags() {
        let args = vec![
            "--bv".to_string(), "0.6".to_string(),
            "--apparent".to_string(), "9.1".to_string(),
            "--absolute".to_string(), "5.0".to_string(),
            "--json".to_string(),
        ];
        let parsed = parse_cli_args(&args);
        assert!((parsed.color_bv - 0.6).abs() < 1e-9);
        assert!((parsed.apparent_mag - 9.1).abs() < 1e-9);
        assert!((parsed.absolute_mag - 5.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
