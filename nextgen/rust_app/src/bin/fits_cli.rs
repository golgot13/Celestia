use observatory_core::{
    compute_frame_statistics, create_astronomical_fits_image, read_fits_file, read_ppm_file,
    write_fits_binary, FitsImage,
};

#[derive(Clone, Debug, PartialEq)]
enum Command {
    Inspect { path: String },
    ConvertPpm { source: String, output: String },
}

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    command: Option<Command>,
    object: String,
    filter: String,
    exposure_s: f64,
    julian_day: f64,
    saturation_limit_adu: f64,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            command: None,
            object: "UNKNOWN".to_string(),
            filter: "UNKNOWN".to_string(),
            exposure_s: 0.0,
            julian_day: 0.0,
            saturation_limit_adu: 65_535.0,
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
    let mut source_ppm: Option<String> = None;
    let mut output_path: Option<String> = None;

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--inspect" => {
                options.command = Some(Command::Inspect {
                    path: require_value(args, index, "--inspect").to_string(),
                });
                index += 2;
            }
            "--from-ppm" => {
                source_ppm = Some(require_value(args, index, "--from-ppm").to_string());
                index += 2;
            }
            "--output" => {
                output_path = Some(require_value(args, index, "--output").to_string());
                index += 2;
            }
            "--object" => {
                options.object = require_value(args, index, "--object").to_string();
                index += 2;
            }
            "--filter" => {
                options.filter = require_value(args, index, "--filter").to_string();
                index += 2;
            }
            "--exposure" => {
                options.exposure_s =
                    parse_number(require_value(args, index, "--exposure"), "--exposure");
                index += 2;
            }
            "--jd" => {
                options.julian_day = parse_number(require_value(args, index, "--jd"), "--jd");
                index += 2;
            }
            "--saturation" => {
                options.saturation_limit_adu =
                    parse_number(require_value(args, index, "--saturation"), "--saturation");
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

    if let Some(source) = source_ppm {
        let Some(output) = output_path else {
            eprintln!("--from-ppm requires --output <file.fits>");
            std::process::exit(1);
        };
        if options.command.is_some() {
            eprintln!("--inspect and --from-ppm are mutually exclusive");
            std::process::exit(1);
        }
        options.command = Some(Command::ConvertPpm { source, output });
    }

    options
}

fn print_usage() {
    println!("Usage:");
    println!("  fits_cli --inspect <file.fits> [--saturation <adu>] [--json]");
    println!("  fits_cli --from-ppm <file.ppm> --output <file.fits> [metadata]");
    println!();
    println!("This tool only reads and converts existing images; it never generates");
    println!("pixel data of its own.");
    println!();
    println!("Inspection:");
    println!("  --inspect <path>      read a FITS image and report its header and statistics");
    println!("  --saturation <adu>    saturation level used for the pixel census");
    println!();
    println!("Conversion of a real PPM image into FITS:");
    println!("  --from-ppm <path>     binary PPM (P6) source image");
    println!("  --output <path>       destination FITS file");
    println!("  --object <name>       OBJECT keyword written into the header");
    println!("  --filter <name>       FILTER keyword written into the header");
    println!("  --exposure <seconds>  EXPTIME keyword written into the header");
    println!("  --jd <value>          JD keyword written into the header");
    println!();
    println!("  --json                print the report as JSON on stdout");
}

fn report_to_json(path: &str, image: &FitsImage, saturation_limit_adu: f64) -> String {
    let statistics = compute_frame_statistics(&image.data, saturation_limit_adu);
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!("  \"path\": \"{}\",\n", path.replace('\\', "/")));
    json.push_str(&format!("  \"width\": {},\n", image.width));
    json.push_str(&format!("  \"height\": {},\n", image.height));
    json.push_str(&format!("  \"bitpix\": {:?},\n", image.bitpix));
    json.push_str(&format!("  \"header_cards\": {},\n", image.header.cards.len()));
    json.push_str(&format!("  \"minimum\": {:.6},\n", statistics.minimum));
    json.push_str(&format!("  \"maximum\": {:.6},\n", statistics.maximum));
    json.push_str(&format!("  \"median\": {:.6},\n", statistics.median));
    json.push_str(&format!(
        "  \"background_sigma\": {:.6},\n",
        statistics.background_sigma
    ));
    json.push_str(&format!(
        "  \"saturated_pixel_count\": {}\n",
        statistics.saturated_pixel_count
    ));
    json.push_str("}\n");
    json
}

fn inspect(path: &str, options: &CliOptions) -> Result<(), String> {
    let image = read_fits_file(path)?;

    if options.json_stdout {
        print!("{}", report_to_json(path, &image, options.saturation_limit_adu));
        return Ok(());
    }

    let statistics = compute_frame_statistics(&image.data, options.saturation_limit_adu);
    println!("path={path}");
    println!("width={}", image.width);
    println!("height={}", image.height);
    println!("bitpix={:?}", image.bitpix);
    println!("header_cards={}", image.header.cards.len());
    for card in &image.header.cards {
        match &card.comment {
            Some(comment) => println!(
                "  {:8}= {:<24} / {comment}",
                card.keyword.trim(),
                card.value
            ),
            None => println!("  {:8}= {}", card.keyword.trim(), card.value),
        }
    }
    println!("minimum={:.6}", statistics.minimum);
    println!("maximum={:.6}", statistics.maximum);
    println!("median={:.6}", statistics.median);
    println!("background_sigma={:.6}", statistics.background_sigma);
    println!("saturated_pixel_count={}", statistics.saturated_pixel_count);

    Ok(())
}

fn convert_ppm(source: &str, output: &str, options: &CliOptions) -> Result<usize, String> {
    let ppm = read_ppm_file(source)?;
    let luminance = ppm.luminance();

    let image = create_astronomical_fits_image(
        &luminance,
        ppm.width as usize,
        ppm.height as usize,
        &options.object,
        &options.filter,
        options.exposure_s,
        options.julian_day,
    );

    write_fits_binary(&image, output)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        std::process::exit(1);
    }

    let options = parse_cli_args(&args);

    let Some(command) = options.command.clone() else {
        eprintln!("no operation requested: use --inspect or --from-ppm");
        print_usage();
        std::process::exit(1);
    };

    match command {
        Command::Inspect { path } => {
            if let Err(error) = inspect(&path, &options) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Command::ConvertPpm { source, output } => {
            let written = convert_ppm(&source, &output, &options).unwrap_or_else(|error| {
                eprintln!("{error}");
                std::process::exit(1);
            });
            if options.json_stdout {
                print!(
                    "{{\n  \"source\": \"{}\",\n  \"output\": \"{}\",\n  \"bytes_written\": {}\n}}\n",
                    source.replace('\\', "/"),
                    output.replace('\\', "/"),
                    written
                );
            } else {
                println!("source={source}");
                println!("output={output}");
                println!("bytes_written={written}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_selects_the_inspection_command() {
        let args = vec![
            "--inspect".to_string(),
            "frame.fits".to_string(),
            "--saturation".to_string(),
            "60000".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(
            parsed.command,
            Some(Command::Inspect {
                path: "frame.fits".to_string()
            })
        );
        assert!((parsed.saturation_limit_adu - 60_000.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }

    #[test]
    fn parse_cli_args_selects_the_conversion_command_with_metadata() {
        let args = vec![
            "--from-ppm".to_string(),
            "source.ppm".to_string(),
            "--output".to_string(),
            "target.fits".to_string(),
            "--object".to_string(),
            "M13".to_string(),
            "--filter".to_string(),
            "V".to_string(),
            "--exposure".to_string(),
            "45".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert_eq!(
            parsed.command,
            Some(Command::ConvertPpm {
                source: "source.ppm".to_string(),
                output: "target.fits".to_string()
            })
        );
        assert_eq!(parsed.object, "M13");
        assert_eq!(parsed.filter, "V");
        assert!((parsed.exposure_s - 45.0).abs() < 1e-9);
    }

    #[test]
    fn inspecting_a_missing_file_is_reported() {
        let options = CliOptions::default();
        let error = inspect("absent_frame.fits", &options).unwrap_err();
        assert!(error.contains("absent_frame.fits"), "{error}");
    }

    #[test]
    fn converts_a_real_ppm_into_a_readable_fits_file() {
        let directory = std::env::temp_dir();
        let ppm_path = directory.join("fits_cli_source.ppm");
        let fits_path = directory.join("fits_cli_target.fits");

        // 2x2 PPM: red, green, blue, white.
        let ppm_bytes: Vec<u8> = b"P6\n2 2\n255\n"
            .iter()
            .copied()
            .chain([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            .collect();
        std::fs::write(&ppm_path, ppm_bytes).unwrap();

        let options = CliOptions {
            object: "TestField".to_string(),
            filter: "L".to_string(),
            exposure_s: 12.5,
            julian_day: 2_460_000.5,
            ..CliOptions::default()
        };

        let written = convert_ppm(
            ppm_path.to_str().unwrap(),
            fits_path.to_str().unwrap(),
            &options,
        )
        .unwrap();
        assert_eq!(written % 2880, 0);

        let decoded = read_fits_file(fits_path.to_str().unwrap()).unwrap();
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.header.get_str("OBJECT").as_deref(), Some("TestField"));
        assert!((decoded.data[0] - 0.2126 * 255.0).abs() < 1e-6);
        assert!((decoded.data[3] - 255.0).abs() < 1e-6);

        let _ = std::fs::remove_file(ppm_path);
        let _ = std::fs::remove_file(fits_path);
    }
}
