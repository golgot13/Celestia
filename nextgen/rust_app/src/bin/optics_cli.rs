use std::path::PathBuf;

use observatory_core::{
    analyze_optical_wavefront, optical_wavefront_summary_to_json, trace_pupil_spot_diagram,
    WavefrontAberrations,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    focal_ratio: f64,
    focal_length_mm: f64,
    wavelength_nm: f64,
    defocus: f64,
    astigmatism: f64,
    coma: f64,
    spherical: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            focal_ratio: 5.0,
            focal_length_mm: 1000.0,
            wavelength_nm: 550.0,
            defocus: 0.0,
            astigmatism: 0.0,
            coma: 0.0,
            spherical: 0.0,
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
            "--focal-ratio" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --focal-ratio");
                    std::process::exit(1);
                }
                options.focal_ratio = args[index + 1].parse::<f64>().unwrap_or(5.0);
                index += 2;
            }
            "--focal-length" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --focal-length");
                    std::process::exit(1);
                }
                options.focal_length_mm = args[index + 1].parse::<f64>().unwrap_or(1000.0);
                index += 2;
            }
            "--wavelength" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --wavelength");
                    std::process::exit(1);
                }
                options.wavelength_nm = args[index + 1].parse::<f64>().unwrap_or(550.0);
                index += 2;
            }
            "--defocus" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --defocus");
                    std::process::exit(1);
                }
                options.defocus = args[index + 1].parse::<f64>().unwrap_or(0.0);
                index += 2;
            }
            "--astig" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --astig");
                    std::process::exit(1);
                }
                options.astigmatism = args[index + 1].parse::<f64>().unwrap_or(0.0);
                index += 2;
            }
            "--coma" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --coma");
                    std::process::exit(1);
                }
                options.coma = args[index + 1].parse::<f64>().unwrap_or(0.0);
                index += 2;
            }
            "--spherical" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --spherical");
                    std::process::exit(1);
                }
                options.spherical = args[index + 1].parse::<f64>().unwrap_or(0.0);
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

    let aberrations = WavefrontAberrations {
        defocus_z3: options.defocus,
        astigmatism_primary_z4: options.astigmatism,
        coma_horizontal_z6: options.coma,
        spherical_primary_z8: options.spherical,
        ..Default::default()
    };

    let wavefront = analyze_optical_wavefront(&aberrations, 32);
    let (_, spot_metrics) = trace_pupil_spot_diagram(
        &aberrations,
        options.focal_ratio,
        options.focal_length_mm,
        options.wavelength_nm,
        10,
    );

    let json_str = optical_wavefront_summary_to_json(&wavefront, &spot_metrics);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let optics_file = dir.join("optical_wavefront.json");
        if let Err(error) = std::fs::write(&optics_file, &json_str) {
            eprintln!("failed to write optical wavefront report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("rms_wavefront_error_waves={:.4} lambda", wavefront.rms_wavefront_error_waves);
        println!("peak_to_valley_waves={:.4} lambda", wavefront.peak_to_valley_waves);
        println!("strehl_ratio={:.4}", wavefront.strehl_ratio);
        println!("maréchal_diffraction_limited={}", wavefront.maréchal_diffraction_limited);
        println!("dominant_aberration={}", wavefront.dominant_aberration);
        println!("rms_spot_radius={:.3} um", spot_metrics.rms_spot_radius_um);
        println!("airy_disk_radius={:.3} um", spot_metrics.airy_disk_radius_um);
        println!("is_diffraction_limited={}", spot_metrics.is_diffraction_limited);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_optics_options() {
        let args = vec![
            "--focal-ratio".to_string(),
            "4.0".to_string(),
            "--focal-length".to_string(),
            "800.0".to_string(),
            "--spherical".to_string(),
            "0.05".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.focal_ratio - 4.0).abs() < 1e-9);
        assert!((parsed.focal_length_mm - 800.0).abs() < 1e-9);
        assert!((parsed.spherical - 0.05).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
