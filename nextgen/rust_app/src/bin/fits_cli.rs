use std::path::PathBuf;

use observatory_core::{create_astronomical_fits_image, write_fits_binary};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    target: String,
    filter: String,
    exposure_s: f64,
    jd: f64,
    width: usize,
    height: usize,
    output_path: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            target: "M31".to_string(),
            filter: "R".to_string(),
            exposure_s: 120.0,
            jd: 2459000.5,
            width: 64,
            height: 64,
            output_path: None,
            json_stdout: false,
        }
    }
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--target" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --target");
                    std::process::exit(1);
                }
                options.target = args[index + 1].clone();
                index += 2;
            }
            "--filter" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --filter");
                    std::process::exit(1);
                }
                options.filter = args[index + 1].clone();
                index += 2;
            }
            "--exposure" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --exposure");
                    std::process::exit(1);
                }
                options.exposure_s = args[index + 1].parse::<f64>().unwrap_or(120.0);
                index += 2;
            }
            "--jd" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --jd");
                    std::process::exit(1);
                }
                options.jd = args[index + 1].parse::<f64>().unwrap_or(2459000.5);
                index += 2;
            }
            "--width" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --width");
                    std::process::exit(1);
                }
                options.width = args[index + 1].parse::<usize>().unwrap_or(64);
                index += 2;
            }
            "--height" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --height");
                    std::process::exit(1);
                }
                options.height = args[index + 1].parse::<usize>().unwrap_or(64);
                index += 2;
            }
            "--output" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --output");
                    std::process::exit(1);
                }
                options.output_path = Some(args[index + 1].clone());
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
    let mut data = Vec::with_capacity(total_pixels);

    for idx in 0..total_pixels {
        let x = (idx % options.width) as f64;
        let y = (idx / options.width) as f64;
        let dx = x - (options.width as f64 / 2.0);
        let dy = y - (options.height as f64 / 2.0);
        let star = 500.0 * (-((dx * dx + dy * dy) / (2.0 * 2.5 * 2.5))).exp();
        let sky = 20.0 + ((idx % 7) as f64) * 0.2;
        data.push(sky + star);
    }

    let fits = create_astronomical_fits_image(
        &data,
        options.width,
        options.height,
        &options.target,
        &options.filter,
        options.exposure_s,
        options.jd,
    );

    let output_path = options.output_path.unwrap_or_else(|| "out/frame.fits".to_string());
    if let Some(parent) = PathBuf::from(&output_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let bytes_written = write_fits_binary(&fits, &output_path)
        .unwrap_or_else(|error| {
            eprintln!("FITS serialization failed: {error}");
            std::process::exit(1);
        });

    if options.json_stdout {
        println!(
            "{{\n  \"target\": \"{}\",\n  \"filter\": \"{}\",\n  \"width\": {},\n  \"height\": {},\n  \"bytes_written\": {},\n  \"fits_blocks_2880\": {},\n  \"output_path\": \"{}\"\n}}",
            options.target,
            options.filter,
            options.width,
            options.height,
            bytes_written,
            bytes_written / 2880,
            output_path
        );
    } else {
        println!("target={}", options.target);
        println!("filter={}", options.filter);
        println!("dimensions={}x{}", options.width, options.height);
        println!("bytes_written={}", bytes_written);
        println!("fits_blocks_2880={}", bytes_written / 2880);
        println!("output_path={}", output_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_fits_flags() {
        let args = vec![
            "--target".to_string(),
            "NGC224".to_string(),
            "--filter".to_string(),
            "B".to_string(),
            "--exposure".to_string(),
            "180".to_string(),
            "--width".to_string(),
            "128".to_string(),
            "--height".to_string(),
            "128".to_string(),
            "--output".to_string(),
            "out/ngc224.fits".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.target, "NGC224");
        assert_eq!(parsed.filter, "B");
        assert!((parsed.exposure_s - 180.0).abs() < 1e-9);
        assert_eq!(parsed.width, 128);
        assert_eq!(parsed.height, 128);
        assert_eq!(parsed.output_path, Some("out/ngc224.fits".to_string()));
        assert!(parsed.json_stdout);
    }
}
