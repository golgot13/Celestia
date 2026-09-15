use std::path::PathBuf;

use observatory_core::{generate_mosaic_grid, mosaic_plan_to_json, MosaicGridConfig};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    center_ra: f64,
    center_dec: f64,
    fov_w: f64,
    fov_h: f64,
    cols: usize,
    rows: usize,
    overlap: f64,
    pa: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            center_ra: 10.6847,
            center_dec: 41.2687,
            fov_w: 60.0,
            fov_h: 40.0,
            cols: 2,
            rows: 2,
            overlap: 15.0,
            pa: 0.0,
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
            "--ra" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --ra");
                    std::process::exit(1);
                }
                options.center_ra = args[index + 1].parse::<f64>().unwrap_or(10.6847);
                index += 2;
            }
            "--dec" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --dec");
                    std::process::exit(1);
                }
                options.center_dec = args[index + 1].parse::<f64>().unwrap_or(41.2687);
                index += 2;
            }
            "--fov-w" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --fov-w");
                    std::process::exit(1);
                }
                options.fov_w = args[index + 1].parse::<f64>().unwrap_or(60.0);
                index += 2;
            }
            "--fov-h" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --fov-h");
                    std::process::exit(1);
                }
                options.fov_h = args[index + 1].parse::<f64>().unwrap_or(40.0);
                index += 2;
            }
            "--cols" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --cols");
                    std::process::exit(1);
                }
                options.cols = args[index + 1].parse::<usize>().unwrap_or(2);
                index += 2;
            }
            "--rows" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --rows");
                    std::process::exit(1);
                }
                options.rows = args[index + 1].parse::<usize>().unwrap_or(2);
                index += 2;
            }
            "--overlap" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --overlap");
                    std::process::exit(1);
                }
                options.overlap = args[index + 1].parse::<f64>().unwrap_or(15.0);
                index += 2;
            }
            "--pa" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --pa");
                    std::process::exit(1);
                }
                options.pa = args[index + 1].parse::<f64>().unwrap_or(0.0);
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

    let config = MosaicGridConfig {
        center_ra_deg: options.center_ra,
        center_dec_deg: options.center_dec,
        fov_width_arcmin: options.fov_w,
        fov_height_arcmin: options.fov_h,
        columns: options.cols,
        rows: options.rows,
        overlap_percentage: options.overlap,
        position_angle_deg: options.pa,
    };

    let plan = generate_mosaic_grid(&config).unwrap_or_else(|error| {
        eprintln!("mosaic generation failed: {error}");
        std::process::exit(1);
    });

    let json_str = mosaic_plan_to_json(&plan);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let mosaic_file = dir.join("mosaic_plan.json");
        if let Err(error) = std::fs::write(&mosaic_file, &json_str) {
            eprintln!("failed to write mosaic plan: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("total_tiles={}", plan.total_tiles);
        println!("total_width_arcmin={:.2}", plan.total_width_arcmin);
        println!("total_height_arcmin={:.2}", plan.total_height_arcmin);
        println!("total_area_sq_deg={:.4}", plan.total_area_sq_deg);
        for t in &plan.tiles {
            println!(
                "  Tile #{:02} [R{}, C{}]: RA={:.4} deg, Dec={:.4} deg",
                t.tile_index, t.row, t.column, t.center_ra_deg, t.center_dec_deg
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_mosaic_flags() {
        let args = vec![
            "--ra".to_string(),
            "12.5".to_string(),
            "--dec".to_string(),
            "42.0".to_string(),
            "--cols".to_string(),
            "3".to_string(),
            "--rows".to_string(),
            "2".to_string(),
            "--overlap".to_string(),
            "10.0".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.center_ra - 12.5).abs() < 1e-9);
        assert!((parsed.center_dec - 42.0).abs() < 1e-9);
        assert_eq!(parsed.cols, 3);
        assert_eq!(parsed.rows, 2);
        assert!((parsed.overlap - 10.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
