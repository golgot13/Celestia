#[derive(Clone, Debug, PartialEq)]
pub struct CatalogStar {
    pub id: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub magnitude: f64,
    pub proper_motion_ra_mas_yr: f64,
    pub proper_motion_dec_mas_yr: f64,
    pub epoch_jd: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogMatch {
    pub target_id: &'static str,
    pub matched_id: &'static str,
    pub separation_arcsec: f64,
    pub match_probability: f64,
    pub propagated_ra_deg: f64,
    pub propagated_dec_deg: f64,
}

pub fn propagate_catalog_position(star: &CatalogStar, observation_jd: f64) -> (f64, f64) {
    let dt_years = (observation_jd - star.epoch_jd) / 365.25;
    let dra_deg = (star.proper_motion_ra_mas_yr * dt_years) / 3600000.0;
    let ddec_deg = (star.proper_motion_dec_mas_yr * dt_years) / 3600000.0;
    (star.ra_deg + dra_deg, star.dec_deg + ddec_deg)
}

fn angular_separation_deg(ra1_deg: f64, dec1_deg: f64, ra2_deg: f64, dec2_deg: f64) -> f64 {
    let ra1 = ra1_deg.to_radians();
    let dec1 = dec1_deg.to_radians();
    let ra2 = ra2_deg.to_radians();
    let dec2 = dec2_deg.to_radians();

    let cos_d = (dec1.sin() * dec2.sin()) + (dec1.cos() * dec2.cos() * (ra1 - ra2).cos());
    let d = cos_d.clamp(-1.0, 1.0).acos();
    d.to_degrees()
}

pub fn crossmatch_catalog(
    target_id: &'static str,
    target_ra_deg: f64,
    target_dec_deg: f64,
    observation_jd: f64,
    catalog: &[CatalogStar],
    max_match_separation_arcsec: f64,
) -> Vec<CatalogMatch> {
    let mut matches = Vec::new();
    let max_sep_deg = max_match_separation_arcsec / 3600.0;

    for star in catalog {
        let (ra_prop, dec_prop) = propagate_catalog_position(star, observation_jd);
        let sep_deg = angular_separation_deg(target_ra_deg, target_dec_deg, ra_prop, dec_prop);
        if sep_deg <= max_sep_deg {
            let probability = 1.0 - (sep_deg / max_sep_deg).clamp(0.0, 1.0);
            matches.push(CatalogMatch {
                target_id,
                matched_id: star.id,
                separation_arcsec: sep_deg * 3600.0,
                match_probability: probability,
                propagated_ra_deg: ra_prop,
                propagated_dec_deg: dec_prop,
            });
        }
    }

    matches.sort_by(|a, b| a.separation_arcsec.partial_cmp(&b.separation_arcsec).unwrap());
    matches
}

pub fn estimate_magnitude_limit_for_snr(
    snr: f64,
    sky_background_e_per_sec: f64,
    exposure_time_s: f64,
    gain_e_per_adu: f64,
) -> f64 {
    let noise = (sky_background_e_per_sec * exposure_time_s / gain_e_per_adu).sqrt();
    let flux_ratio = snr * noise;
    2.5 * (flux_ratio.max(1.0).ln() / std::f64::consts::LN_10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn propagates_star_position_and_crossmatches() {
        let stars = vec![
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
            10.6847,
            41.2687,
            2455000.0,
            &stars,
            2.0 * 3600.0,
        );

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].matched_id, "HIP 1086");
        assert!(matches[0].separation_arcsec < 2.0);
        assert!(matches[0].match_probability > 0.9);
    }

    #[test]
    fn estimates_magnitude_limit_for_requested_snr() {
        let limit = estimate_magnitude_limit_for_snr(10.0, 50.0, 120.0, 1.5);
        assert!(limit > 0.0 && limit < 20.0);
    }
}
