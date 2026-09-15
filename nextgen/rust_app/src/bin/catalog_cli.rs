use std::path::PathBuf;

use observatory_core::{
    crossmatch_catalog, CatalogStar,
};

#[derive(Clone, Debug, PartialEq)]
struct CliOptions {
    target_ra: f64,
    target_dec: f64,
    max_arcsec: f64,
    output_dir: Option<String>,
    json_stdout: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            target_ra: 10.6847,
            target_dec: 41.2687,
            max_arcsec: 10.0,
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
                if index + 1 >= args.len() { eprintln!("missing value after --ra"); std::process::exit(1); }
                options.target_ra = args[index + 1].parse::<f64>().unwrap_or(10.6847);
                index += 2;
            }
            "--dec" => {
                if index + 1 >= args.len() { eprintln!("missing value after --dec"); std::process::exit(1); }
                options.target_dec = args[index + 1].parse::<f64>().unwrap_or(41.2687);
                index += 2;
            }
            "--max-arcsec" => {
                if index + 1 >= args.len() { eprintln!("missing value after --max-arcsec"); std::process::exit(1); }
                options.max_arcsec = args[index + 1].parse::<f64>().unwrap_or(10.0);
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

    let catalog = vec![
        CatalogStar {
            id: "HIP 1086",
            ra_deg: 10.6847,
            dec_deg: 41.2687,
            magnitude: 7.2,
            proper_motion_ra_mas_yr: 120.0,
            proper_motion_dec_mas_yr: 50.0,
            epoch_jd: 2450000.0,
        },
        CatalogStar {
            id: "TYC 1234-567-1",
            ra_deg: 12.0,
            dec_deg: 45.0,
            magnitude: 8.4,
            proper_motion_ra_mas_yr: 0.0,
            proper_motion_dec_mas_yr: 0.0,
            epoch_jd: 2450000.0,
        },
    ];

    let matches = crossmatch_catalog(
        "target_1",
        options.target_ra,
        options.target_dec,
        2455000.0,
        &catalog,
        options.max_arcsec,
    );

    let match_entries = matches
        .iter()
        .map(|m| format!(
            "{{\"matched_id\": \"{}\", \"separation_arcsec\": {:.3}, \"match_probability\": {:.3}}}",
            m.matched_id,
            m.separation_arcsec,
            m.match_probability
        ))
        .collect::<Vec<_>>()
        .join(", ");

    let json = format!(
        "{{\n  \"target_ra_deg\": {:.4},\n  \"target_dec_deg\": {:.4},\n  \"max_match_arcsec\": {:.2},\n  \"matches\": [{}]\n}}\n",
        options.target_ra,
        options.target_dec,
        options.max_arcsec,
        match_entries,
    );

    if let Some(output_dir) = options.output_dir.as_ref() {
        let dir = PathBuf::from(output_dir);
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("failed to create output dir '{output_dir}': {error}");
            std::process::exit(1);
        }
        let path = dir.join("catalog_crossmatch.json");
        if let Err(error) = std::fs::write(&path, &json) {
            eprintln!("failed to write crossmatch report: {error}");
            std::process::exit(1);
        }
    }

    if options.json_stdout {
        print!("{json}");
    } else {
        for m in &matches {
            println!("matched_id={} separation_arcsec={:.3} match_probability={:.3}", m.matched_id, m.separation_arcsec, m.match_probability);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_args_accepts_catalog_flags() {
        let args = vec![
            "--ra".to_string(), "11.0".to_string(),
            "--dec".to_string(), "42.0".to_string(),
            "--max-arcsec".to_string(), "15.0".to_string(),
            "--json".to_string(),
        ];
        let parsed = parse_cli_args(&args);
        assert!((parsed.target_ra - 11.0).abs() < 1e-9);
        assert!((parsed.target_dec - 42.0).abs() < 1e-9);
        assert!((parsed.max_arcsec - 15.0).abs() < 1e-9);
        assert!(parsed.json_stdout);
    }
}
