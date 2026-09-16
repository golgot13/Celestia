use std::collections::HashMap;
use std::path::PathBuf;

use observatory_core::{
    calibrate_target_magnitude, calibration_result_to_json, solve_zero_point_and_extinction,
    ObservedStarPhotometry, StandardStar, Table,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    standards_path: Option<String>,
    observations_path: Option<String>,
    filter: Option<String>,
    target_name: Option<String>,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            standards_path: None,
            observations_path: None,
            filter: None,
            target_name: None,
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

fn parse_cli_args(args: &[String]) -> CliOptions {
    let mut options = CliOptions::default();

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--standards" => {
                options.standards_path = Some(require_value(args, index, "--standards").to_string());
                index += 2;
            }
            "--observations" => {
                options.observations_path =
                    Some(require_value(args, index, "--observations").to_string());
                index += 2;
            }
            "--filter" => {
                options.filter = Some(require_value(args, index, "--filter").to_string());
                index += 2;
            }
            "--target" => {
                options.target_name = Some(require_value(args, index, "--target").to_string());
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
    println!("  photometry_cli --standards <file> --observations <file> --filter <name> [options]");
    println!();
    println!("The zero point and the extinction coefficient are solved from the measured");
    println!("fluxes supplied in the tables; no measurement is ever synthesised.");
    println!();
    println!("Required inputs:");
    println!("  --standards <path>     table with columns name, ra_deg, dec_deg, filter, magnitude");
    println!("  --observations <path>  table with columns name, filter, flux, exposure_s, airmass");
    println!("  --filter <name>        photometric band to solve");
    println!();
    println!("Optional:");
    println!("  --target <name>        calibrate this observation with the solved zero point");
    println!("  --output-dir <path>    write photometric_calibration.json into this directory");
    println!("  --json                 print the JSON result on stdout");
}

/// Catalogue entries are interned because the core photometry types hold `&'static str`.
fn intern(pool: &mut HashMap<String, &'static str>, value: &str) -> &'static str {
    if let Some(existing) = pool.get(value) {
        return existing;
    }
    let leaked: &'static str = Box::leak(value.to_string().into_boxed_str());
    pool.insert(value.to_string(), leaked);
    leaked
}

fn load_standards(
    path: &str,
    pool: &mut HashMap<String, &'static str>,
) -> Result<Vec<StandardStar>, String> {
    let table = Table::read(path)?;
    let name_column = table.column_index_any(&["name", "star", "star_name"])?;
    let ra_column = table.column_index_any(&["ra_deg", "ra"])?;
    let dec_column = table.column_index_any(&["dec_deg", "dec"])?;
    let filter_column = table.column_index_any(&["filter", "band"])?;
    let magnitude_column = table.column_index_any(&["magnitude", "mag"])?;

    let mut by_name: Vec<StandardStar> = Vec::new();
    for row in 0..table.row_count() {
        let name = intern(pool, table.text(row, name_column)?);
        let filter = intern(pool, table.text(row, filter_column)?);
        let magnitude = table.number(row, magnitude_column)?;
        let ra_deg = table.number(row, ra_column)?;
        let dec_deg = table.number(row, dec_column)?;

        match by_name.iter_mut().find(|star| star.name == name) {
            Some(star) => {
                star.catalog_magnitudes.insert(filter, magnitude);
            }
            None => {
                let mut catalog_magnitudes = HashMap::new();
                catalog_magnitudes.insert(filter, magnitude);
                by_name.push(StandardStar {
                    name,
                    ra_deg,
                    dec_deg,
                    catalog_magnitudes,
                });
            }
        }
    }

    Ok(by_name)
}

fn load_observations(
    path: &str,
    pool: &mut HashMap<String, &'static str>,
) -> Result<Vec<ObservedStarPhotometry>, String> {
    let table = Table::read(path)?;
    let name_column = table.column_index_any(&["name", "star", "star_name"])?;
    let filter_column = table.column_index_any(&["filter", "band"])?;
    let flux_column = table.column_index_any(&["flux", "instrumental_flux", "net_flux"])?;
    let exposure_column = table.column_index_any(&["exposure_s", "exptime", "exposure"])?;
    let airmass_column = table.column_index_any(&["airmass", "x"])?;

    let mut observations = Vec::with_capacity(table.row_count());
    for row in 0..table.row_count() {
        observations.push(ObservedStarPhotometry {
            star_name: intern(pool, table.text(row, name_column)?),
            filter: intern(pool, table.text(row, filter_column)?),
            instrumental_flux: table.number(row, flux_column)?,
            exposure_s: table.number(row, exposure_column)?,
            airmass: table.number(row, airmass_column)?,
        });
    }

    Ok(observations)
}

#[derive(Debug)]
struct Inputs {
    standards: Vec<StandardStar>,
    observations: Vec<ObservedStarPhotometry>,
    filter: String,
}

fn gather_inputs(options: &CliOptions) -> Result<Inputs, String> {
    let standards_path = options
        .standards_path
        .as_ref()
        .ok_or_else(|| "no standard star table supplied: use --standards <file>".to_string())?;
    let observations_path = options
        .observations_path
        .as_ref()
        .ok_or_else(|| "no observation table supplied: use --observations <file>".to_string())?;
    let filter = options
        .filter
        .clone()
        .ok_or_else(|| "no photometric band supplied: use --filter <name>".to_string())?;

    let mut pool = HashMap::new();
    let standards = load_standards(standards_path, &mut pool)?;
    let observations = load_observations(observations_path, &mut pool)?;

    let matching = observations
        .iter()
        .filter(|observation| observation.filter == filter)
        .count();
    if matching == 0 {
        return Err(format!(
            "no observation in band '{filter}' among {} record(s)",
            observations.len()
        ));
    }

    Ok(Inputs {
        standards,
        observations,
        filter,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let options = parse_cli_args(&args);
    let inputs = gather_inputs(&options).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let calibration =
        solve_zero_point_and_extinction(&inputs.observations, &inputs.standards, &inputs.filter)
            .unwrap_or_else(|error| {
                eprintln!("photometric calibration failed: {error}");
                std::process::exit(1);
            });

    let json_str = calibration_result_to_json(&calibration);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let calibration_file = dir.join("photometric_calibration.json");
        if let Err(error) = std::fs::write(&calibration_file, &json_str) {
            eprintln!("failed to write photometric calibration: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("standard_stars={}", inputs.standards.len());
        println!("observations={}", inputs.observations.len());
        println!("filter={}", calibration.filter);
        println!("zero_point_mag={:.4}", calibration.zero_point_mag);
        println!(
            "extinction_coefficient={:.4}",
            calibration.extinction_coefficient
        );
        println!("residual_rms_mag={:.4}", calibration.residual_rms_mag);
        println!("reference_star_count={}", calibration.reference_star_count);
        println!("valid={}", calibration.valid);
    }

    if let Some(target_name) = options.target_name.as_ref() {
        let Some(observation) = inputs
            .observations
            .iter()
            .find(|entry| entry.star_name == target_name && entry.filter == inputs.filter)
        else {
            eprintln!("target '{target_name}' has no observation in band '{}'", inputs.filter);
            std::process::exit(1);
        };

        let calibrated =
            calibrate_target_magnitude(observation, &calibration).unwrap_or_else(|error| {
                eprintln!("target calibration failed: {error}");
                std::process::exit(1);
            });

        println!("target_name={}", calibrated.star_name);
        println!("target_instrumental_mag={:.4}", calibrated.instrumental_mag);
        println!("target_calibrated_mag={:.4}", calibrated.calibrated_mag);
        println!("target_error_mag={:.4}", calibrated.error_estimate_mag);
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
    fn parse_cli_args_reads_table_paths_and_band() {
        let args = vec![
            "--standards".to_string(),
            "std.csv".to_string(),
            "--observations".to_string(),
            "obs.csv".to_string(),
            "--filter".to_string(),
            "V".to_string(),
            "--target".to_string(),
            "NGC7000-1".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(parsed.standards_path.as_deref(), Some("std.csv"));
        assert_eq!(parsed.observations_path.as_deref(), Some("obs.csv"));
        assert_eq!(parsed.filter.as_deref(), Some("V"));
        assert_eq!(parsed.target_name.as_deref(), Some("NGC7000-1"));
        assert!(parsed.json_stdout);
    }

    #[test]
    fn groups_several_bands_of_the_same_standard_star() {
        let path = write_temp(
            "photometry_standards.csv",
            "name,ra_deg,dec_deg,filter,magnitude\nSA104-334,180.0,-0.5,V,12.35\nSA104-334,180.0,-0.5,B,13.10\nSA104-428,181.0,-0.4,V,11.20\n",
        );
        let mut pool = HashMap::new();
        let standards = load_standards(&path, &mut pool).unwrap();
        assert_eq!(standards.len(), 2);
        let first = standards.iter().find(|s| s.name == "SA104-334").unwrap();
        assert_eq!(first.catalog_magnitudes.len(), 2);
        assert!((first.catalog_magnitudes["V"] - 12.35).abs() < 1e-9);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loads_observations_with_aliased_columns() {
        let path = write_temp(
            "photometry_observations.csv",
            "star,band,net_flux,exptime,airmass\nSA104-334,V,150000,60,1.25\n",
        );
        let mut pool = HashMap::new();
        let observations = load_observations(&path, &mut pool).unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].star_name, "SA104-334");
        assert!((observations[0].airmass - 1.25).abs() < 1e-9);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_inputs_are_reported_one_by_one() {
        let error = gather_inputs(&CliOptions::default()).unwrap_err();
        assert!(error.contains("--standards"), "{error}");

        let options = CliOptions {
            standards_path: Some("std.csv".to_string()),
            ..CliOptions::default()
        };
        let error = gather_inputs(&options).unwrap_err();
        assert!(error.contains("--observations"), "{error}");
    }

    #[test]
    fn a_band_without_observation_is_reported() {
        let standards = write_temp(
            "photometry_standards_band.csv",
            "name,ra_deg,dec_deg,filter,magnitude\nSA104-334,180.0,-0.5,V,12.35\n",
        );
        let observations = write_temp(
            "photometry_observations_band.csv",
            "name,filter,flux,exposure_s,airmass\nSA104-334,V,150000,60,1.25\n",
        );

        let options = CliOptions {
            standards_path: Some(standards.clone()),
            observations_path: Some(observations.clone()),
            filter: Some("Ha".to_string()),
            ..CliOptions::default()
        };
        let error = gather_inputs(&options).unwrap_err();
        assert!(error.contains("Ha"), "{error}");

        let _ = std::fs::remove_file(standards);
        let _ = std::fs::remove_file(observations);
    }
}
