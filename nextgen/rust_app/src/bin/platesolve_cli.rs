use std::path::PathBuf;

use observatory_core::{
    analyze_frame, blind_solver_result_to_json, read_fits_file, solve_blind_astrometry,
    CalibrationFrame, CatalogSource, DetectedSource, DetectionParams, QcThresholds, Table,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    catalog_path: Option<String>,
    detections_path: Option<String>,
    frame_path: Option<String>,
    calibration: CalibrationFrame,
    detection: DetectionParams,
    scale: f64,
    tolerance: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            catalog_path: None,
            detections_path: None,
            frame_path: None,
            calibration: CalibrationFrame {
                bias: 0.0,
                dark_current: 0.0,
                flat_field: 1.0,
            },
            detection: DetectionParams::default(),
            scale: 1.0,
            tolerance: 0.015,
            output_dir: None,
            json_stdout: false,
        }
    }
}

fn require_value<'a>(args: &'a [String], index: usize, flag: &str) -> &'a str {
    match args.get(index + 1) {
        Some(value) => value.as_str(),
        None => {
            eprintln!("missing value after {flag}");
            std::process::exit(1);
        }
    }
}

fn parse_number<T: std::str::FromStr>(raw: &str, flag: &str) -> T {
    raw.parse::<T>().unwrap_or_else(|_| {
        eprintln!("invalid value '{raw}' for {flag}");
        std::process::exit(1);
    })
}

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--catalog" => {
                options.catalog_path = Some(require_value(args, index, "--catalog").to_string());
                index += 2;
            }
            "--detections" => {
                options.detections_path =
                    Some(require_value(args, index, "--detections").to_string());
                index += 2;
            }
            "--frame" => {
                options.frame_path = Some(require_value(args, index, "--frame").to_string());
                index += 2;
            }
            "--bias" => {
                options.calibration.bias =
                    parse_number(require_value(args, index, "--bias"), "--bias");
                index += 2;
            }
            "--dark" => {
                options.calibration.dark_current =
                    parse_number(require_value(args, index, "--dark"), "--dark");
                index += 2;
            }
            "--flat" => {
                options.calibration.flat_field =
                    parse_number(require_value(args, index, "--flat"), "--flat");
                index += 2;
            }
            "--detection-sigma" => {
                options.detection.detection_sigma = parse_number(
                    require_value(args, index, "--detection-sigma"),
                    "--detection-sigma",
                );
                index += 2;
            }
            "--max-sources" => {
                options.detection.max_sources =
                    parse_number(require_value(args, index, "--max-sources"), "--max-sources");
                index += 2;
            }
            "--scale" => {
                options.scale = parse_number(require_value(args, index, "--scale"), "--scale");
                index += 2;
            }
            "--tolerance" => {
                options.tolerance =
                    parse_number(require_value(args, index, "--tolerance"), "--tolerance");
                index += 2;
            }
            "--output-dir" => {
                options.output_dir = Some(require_value(args, index, "--output-dir").to_string());
                index += 2;
            }
            "--json" => {
                options.json_stdout = true;
                index += 1;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("unexpected argument '{other}'");
                print_usage();
                std::process::exit(1);
            }
        }
    }

    options
}

fn print_usage() {
    println!("Usage:");
    println!("  platesolve_cli --catalog <file> --frame <file.fits> [options]");
    println!("  platesolve_cli --catalog <file> --detections <file> [options]");
    println!();
    println!("Both the reference catalogue and the detected sources come from real data;");
    println!("neither list is ever synthesised.");
    println!();
    println!("Reference catalogue (required):");
    println!("  --catalog <path>          table with columns id, ra_deg, dec_deg, magnitude");
    println!();
    println!("Detected sources, one of the two:");
    println!("  --frame <path>            FITS frame, sources are detected in the image");
    println!("  --detections <path>       table with columns x, y, flux, snr");
    println!();
    println!("Detection when reading a frame:");
    println!("  --bias <adu> --dark <adu> --flat <value>");
    println!("  --detection-sigma <value> detection threshold above the noise (default: 5)");
    println!("  --max-sources <count>     maximum number of sources (default: 64)");
    println!();
    println!("Solver:");
    println!("  --scale <value>           initial pixel scale guess (default: 1)");
    println!("  --tolerance <value>       asterism matching tolerance (default: 0.015)");
    println!();
    println!("Output:");
    println!("  --output-dir <path>       write plate_solve_result.json into this directory");
    println!("  --json                    print the JSON result on stdout");
}

fn load_catalog(path: &str) -> Result<Vec<CatalogSource>, String> {
    let table = Table::read(path)?;
    let id_column = table.column_index_any(&["id", "source_id", "catalog_id"])?;
    let ra_column = table.column_index_any(&["ra_deg", "ra"])?;
    let dec_column = table.column_index_any(&["dec_deg", "dec"])?;
    let magnitude_column = table.column_index_any(&["magnitude", "mag", "vmag"])?;

    let mut sources = Vec::with_capacity(table.row_count());
    for row in 0..table.row_count() {
        sources.push(CatalogSource {
            id: table.integer(row, id_column)?.max(0) as u64,
            ra_deg: table.number(row, ra_column)?,
            dec_deg: table.number(row, dec_column)?,
            magnitude: table.number(row, magnitude_column)?,
        });
    }

    Ok(sources)
}

fn load_detections(path: &str) -> Result<Vec<DetectedSource>, String> {
    let table = Table::read(path)?;
    let x_column = table.column_index_any(&["x", "x_pixel", "xcentroid"])?;
    let y_column = table.column_index_any(&["y", "y_pixel", "ycentroid"])?;
    let flux_column = table.column_index_any(&["flux", "net_flux"])?;
    let snr_column = table.column_index_any(&["snr", "signal_to_noise"])?;

    let mut detections = Vec::with_capacity(table.row_count());
    for row in 0..table.row_count() {
        detections.push(DetectedSource {
            x: table.number(row, x_column)?,
            y: table.number(row, y_column)?,
            flux: table.number(row, flux_column)?,
            snr: table.number(row, snr_column)?,
        });
    }

    Ok(detections)
}

fn detect_in_frame(path: &str, options: &CliOptions) -> Result<Vec<DetectedSource>, String> {
    let image = read_fits_file(path)?;
    let analysis = analyze_frame(
        &image.data,
        image.width,
        image.height,
        options.calibration,
        &options.detection,
        &QcThresholds::default(),
    )?;

    Ok(analysis
        .stars
        .iter()
        .map(|star| DetectedSource {
            x: star.measurement.x,
            y: star.measurement.y,
            flux: star.photometry.net_flux,
            snr: star.measurement.snr,
        })
        .collect())
}

fn gather_inputs(options: &CliOptions) -> Result<(Vec<CatalogSource>, Vec<DetectedSource>), String> {
    let catalog_path = options
        .catalog_path
        .as_ref()
        .ok_or_else(|| "no reference catalogue supplied: use --catalog <file>".to_string())?;
    let catalog = load_catalog(catalog_path)?;

    let detections = match (&options.frame_path, &options.detections_path) {
        (Some(_), Some(_)) => {
            return Err("--frame and --detections are mutually exclusive".to_string())
        }
        (Some(frame), None) => detect_in_frame(frame, options)?,
        (None, Some(detections)) => load_detections(detections)?,
        (None, None) => {
            return Err(
                "no source list supplied: use --frame <file.fits> or --detections <file>"
                    .to_string(),
            )
        }
    };

    if detections.is_empty() {
        return Err("no source available for plate solving".to_string());
    }

    Ok((catalog, detections))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let options = parse_cli_args(&args);
    let (catalog, detected) = gather_inputs(&options).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let result = solve_blind_astrometry(&detected, &catalog, options.scale, options.tolerance)
        .unwrap_or_else(|error| {
            eprintln!("plate solving failed: {error}");
            std::process::exit(1);
        });

    let json_str = blind_solver_result_to_json(&result);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let solve_file = dir.join("plate_solve_result.json");
        if let Err(error) = std::fs::write(&solve_file, &json_str) {
            eprintln!("failed to write plate solve result: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("catalog_sources={}", catalog.len());
        println!("detected_sources={}", detected.len());
        println!("solved={}", result.solved);
        println!("matched_stars_count={}", result.matched_stars_count);
        println!("center_ra_deg={:.6}", result.center_ra_deg);
        println!("center_dec_deg={:.6}", result.center_dec_deg);
        println!("pixel_scale_arcsec={:.4}", result.pixel_scale_arcsec);
        println!("rms_error_arcsec={:.4}", result.rms_error_arcsec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, contents: &str) -> String {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, contents).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn parse_cli_args_reads_input_paths_and_solver_settings() {
        let args = vec![
            "--catalog".to_string(),
            "catalog.csv".to_string(),
            "--detections".to_string(),
            "sources.csv".to_string(),
            "--scale".to_string(),
            "1.35".to_string(),
            "--tolerance".to_string(),
            "0.02".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.catalog_path.as_deref(), Some("catalog.csv"));
        assert_eq!(parsed.detections_path.as_deref(), Some("sources.csv"));
        assert!((parsed.scale - 1.35).abs() < 1e-9);
        assert!((parsed.tolerance - 0.02).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }

    #[test]
    fn loads_a_catalogue_with_aliased_columns() {
        let path = write_temp(
            "platesolve_catalog.csv",
            "# reference stars\nsource_id,ra,dec,vmag\n1001,10.6847,41.2687,7.2\n1002,10.8847,41.2687,8.4\n",
        );
        let catalog = load_catalog(&path).unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].id, 1001);
        assert!((catalog[1].ra_deg - 10.8847).abs() < 1e-9);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loads_detections_from_a_table() {
        let path = write_temp(
            "platesolve_detections.csv",
            "x,y,flux,snr\n250.0,250.0,50000,60\n450.0,250.0,30000,45\n",
        );
        let detections = load_detections(&path).unwrap();
        assert_eq!(detections.len(), 2);
        assert!((detections[1].x - 450.0).abs() < 1e-9);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_catalogue_is_reported() {
        let error = gather_inputs(&CliOptions::default()).unwrap_err();
        assert!(error.contains("--catalog"), "{error}");
    }

    #[test]
    fn a_catalogue_without_a_source_list_is_reported() {
        let path = write_temp(
            "platesolve_catalog_only.csv",
            "id,ra_deg,dec_deg,magnitude\n1,10.0,41.0,7.0\n",
        );
        let options = CliOptions {
            catalog_path: Some(path.clone()),
            ..CliOptions::default()
        };
        let error = gather_inputs(&options).unwrap_err();
        assert!(error.contains("--frame"), "{error}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_column_is_reported_with_the_available_ones() {
        let path = write_temp("platesolve_bad_catalog.csv", "id,ra_deg\n1,10.0\n");
        let error = load_catalog(&path).unwrap_err();
        assert!(error.contains("dec_deg"), "{error}");
        let _ = std::fs::remove_file(path);
    }
}
