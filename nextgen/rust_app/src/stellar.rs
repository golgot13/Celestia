#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectralType {
    O,
    B,
    A,
    F,
    G,
    K,
    M,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StellarClassification {
    pub spectral_type: SpectralType,
    pub luminosity_class: &'static str,
    pub color_index_b_v: f64,
    pub estimated_temperature_k: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhotometricDistance {
    pub apparent_mag: f64,
    pub absolute_mag: f64,
    pub distance_pc: f64,
    pub distance_modulus: f64,
}

pub fn estimate_stellar_temperature_k(color_index_b_v: f64) -> f64 {
    if color_index_b_v <= -0.3 {
        30000.0
    } else if color_index_b_v <= 0.0 {
        18000.0 + (0.0 - color_index_b_v) * 3000.0
    } else if color_index_b_v <= 0.4 {
        9000.0 + (0.4 - color_index_b_v) * 10000.0
    } else if color_index_b_v <= 0.7 {
        6500.0 + (0.7 - color_index_b_v) * 6000.0
    } else if color_index_b_v <= 1.1 {
        5500.0 + (1.1 - color_index_b_v) * 4000.0
    } else if color_index_b_v <= 1.6 {
        4200.0 + (1.6 - color_index_b_v) * 2300.0
    } else {
        2600.0
    }
}

pub fn classify_star_by_color_index(color_index_b_v: f64) -> StellarClassification {
    let (spectral_type, luminosity_class, temp) = if color_index_b_v < -0.3 {
        (SpectralType::O, "V", 30000.0)
    } else if color_index_b_v < 0.0 {
        (SpectralType::B, "V", estimate_stellar_temperature_k(color_index_b_v))
    } else if color_index_b_v < 0.4 {
        (SpectralType::A, "V", estimate_stellar_temperature_k(color_index_b_v))
    } else if color_index_b_v < 0.7 {
        (SpectralType::F, "V", estimate_stellar_temperature_k(color_index_b_v))
    } else if color_index_b_v < 1.1 {
        (SpectralType::G, "V", estimate_stellar_temperature_k(color_index_b_v))
    } else if color_index_b_v < 1.6 {
        (SpectralType::K, "III", estimate_stellar_temperature_k(color_index_b_v))
    } else {
        (SpectralType::M, "V", estimate_stellar_temperature_k(color_index_b_v))
    };

    StellarClassification {
        spectral_type,
        luminosity_class,
        color_index_b_v,
        estimated_temperature_k: temp,
    }
}

pub fn compute_distance_modulus_and_pc(apparent_mag: f64, absolute_mag: f64) -> PhotometricDistance {
    let distance_modulus = apparent_mag - absolute_mag;
    let distance_pc = 10.0_f64.powf(distance_modulus / 5.0);

    PhotometricDistance {
        apparent_mag,
        absolute_mag,
        distance_pc,
        distance_modulus,
    }
}

pub fn stellar_classification_to_json(classification: &StellarClassification) -> String {
    format!(
        "{{\n  \"spectral_type\": \"{:?}\",\n  \"luminosity_class\": \"{}\",\n  \"color_index_b_v\": {:.3},\n  \"estimated_temperature_k\": {:.0}\n}}\n",
        classification.spectral_type,
        classification.luminosity_class,
        classification.color_index_b_v,
        classification.estimated_temperature_k,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_solar_like_star() {
        let solar = classify_star_by_color_index(0.65);
        assert_eq!(solar.spectral_type, SpectralType::F);
        assert_eq!(solar.luminosity_class, "V");
        assert!(solar.estimated_temperature_k > 6500.0 && solar.estimated_temperature_k < 7600.0);
    }

    #[test]
    fn classifies_hot_blue_star() {
        let blue = classify_star_by_color_index(-0.2);
        assert_eq!(blue.spectral_type, SpectralType::B);
        assert!(blue.estimated_temperature_k > 15000.0);
    }

    #[test]
    fn computes_distance_modulus_for_nearby_star() {
        let distance = compute_distance_modulus_and_pc(8.0, 4.8);
        assert!((distance.distance_modulus - 3.2).abs() < 1e-9);
        assert!((distance.distance_pc - 4.365).abs() < 0.05);
    }
}
