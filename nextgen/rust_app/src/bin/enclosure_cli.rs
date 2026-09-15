use std::path::PathBuf;

use observatory_core::{
    enclosure_report_to_json, evaluate_weather_safety, EnclosureState, WeatherLimits,
    WeatherTelemetry,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    temp_c: f64,
    sky_temp_c: f64,
    humidity_pct: f64,
    wind_speed: f64,
    wind_gust: f64,
    rain: bool,
    enclosure: EnclosureState,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            temp_c: 12.0,
            sky_temp_c: -25.0,
            humidity_pct: 55.0,
            wind_speed: 15.0,
            wind_gust: 22.0,
            rain: false,
            enclosure: EnclosureState::Closed,
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
            "--temp" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --temp");
                    std::process::exit(1);
                }
                options.temp_c = args[index + 1].parse::<f64>().unwrap_or(12.0);
                index += 2;
            }
            "--sky-temp" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --sky-temp");
                    std::process::exit(1);
                }
                options.sky_temp_c = args[index + 1].parse::<f64>().unwrap_or(-25.0);
                index += 2;
            }
            "--humidity" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --humidity");
                    std::process::exit(1);
                }
                options.humidity_pct = args[index + 1].parse::<f64>().unwrap_or(55.0);
                index += 2;
            }
            "--wind" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --wind");
                    std::process::exit(1);
                }
                options.wind_speed = args[index + 1].parse::<f64>().unwrap_or(15.0);
                index += 2;
            }
            "--gust" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --gust");
                    std::process::exit(1);
                }
                options.wind_gust = args[index + 1].parse::<f64>().unwrap_or(22.0);
                index += 2;
            }
            "--rain" => {
                options.rain = true;
                index += 1;
            }
            "--state" => {
                if index + 1 >= args.len() {
                    eprintln!("missing value after --state");
                    std::process::exit(1);
                }
                options.enclosure = match args[index + 1].to_lowercase().as_str() {
                    "open" => EnclosureState::Open,
                    "opening" => EnclosureState::Opening,
                    "closing" => EnclosureState::Closing,
                    "error" => EnclosureState::Error,
                    _ => EnclosureState::Closed,
                };
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

    let telemetry = WeatherTelemetry {
        ambient_temp_c: options.temp_c,
        sky_temp_c: options.sky_temp_c,
        relative_humidity_pct: options.humidity_pct,
        wind_speed_km_h: options.wind_speed,
        wind_gust_km_h: options.wind_gust,
        rain_detected: options.rain,
        sky_brightness_mpsas: 21.4,
    };

    let limits = WeatherLimits::default();
    let report = evaluate_weather_safety(&telemetry, &limits, options.enclosure);

    let json_str = enclosure_report_to_json(&report);

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }

        let report_file = dir.join("weather_enclosure_report.json");
        if let Err(error) = std::fs::write(&report_file, &json_str) {
            eprintln!("failed to write enclosure report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json_str}");
    } else {
        println!("weather_status={:?}", report.weather_status);
        println!("enclosure_state={:?}", report.enclosure_state);
        println!("can_open_shutter={}", report.can_open_shutter);
        println!("dew_point_c={:.2}", report.dew_point_c);
        println!("dew_point_margin_c={:.2}", report.dew_point_margin_c);
        println!("cloud_cover_delta_c={:.2}", report.cloud_cover_delta_c);
        for reason in &report.safety_reasons {
            println!("  [Alarm] {}", reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_enclosure_options() {
        let args = vec![
            "--temp".to_string(),
            "18.5".to_string(),
            "--humidity".to_string(),
            "62.0".to_string(),
            "--wind".to_string(),
            "20.0".to_string(),
            "--gust".to_string(),
            "30.0".to_string(),
            "--rain".to_string(),
            "--state".to_string(),
            "open".to_string(),
            "--json".to_string(),
        ];

        let parsed = parse_cli_args(&args);
        assert!((parsed.temp_c - 18.5).abs() < 1e-9);
        assert!((parsed.humidity_pct - 62.0).abs() < 1e-9);
        assert!(parsed.rain);
        assert_eq!(parsed.enclosure, EnclosureState::Open);
        assert!(parsed.json_stdout);
    }
}
