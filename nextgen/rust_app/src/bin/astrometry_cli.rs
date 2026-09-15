use std::path::PathBuf;

use observatory_core::{
    pixel_to_world, solve_wcs_from_reference_points, world_to_pixel, PixelCoord, WorldCoord,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    ref_points: Vec<(PixelCoord, WorldCoord)>,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            ref_points: vec![
                (PixelCoord { x: 100.0, y: 200.0 }, WorldCoord { ra_deg: 12.0, dec_deg: 45.0 }),
                (PixelCoord { x: 150.0, y: 250.0 }, WorldCoord { ra_deg: 12.5, dec_deg: 45.5 }),
            ],
            output_dir: None,
            json_stdout: false,
        }
    }
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();
    options.ref_points.clear();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--point" => {
                if index + 1 >= args.len() {
                    eprintln!("missing point value after --point");
                    std::process::exit(1);
                }
                let raw = args[index + 1].clone();
                let parts: Vec<&str> = raw.split(',').collect();
                if parts.len() != 4 {
                    eprintln!("--point expects 'x,y,ra,dec'");
                    std::process::exit(1);
                }
                let x = parts[0].parse::<f64>().unwrap_or(0.0);
                let y = parts[1].parse::<f64>().unwrap_or(0.0);
                let ra = parts[2].parse::<f64>().unwrap_or(0.0);
                let dec = parts[3].parse::<f64>().unwrap_or(0.0);
                options.ref_points.push((PixelCoord { x, y }, WorldCoord { ra_deg: ra, dec_deg: dec }));
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

    let wcs = solve_wcs_from_reference_points(&options.ref_points).unwrap_or_else(|| {
        eprintln!("failed to derive WCS transform from reference points");
        std::process::exit(1);
    });

    let reference = options.ref_points[0].1;
    let world_pixel = world_to_pixel(wcs, reference);
    let recovered = pixel_to_world(wcs, world_pixel);

    let json = format!(
        "{{\n  \"crpix_x\": {:.4},\n  \"crpix_y\": {:.4},\n  \"crval_ra_deg\": {:.6},\n  \"crval_dec_deg\": {:.6},\n  \"cd11\": {:.6},\n  \"cd22\": {:.6},\n  \"recovered_ra_deg\": {:.6},\n  \"recovered_dec_deg\": {:.6}\n}}\n",
        wcs.crpix_x,
        wcs.crpix_y,
        wcs.crval_ra_deg,
        wcs.crval_dec_deg,
        wcs.cd11,
        wcs.cd22,
        recovered.ra_deg,
        recovered.dec_deg,
    );

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }
        let report = dir.join("wcs_solution.json");
        if let Err(error) = std::fs::write(&report, &json) {
            eprintln!("failed to write WCS solution: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json}");
    } else {
        println!("crpix_x={:.4}", wcs.crpix_x);
        println!("crpix_y={:.4}", wcs.crpix_y);
        println!("crval_ra_deg={:.6}", wcs.crval_ra_deg);
        println!("crval_dec_deg={:.6}", wcs.crval_dec_deg);
        println!("cd11={:.6}", wcs.cd11);
        println!("cd22={:.6}", wcs.cd22);
        println!("recovered_ra_deg={:.6}", recovered.ra_deg);
        println!("recovered_dec_deg={:.6}", recovered.dec_deg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reference_point_arguments() {
        let args = vec![
            "--point".to_string(),
            "100,200,12.0,45.0".to_string(),
            "--point".to_string(),
            "150,250,12.5,45.5".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.ref_points.len(), 2);
        assert!(parsed.json_stdout);
    }
}
