use observatory_core::{
    CosmologicalParameters, LinearPerturbationParameters, SpatialGeometry,
};

fn main() {
    let mut model = "flat";
    let mut redshift = 1.0_f64;
    let mut h0 = CosmologicalParameters::PLANCK_2018.h0_km_s_mpc;
    let mut omega_matter = CosmologicalParameters::PLANCK_2018.omega_matter;
    let mut omega_radiation = CosmologicalParameters::PLANCK_2018.omega_radiation;
    let mut omega_lambda = CosmologicalParameters::PLANCK_2018.omega_lambda;
    let mut custom_densities = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        let value = || {
            args.get(index + 1).unwrap_or_else(|| {
                eprintln!("valeur manquante apres {option}");
                std::process::exit(2);
            })
        };
        match option {
            "--model" => model = value(),
            "--z" => redshift = parse(value(), "--z"),
            "--h0" => h0 = parse(value(), "--h0"),
            "--omega-m" => {
                omega_matter = parse(value(), "--omega-m");
                custom_densities = true;
            }
            "--omega-r" => {
                omega_radiation = parse(value(), "--omega-r");
                custom_densities = true;
            }
            "--omega-lambda" => {
                omega_lambda = parse(value(), "--omega-lambda");
                custom_densities = true;
            }
            "--help" | "-h" => {
                print_usage();
                return;
            }
            unknown => {
                eprintln!("option inconnue: {unknown}");
                print_usage();
                std::process::exit(2);
            }
        }
        index += if option == "--help" || option == "-h" { 1 } else { 2 };
    }

    match model {
        "open" if !custom_densities => {
            omega_matter = 0.3;
            omega_radiation = 0.0;
            omega_lambda = 0.5;
        }
        "flat" if !custom_densities => {
            omega_matter = 0.3;
            omega_radiation = 0.0;
            omega_lambda = 0.7;
        }
        "closed" if !custom_densities => {
            omega_matter = 0.8;
            omega_radiation = 0.0;
            omega_lambda = 0.5;
        }
        "open" | "flat" | "closed" => {}
        _ => {
            eprintln!("--model doit valoir open, flat ou closed");
            std::process::exit(2);
        }
    }

    let cosmology = CosmologicalParameters {
        h0_km_s_mpc: h0,
        omega_matter,
        omega_radiation,
        omega_lambda,
    };
    let geometry = match cosmology.geometry() {
        SpatialGeometry::Open => "open",
        SpatialGeometry::Flat => "flat",
        SpatialGeometry::Closed => "closed",
    };
    let hubble = required(cosmology.hubble_at_redshift(redshift));
    let comoving = required(cosmology.comoving_distance_mpc(redshift));
    let luminosity = required(cosmology.luminosity_distance_mpc(redshift));
    let lookback = required(cosmology.lookback_time_gyr(redshift));
    let age = required(cosmology.age_gyr());
    let scale_factor = 1.0 / (1.0 + redshift);
    let growth = required(cosmology.linear_growth_factor(scale_factor));
    let growth_rate = required(cosmology.linear_growth_rate(scale_factor));

    println!("{{");
    println!("  \"geometry\": \"{geometry}\",");
    println!("  \"h0_km_s_mpc\": {h0:.8},");
    println!("  \"omega_matter\": {omega_matter:.8},");
    println!("  \"omega_radiation\": {omega_radiation:.8},");
    println!("  \"omega_lambda\": {omega_lambda:.8},");
    println!("  \"omega_curvature\": {:.8},", cosmology.omega_curvature());
    println!("  \"redshift\": {redshift:.8},");
    println!("  \"hubble_km_s_mpc\": {hubble:.8},");
    println!("  \"comoving_distance_mpc\": {comoving:.8},");
    println!("  \"luminosity_distance_mpc\": {luminosity:.8},");
    println!("  \"lookback_time_gyr\": {lookback:.8},");
    println!("  \"growth_factor\": {growth:.8},");
    println!("  \"growth_rate\": {growth_rate:.8},");
    println!("  \"sigma8\": {:.8},", LinearPerturbationParameters::PLANCK_2018.sigma8);
    println!("  \"scalar_spectral_index\": {:.8},", LinearPerturbationParameters::PLANCK_2018.scalar_spectral_index);
    println!("  \"age_gyr\": {age:.8}");
    println!("}}");
}

fn parse(value: &str, option: &str) -> f64 {
    value.parse::<f64>().unwrap_or_else(|_| {
        eprintln!("valeur invalide pour {option}: {value}");
        std::process::exit(2);
    })
}

fn required<T>(result: Result<T, String>) -> T {
    result.unwrap_or_else(|error| {
        eprintln!("calcul cosmologique impossible: {error}");
        std::process::exit(1);
    })
}

fn print_usage() {
    println!("Usage: cosmology_cli [options]");
    println!("  --model <open|flat|closed>  geometrie spatiale (default: flat)");
    println!("  --z <redshift>              redshift (default: 1)");
    println!("  --h0 <km/s/Mpc>             constante de Hubble");
    println!("  --omega-m <value>           densite de matiere");
    println!("  --omega-r <value>           densite de rayonnement");
    println!("  --omega-lambda <value>      energie noire");
}
